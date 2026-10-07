//! Persistent 웹훅을 데이터 루트의 webhooks.toml에 저장하고 복원한다.
//! Temporary는 제외하며 ID·메서드·핸들러·시퀀스·인증·남은 제한을 저장한다.
//! URL의 포트는 리스너 설정을 따르므로 재시작 후에도 반드시 같은 URL인 것은 아니다.
//!
//! 설정 파일의 webhook 배열만 바꾼다. 파일을 읽거나 파싱하지 못하면 쓰지 않고 오류를 돌려준다.
//! 그 테이블로 다시 쓰면 포트 설정과 다른 등록이 모두 사라지기 때문이다.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::auth::WebhookAuth;
use super::lifetime::{Lifetime, Limit, Persistence, now_unix};
use super::registry::WebhookEntry;
use crate::hook_handler::{HookHandlerId, IpcCall};

/// 포트 설정과 같은 파일이어야 하므로 경로 규칙은 `webhook_port_file::path` 하나를 쓴다.
pub(super) fn config_path() -> PathBuf {
    tasty_settings::webhook_port_file::path()
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

/// 읽어 온 설정에서 webhook 배열을 교체한다. 읽기·파싱·저장 실패는 쓰지 않고 오류다.
pub(super) fn write(persistent: &[PersistedWebhook]) -> Result<(), String> {
    write_to(&config_path(), persistent)
}

fn write_to(path: &Path, persistent: &[PersistedWebhook]) -> Result<(), String> {
    let doc = merge_webhook_section(path, persistent)?;
    let text =
        toml::to_string_pretty(&doc).map_err(|e| format!("webhooks.toml render failed: {e}"))?;
    atomic_write(path, &text)
        .map_err(|e| format!("webhooks.toml write failed ({}): {e}", path.display()))
}

/// 파일이 없으면 빈 테이블에서 시작한다. 읽기·파싱 실패는 포트 설정과 같은 규칙으로 오류다.
fn merge_webhook_section(
    path: &Path,
    persistent: &[PersistedWebhook],
) -> Result<toml::Table, String> {
    let mut doc = tasty_settings::webhook_port_file::read_table(path).map_err(|e| e.to_string())?;
    if persistent.is_empty() {
        doc.remove("webhook");
    } else {
        let value = toml::Value::try_from(persistent)
            .map_err(|e| format!("webhook persist serialize failed: {e}"))?;
        doc.insert("webhook".to_string(), value);
    }
    Ok(doc)
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

/// 시간·횟수가 만료된 항목을 제외하고 복원한 수를 돌려준다. 제외한 항목이 있으면 파일 정리도 시도한다.
pub(super) fn restore_into_registry() -> usize {
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
    restored
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
    fn a_file_that_does_not_parse_is_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("webhooks.toml");
        let broken = "port = 40123\n[[webhook]]\nid = \"keepme\"\nthis is not toml\n";
        std::fs::write(&path, broken).unwrap();
        let items = vec![PersistedWebhook {
            id: "deadbeefdeadbeef".to_string(),
            methods: vec!["POST".to_string()],
            handler: None,
            calls: vec![],
            limit: PersistedLimit::Unlimited,
            auth: None,
        }];
        assert!(write_to(&path, &items).is_err());
        assert!(write_to(&path, &[]).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), broken);
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
