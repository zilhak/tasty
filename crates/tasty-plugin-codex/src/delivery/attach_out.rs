//! attach 원격 출처 부모에게 돌려보낼 확정된 턴 결과.
//!
//! 이 Tasty 는 부모의 수신처를 모른다. 턴을 확정하면 결과를 `attach-out/` 에 영속 기록으로 옮기고 서버 host 에
//! 맡긴다(`attach.child_notice`). client 의 같은 plugin 이 그 부모의 수신처 원장에 기록한다. 기록은 client 가
//! 기록을 확인한(`delivered`) 뒤에만 지운다. `pending` 이나 호출 실패는 같은 키로 늘어나는 간격([`super::backoff`])을
//! 두고 다시 묻는다. 다시 묻는 계기는 턴 펌프(훅·예약·재시작)다.
//! 받는 쪽이 영구히 받을 수 없다고 했거나(`rejected`) 첫 실패부터 [`AUTO_RETRY_WINDOW`] 가 지나면 지우지 않고
//! 사유와 함께 포기한 기록(`gave_up`)으로 남기고 더 묻지 않는다.
//! 이 펌프는 worker 에서 돈다. 호출 하나는 [`tasty_plugin_agent_common::remote_attach::NOTICE_TIMEOUT`] 을 넘지 않고,
//! 호출이 실패하면 그 펌프에서는 남은 기록을 묻지 않는다.

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
    #[serde(default)]
    first_failed_ms: Option<u64>,
    /// 다시 묻지 않는다. 결과와 사유는 이 파일에 남는다.
    #[serde(default)]
    gave_up: bool,
    #[serde(default)]
    error: Option<String>,
}

/// 첫 실패부터 자동으로 다시 묻는 기간. claude 결과 송신과 같다.
pub(crate) const AUTO_RETRY_WINDOW: std::time::Duration =
    std::time::Duration::from_secs(24 * 60 * 60);

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
            first_failed_ms: None,
            gave_up: false,
            error: None,
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
    let mut stalled = false;
    for name in names(ctx.root) {
        // host 호출이 실패했으면 이번에는 더 묻지 않는다. worker 를 한 번에 오래 막지 않도록 남은 기록은 다음
        // 깨우기에 묻는다.
        if stalled {
            let at = now + super::backoff(1).as_millis() as u64;
            next = Some(next.map_or(at, |t| t.min(at)));
            break;
        }
        let path = file(ctx.root, &name);
        let mut out = match store::read_json::<Outgoing>(&path) {
            Ok(Some(o)) => o,
            Ok(None) => continue,
            Err(e) => {
                tracing::warn!("codex attach result {name}: unreadable: {e}");
                continue;
            }
        };
        if out.gave_up {
            continue;
        }
        if out.next_at_ms > now {
            next = Some(next.map_or(out.next_at_ms, |t| t.min(out.next_at_ms)));
            continue;
        }
        let status = ctx.dispatch.child_notice(
            out.surface,
            &out.key,
            json!({ PAYLOAD_KEY: out.completion.clone() }),
        );
        let failure = match &status {
            Ok(NoticeStatus::Delivered) => {
                if let Err(e) = store::remove_file(&path) {
                    tracing::warn!("codex attach result {name}: remove: {e}");
                }
                continue;
            }
            Ok(NoticeStatus::Rejected(reason)) => {
                out.gave_up = true;
                format!("refused by the attach parent: {reason}")
            }
            Ok(NoticeStatus::Pending) => "waiting for the attach parent to record it".to_string(),
            Err(e) => {
                stalled = true;
                e.clone()
            }
        };
        out.attempts = out.attempts.saturating_add(1);
        out.error = Some(failure.clone());
        let first = *out.first_failed_ms.get_or_insert(now);
        if now.saturating_sub(first) >= AUTO_RETRY_WINDOW.as_millis() as u64 {
            out.gave_up = true;
        }
        if out.gave_up {
            tracing::warn!(
                "codex attach result {}: gave up after {} attempts and kept it in attach-out: {failure}",
                out.key,
                out.attempts
            );
        } else {
            tracing::warn!(
                "codex attach result {}: not handed over yet: {failure}",
                out.key
            );
            out.next_at_ms = now + super::backoff(out.attempts).as_millis() as u64;
            next = Some(next.map_or(out.next_at_ms, |t| t.min(out.next_at_ms)));
        }
        if let Err(e) = store::write_json(&path, &out) {
            tracing::warn!("codex attach result {name}: keep retry time: {e}");
        }
    }
    next
}

/// 아직 보내지 못한 기록 수(포기한 기록 포함).
#[cfg(test)]
pub(crate) fn pending(root: &Path) -> usize {
    names(root).len()
}

/// 포기한 기록 수.
#[cfg(test)]
pub(crate) fn given_up(root: &Path) -> usize {
    names(root)
        .iter()
        .filter_map(|n| store::read_json::<Outgoing>(&file(root, n)).ok().flatten())
        .filter(|o| o.gave_up)
        .count()
}

/// 첫 실패 시각을 `ms` 만큼 앞당긴다(시간 경과를 흉내 낸다).
#[cfg(test)]
pub(crate) fn age_failures(root: &Path, ms: u64) {
    for name in names(root) {
        let path = file(root, &name);
        let mut out: Outgoing = store::read_json(&path).unwrap().unwrap();
        out.first_failed_ms = out.first_failed_ms.map(|t| t.saturating_sub(ms));
        out.next_at_ms = 0;
        store::write_json(&path, &out).unwrap();
    }
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
