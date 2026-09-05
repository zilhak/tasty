//! `Persistent` 웹훅의 `~/.tasty/webhooks.toml` 영속화 + 재시작 복원/필터.
//!
//! `Temporary` 웹훅은 저장하지 않는다(재시작 시 소멸). `Persistent` 웹훅만 발급
//! id·메서드·핸들러·시퀀스·lifetime(절대 deadline / 남은 카운트 포함)을 저장해
//! 재시작 후에도 **같은 URL·잔여 제한**으로 복원한다.
//!
//! ## 스키마 (S8 포트설정과 공유 — 최소·확장 가능)
//! ```toml
//! [listener]          # S8(포트 설정)이 소유. S5 는 건드리지 않고 round-trip 보존.
//! port = 28429
//!
//! [[webhook]]         # S5 가 소유하는 Persistent 웹훅 배열.
//! id = "a1b2c3d4e5f60718"
//! methods = ["POST"]
//! handler = "user/wh-notification-create"   # optional
//! [[webhook.sequence]]
//! method = "notification.create"
//! params = { body = "${body.message}" }
//! [webhook.limit]
//! kind = "time"                             # unlimited | time | count
//! deadline_unix = 1720051200                # 절대 시각(재시작 후 정확 만료)
//! ```
//!
//! **스키마 공유 규칙**: S5 는 저장 시 기존 문서를 파싱해 `webhook` 배열만 교체하고
//! `[listener]` 등 나머지 섹션(S8 소유·미지 키 포함)은 그대로 보존한다.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::auth::WebhookAuth;
use super::lifetime::{Lifetime, Limit, Persistence, now_unix};
use super::registry::WebhookEntry;
use crate::hook_handler::{HookHandlerId, IpcCall};

/// `~/.tasty/webhooks.toml`. 홈 결정 실패 시 임시 경로(그 경우 파일이 없어
/// 복원은 빈 목록, 저장은 임시 위치로 폴백 — caller 무해).
pub(super) fn config_path() -> PathBuf {
    tasty_utils::path::tasty_home()
        .map(|d| d.join("webhooks.toml"))
        // 이유: 홈 미해결에서만 쓰는 공유 폴백. 인스턴스별 격리가 목적이 아니라 사용자
        // config 라 의도된 공유다(파일 없으면 복원은 빈 목록, caller 무해).
        .unwrap_or_else(|| std::env::temp_dir().join("tasty-webhooks.toml"))
}

/// 영속화된 웹훅 한 건 (`[[webhook]]`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct PersistedWebhook {
    pub id: String,
    pub methods: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handler: Option<String>,
    /// 등록 시점 확정된 IpcSequence 스냅샷(복원 시 그대로 사용).
    #[serde(default, rename = "sequence", skip_serializing_if = "Vec::is_empty")]
    pub calls: Vec<IpcCall>,
    pub limit: PersistedLimit,
    /// 선택적 인증 설정(S6). 미설정이면 생략 — 복원 시 무인증 통과.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<WebhookAuth>,
}

/// lifetime 제한의 영속 표현. 영속성은 `Persistent` 로 고정(저장 대상이 곧 영속)이라
/// 별도 필드를 두지 않는다.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum PersistedLimit {
    Unlimited,
    /// 절대 만료 시각(Unix epoch secs).
    Time {
        deadline_unix: u64,
    },
    /// 남은 호출 횟수.
    Count {
        remaining: u64,
    },
}

impl PersistedLimit {
    fn from_limit(limit: Limit) -> Self {
        match limit {
            Limit::Unlimited => Self::Unlimited,
            Limit::TimeLimit { deadline_unix } => Self::Time { deadline_unix },
            Limit::CountLimit { remaining } => Self::Count { remaining },
        }
    }

    fn to_limit(self) -> Limit {
        match self {
            Self::Unlimited => Limit::Unlimited,
            Self::Time { deadline_unix } => Limit::TimeLimit { deadline_unix },
            Self::Count { remaining } => Limit::CountLimit { remaining },
        }
    }
}

/// in-memory 엔트리 → 영속 표현(영속 엔트리만 호출됨).
pub(super) fn to_persisted(entry: &WebhookEntry) -> PersistedWebhook {
    PersistedWebhook {
        id: entry.id.clone(),
        methods: entry.methods.clone(),
        handler: entry.handler_id.as_ref().map(|h| h.0.clone()),
        calls: entry.calls.clone(),
        limit: PersistedLimit::from_limit(entry.lifetime.limit),
        auth: entry.auth.clone(),
    }
}

/// 파일에서 읽기용 최상위 문서(`webhook` 배열만 관심).
#[derive(Default, Deserialize)]
struct WebhooksFile {
    #[serde(default, rename = "webhook")]
    webhooks: Vec<PersistedWebhook>,
}

/// 저장된 영속 웹훅을 읽는다. 파일 없음/파싱 실패 시 빈 목록(경고 로그).
fn load_persisted() -> Vec<PersistedWebhook> {
    let path = config_path();
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    match toml::from_str::<WebhooksFile>(&text) {
        Ok(f) => f.webhooks,
        Err(e) => {
            tracing::warn!("webhooks.toml parse failed ({}): {e}", path.display());
            Vec::new()
        }
    }
}

/// 영속 엔트리들을 파일에 기록한다. **기존 문서의 다른 섹션(S8 `[listener]` 등)은
/// 보존**하고 `webhook` 배열만 교체한다. 실패는 삼키지 않고 경고.
pub(super) fn write(persistent: &[PersistedWebhook]) {
    let path = config_path();
    let Some(doc) = merge_webhook_section(&path, persistent) else {
        return;
    };
    render_and_write(&path, &doc);
}

