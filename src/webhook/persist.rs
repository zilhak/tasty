//! Persistent 웹훅을 데이터 루트의 webhooks.toml에 저장하고 복원한다.
//! Temporary는 제외하며 ID·메서드·핸들러·시퀀스·인증·남은 제한을 저장한다.
//! URL의 포트는 리스너 설정을 따르므로 재시작 후에도 반드시 같은 URL인 것은 아니다.
//!
//! 설정 파일의 webhook 배열만 바꾼다. 읽기·파싱에 실패하면 빈 테이블에서 시작하므로
//! 그 경우 다른 키는 보존되지 않는다. 쓰기 실패는 경고하며 호출자에게 성공을 보장하지 않는다.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::auth::WebhookAuth;
use super::lifetime::{Lifetime, Limit, Persistence, now_unix};
use super::registry::WebhookEntry;
use crate::hook_handler::{HookHandlerId, IpcCall};

/// 데이터 루트가 없으면 임시 디렉터리의 공유 경로를 사용한다.
pub(super) fn config_path() -> PathBuf {
    tasty_utils::path::tasty_home()
        .map(|d| d.join("webhooks.toml"))
        // 이유: 홈이 없을 때도 포트 설정과 영속 등록이 같은 사용자 설정 파일을 공유한다.
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
    /// 인증 설정. 없으면 인증 없이 통과한다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<WebhookAuth>,
}

/// 저장 대상은 모두 Persistent이므로 제한만 기록한다.
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

#[derive(Default, Deserialize)]
struct WebhooksFile {
    #[serde(default, rename = "webhook")]
    webhooks: Vec<PersistedWebhook>,
}

/// 파일을 읽지 못하면 빈 목록이다. 파싱 실패는 경고도 남긴다.
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

/// 읽어 온 설정에서 webhook 배열을 교체한다. 저장 실패는 경고로 남긴다.
pub(super) fn write(persistent: &[PersistedWebhook]) {
    let path = config_path();
    let Some(doc) = merge_webhook_section(&path, persistent) else {
        return;
    };
    render_and_write(&path, &doc);
}

/// 읽기·파싱 실패는 빈 테이블로 시작한다. 직렬화 실패는 경고 후 None이다.
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

/// TOML을 렌더해 저장한다. 실패를 경고하며 호출자에게 Result는 반환하지 않는다.
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

/// 시간·횟수가 만료된 항목을 제외하고 복원한다. 제외한 항목이 있으면 파일 정리도 시도한다.
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
        // 실제 port 설정과 무관한 추가 테이블도 보존되는지 검사한다.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("webhooks.toml");
        std::fs::write(&path, "[listener]\nport = 28429\n").unwrap();

        // 전역 설정 경로 대신 임시 파일로 병합·저장 동작을 구성한다.
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
