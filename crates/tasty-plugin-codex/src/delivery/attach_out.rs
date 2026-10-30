//! attach 원격 출처 부모에게 돌려보낼 확정된 턴 결과.
//!
//! 이 Tasty 는 부모의 수신처를 모른다. 턴을 확정하면 결과를 `attach-out/` 에 영속 기록으로 옮기고 서버 host 에
//! 맡긴다(`attach.child_notice`). client 의 같은 plugin 이 그 부모의 수신처 원장에 기록한다. 기록은 client 가
//! 기록을 확인한(`delivered`) 뒤나 받을 수 없다고 한(`rejected`) 뒤에만 지운다. `pending` 이나 호출 실패는
//! 같은 키로 늘어나는 간격([`super::backoff`])을 두고 다시 묻는다. 다시 묻는 계기는 턴 펌프(훅·예약·재시작)다.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tasty_plugin_agent_common::remote_attach::NoticeStatus;

use super::Ctx;
use super::ledger::Completion;
use crate::store;

/// client 가 받을 결과 본문의 키. 나머지 형식은 이 plugin 이 정하며 host 는 해석하지 않는다.
pub(crate) const PAYLOAD_KEY: &str = "completion";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Outgoing {
    key: String,
    surface: u32,
    completion: Value,
    attempts: u32,
    next_at_ms: u64,
}

fn dir(root: &Path) -> PathBuf {
    root.join("attach-out")
}

fn file(root: &Path, name: &str) -> PathBuf {
    dir(root).join(format!("{name}.json"))
}

/// 원격으로 보낼 완료 본문. client 는 [`super::parse_completion`] 으로 읽는다.
pub(crate) fn completion_json(c: &Completion) -> Value {
    json!({
        "completion_key": c.completion_key,
        "provider": c.provider,
        "child_surface": c.child_surface,
        "child_session": c.child_session,
        "status": c.status,
        "body": c.body,
        "attempt_id": c.attempt_id,
    })
}

/// 확정한 결과를 보낼 기록으로 남긴다. 같은 이름이 이미 있으면(같은 턴) 그대로 둔다.
/// `route` 는 client 가 그 턴을 지시할 때 준 토큰이며 결과에 그대로 싣는다.
pub(crate) fn queue(
    root: &Path,
    name: &str,
    completion: &Completion,
    route: &str,
) -> std::io::Result<()> {
    let path = file(root, name);
    if store::read_json::<Outgoing>(&path)?.is_some() {
        return Ok(());
    }
    let mut body = completion_json(completion);
    body[tasty_plugin_agent_common::remote_attach::ROUTE_FIELD] = json!(route);
    store::write_json(
        &path,
        &Outgoing {
            key: completion.completion_key.clone(),
            surface: completion.child_surface,
            completion: body,
            attempts: 0,
            next_at_ms: 0,
        },
    )
}

fn names(root: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir(root)) else {
        return Vec::new();
    };
    let mut out: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|e| {
            e.file_name()
                .to_str()
                .and_then(|n| n.strip_suffix(".json"))
                .filter(|n| n.len() == 64 && n.bytes().all(|b| b.is_ascii_hexdigit()))
                .map(str::to_string)
        })
        .collect();
    out.sort_unstable();
    out
}

/// 때가 된 기록을 맡기거나 상태를 묻는다. 남은 기록 중 가장 이른 다음 시각을 돌려준다.
pub(crate) fn pump(ctx: &Ctx<'_>) -> Option<u64> {
    let now = store::now_ms();
    let mut next: Option<u64> = None;
    for name in names(ctx.root) {
        let path = file(ctx.root, &name);
        let mut out = match store::read_json::<Outgoing>(&path) {
            Ok(Some(o)) => o,
            Ok(None) => continue,
            Err(e) => {
                tracing::warn!("codex attach result {name}: unreadable: {e}");
                continue;
            }
        };
        if out.next_at_ms > now {
            next = Some(next.map_or(out.next_at_ms, |t| t.min(out.next_at_ms)));
            continue;
        }
        let status = ctx.dispatch.child_notice(
            out.surface,
            &out.key,
            json!({ PAYLOAD_KEY: out.completion.clone() }),
        );
        let settled = match &status {
            Ok(NoticeStatus::Delivered) => true,
            Ok(NoticeStatus::Rejected(reason)) => {
                tracing::warn!(
                    "codex attach result {}: refused by the attach parent, dropped: {reason}",
                    out.key
                );
                true
            }
            Ok(NoticeStatus::Pending) => false,
            Err(e) => {
                tracing::warn!("codex attach result {}: not handed over: {e}", out.key);
                false
            }
        };
        if settled {
            if let Err(e) = store::remove_file(&path) {
                tracing::warn!("codex attach result {name}: remove: {e}");
            }
            continue;
        }
        out.attempts = out.attempts.saturating_add(1);
        out.next_at_ms = now + super::backoff(out.attempts).as_millis() as u64;
        next = Some(next.map_or(out.next_at_ms, |t| t.min(out.next_at_ms)));
        if let Err(e) = store::write_json(&path, &out) {
            tracing::warn!("codex attach result {name}: keep retry time: {e}");
        }
    }
    next
}

/// 아직 보내지 못한 기록 수.
#[cfg(test)]
pub(crate) fn pending(root: &Path) -> usize {
    names(root).len()
}

/// 모든 기록의 다음 시각을 지금으로 당긴다(시간 경과를 흉내 낸다).
#[cfg(test)]
pub(crate) fn make_due(root: &Path) {
    for name in names(root) {
        let path = file(root, &name);
        let mut out: Outgoing = store::read_json(&path).unwrap().unwrap();
        out.next_at_ms = 0;
        store::write_json(&path, &out).unwrap();
    }
}