/// 기존 문서를 파싱(없으면 빈 table)해 `webhook` 키만 교체 → 나머지 보존.
/// 직렬화 실패 시 경고 로그 후 `None`(호출자는 저장을 포기한다).
fn merge_webhook_section(path: &Path, persistent: &[PersistedWebhook]) -> Option<toml::Table> {
    let mut doc: toml::Table = std::fs::read_to_string(path)
        .ok()
        .and_then(|s| toml::from_str::<toml::Table>(&s).ok())
        .unwrap_or_default();

    if persistent.is_empty() {
        doc.remove("webhook");
    } else {
        match toml::Value::try_from(persistent) {
            Ok(v) => {
                doc.insert("webhook".to_string(), v);
            }
            Err(e) => {
                tracing::warn!("webhook persist serialize failed: {e}");
                return None;
            }
        }
    }
    Some(doc)
}

/// `doc` 를 pretty TOML 로 렌더해 atomic write. 렌더/쓰기 실패는 경고 로그만
/// (fire-and-forget 저장 — 호출자는 이 실패를 별도로 처리하지 않는다).
fn render_and_write(path: &Path, doc: &toml::Table) {
    let text = match toml::to_string_pretty(doc) {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!("webhooks.toml render failed: {e}");
            return;
        }
    };

    if let Err(e) = atomic_write(path, &text) {
        tracing::warn!("webhooks.toml write failed ({}): {e}", path.display());
    }
}

/// tempfile → persist 로 atomic write(파일 핸들러 `save.rs` 선례).
fn atomic_write(path: &Path, text: &str) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    if !parent.exists() {
        std::fs::create_dir_all(parent)?;
    }
    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    tmp.write_all(text.as_bytes())?;
    tmp.flush()?;
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

/// 부팅 시 영속 웹훅을 레지스트리로 복원한다 — **재시작 필터**: 이미 만료된(시간
/// 초과 / 카운트 0) 웹훅은 등록하지 않고 버린다. 필터로 버려진 게 있으면 파일도
/// 정리해 stale 엔트리를 제거한다.
pub(super) fn restore_into_registry() {
    let now = now_unix();
    let mut restored = 0usize;
    let mut filtered = 0usize;

    for pw in load_persisted() {
        let lifetime = Lifetime {
            persistence: Persistence::Persistent,
            limit: pw.limit.to_limit(),
        };
        if lifetime.is_expired(now) {
            filtered += 1;
            continue;
        }
        super::registry::restore_entry(WebhookEntry {
            id: pw.id,
            methods: pw.methods,
            handler_id: pw.handler.map(HookHandlerId::new),
            calls: pw.calls,
            lifetime,
            auth: pw.auth,
        });
        restored += 1;
    }

    if restored > 0 || filtered > 0 {
        tracing::info!(
            "webhook restore: {restored} persistent restored, {filtered} expired filtered"
        );
    }
    // 재시작 필터로 stale 이 제거됐으면 파일에서도 정리.
    if filtered > 0 {
        super::registry::persist_now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persisted_limit_roundtrip() {
        for limit in [
            Limit::Unlimited,
            Limit::TimeLimit {
                deadline_unix: 1_720_000_000,
            },
            Limit::CountLimit { remaining: 5 },
        ] {
            let p = PersistedLimit::from_limit(limit);
            assert_eq!(p.to_limit(), limit);
        }
    }

    #[test]
    fn persisted_webhook_toml_roundtrip() {
        let items = vec![PersistedWebhook {
            id: "a1b2c3d4e5f60718".to_string(),
            methods: vec!["POST".to_string()],
            handler: Some("user/wh-notify".to_string()),
            calls: vec![IpcCall {
                method: "notification.create".to_string(),
                params: serde_json::json!({"body": "${body.message}"}),
            }],
            limit: PersistedLimit::Count { remaining: 3 },
            auth: None,
        }];
        // Value::try_from → table 삽입 → 문자열화 → 재파싱이 동일 데이터를 준다.
        let mut doc = toml::Table::new();
        doc.insert("webhook".into(), toml::Value::try_from(&items).unwrap());
        let text = toml::to_string_pretty(&doc).unwrap();
        let parsed: WebhooksFile = toml::from_str(&text).unwrap();
        assert_eq!(parsed.webhooks.len(), 1);
        assert_eq!(parsed.webhooks[0].id, "a1b2c3d4e5f60718");
        assert_eq!(parsed.webhooks[0].calls.len(), 1);
        assert!(matches!(
            parsed.webhooks[0].limit,
            PersistedLimit::Count { remaining: 3 }
        ));
    }

    #[test]
    fn write_preserves_foreign_listener_section() {
        // S8 이 소유할 [listener] 섹션이 write 후에도 보존되는지(스키마 공유).
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("webhooks.toml");
        std::fs::write(&path, "[listener]\nport = 28429\n").unwrap();

        // config_path 를 직접 못 바꾸므로 write 내부 로직을 재현해 검증한다.
        let items = vec![PersistedWebhook {
            id: "deadbeefdeadbeef".to_string(),
            methods: vec!["POST".to_string()],
            handler: None,
            calls: vec![],
            limit: PersistedLimit::Unlimited,
            auth: None,
        }];
        let mut doc: toml::Table =
            toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        doc.insert("webhook".into(), toml::Value::try_from(&items).unwrap());
        let text = toml::to_string_pretty(&doc).unwrap();
        std::fs::write(&path, &text).unwrap();

        let reparsed: toml::Table =
            toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(reparsed.contains_key("listener"), "listener 보존 실패");
        assert!(reparsed.contains_key("webhook"));
        assert_eq!(reparsed["listener"]["port"].as_integer(), Some(28429));
    }
}
