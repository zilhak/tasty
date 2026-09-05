//! `tasty memory ...` CLI → JsonRpcRequest 매핑 + 공용 scope/value/TTL 헬퍼.

use crate::commands::{MemoryCommands, memory::ScopeArgs};

/// scope 선택자 → scope 토큰. 아무것도 안 주면 `None`.
///
/// 인자가 [`ScopeArgs`] 한 덩어리인 것이 요점이다 — 여섯 값을 따로 받으면 호출부마다
/// 여섯 줄이 되고, 그 여섯 줄이 자리 수만큼(16) 복제된 것이 이 모듈의 원래 모습이었다.
pub(super) fn resolve_scope(a: &ScopeArgs) -> Option<String> {
    if let Some(s) = a.scope.as_deref() {
        return Some(s.to_string());
    }
    if let Some(id) = a.surface {
        return Some(format!("surface:{id}"));
    }
    if let Some(id) = a.workspace {
        return Some(format!("workspace:{id}"));
    }
    if let Some(id) = a.window {
        return Some(format!("window:{id}"));
    }
    if let Some(u) = a.account.as_deref() {
        return Some(format!("account:{u}"));
    }
    if a.global {
        return Some("global".to_string());
    }
    None
}

/// `@path` 접두를 가진 value는 파일에서 UTF-8 텍스트로 읽어온다. 이외에는 그대로.
pub(super) fn read_value_arg(value: &str) -> std::io::Result<String> {
    if let Some(path) = value.strip_prefix('@') {
        std::fs::read_to_string(path)
    } else {
        Ok(value.to_string())
    }
}

#[allow(clippy::cognitive_complexity)] // complexity-exempt: CLI enum→(method,params) 평면 match 매핑 — arm 나열, 중첩 없음
pub(super) fn memory_command_to_method_params(
    command: &MemoryCommands,
) -> (&'static str, serde_json::Value) {
    use MemoryCommands::*;
    match command {
        Put {
            scope,
            key,
            value,
            value_b64,
            content_type,
            ttl,
            expires_at,
            cas,
        } => {
            let scope_token = require_scope(scope);
            let mut params = serde_json::json!({
                "scope": scope_token,
                "key": key,
            });
            if let Some(b64) = value_b64.as_deref() {
                params["value_b64"] = serde_json::json!(b64);
            } else if let Some(v) = value.as_deref() {
                let raw = match read_value_arg(v) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!(
                            "{}",
                            tasty_i18n::t_fmt("cli.memory.value_file_read_failed", &e.to_string())
                        );
                        std::process::exit(1);
                    }
                };
                // JSON으로 파싱되면 JSON value, 아니면 string으로 보존.
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&raw) {
                    params["value"] = parsed;
                } else {
                    params["value"] = serde_json::Value::String(raw);
                }
            } else {
                eprintln!(
                    "{}",
                    tasty_i18n::t_fmt("cli.memory.put_requires_value", "memory put")
                );
                std::process::exit(1);
            }
            if let Some(ct) = content_type.as_deref() {
                params["content_type"] = serde_json::json!(ct);
            }
            if let Some(t) = expires_at {
                params["expires_at"] = serde_json::json!(t);
            } else if let Some(secs) = ttl {
                params["expires_at"] = serde_json::json!(ttl_to_expires_at(*secs));
            }
            if let Some(v) = cas {
                params["cas"] = serde_json::json!(v);
            }
            ("memory.put", params)
        }
        Get { scope, key } => {
            let scope_token = require_scope(scope);
            (
                "memory.get",
                serde_json::json!({ "scope": scope_token, "key": key }),
            )
        }
        Delete { scope, key, cas } => {
            let scope_token = require_scope(scope);
            let mut p = serde_json::json!({ "scope": scope_token, "key": key });
            if let Some(c) = cas {
                p["cas"] = serde_json::json!(c);
            }
            ("memory.delete", p)
        }
        Exists { scope, key } => {
            let scope_token = require_scope(scope);
            (
                "memory.exists",
                serde_json::json!({ "scope": scope_token, "key": key }),
            )
        }
        List {
            scope,
            prefix,
            limit,
            since,
            until,
            offset,
        } => {
            let scope_token = require_scope(scope);
            let mut p = serde_json::json!({ "scope": scope_token });
            if let Some(pre) = prefix {
                p["prefix"] = serde_json::json!(pre);
            }
            if let Some(l) = limit {
                p["limit"] = serde_json::json!(l);
            }
            if let Some(s) = since {
                p["since"] = serde_json::json!(s);
            }
            if let Some(u) = until {
                p["until"] = serde_json::json!(u);
            }
            if let Some(o) = offset {
                p["offset"] = serde_json::json!(o);
            }
            ("memory.list", p)
        }
        Query {
            scope,
            path,
            equals,
            prefix,
            limit,
            since,
            until,
            offset,
        } => {
            let scope_token = require_scope(scope);
            // `--equals` 는 JSON 리터럴로 파싱; 실패하면 문자열 그대로.
            let equals_val: serde_json::Value = match serde_json::from_str(equals) {
                Ok(v) => v,
                Err(_) => serde_json::Value::String(equals.clone()),
            };
            let mut p = serde_json::json!({
                "scope": scope_token,
                "path": path,
                "equals": equals_val,
            });
            if let Some(pre) = prefix {
                p["prefix"] = serde_json::json!(pre);
            }
            if let Some(l) = limit {
                p["limit"] = serde_json::json!(l);
            }
            if let Some(s) = since {
                p["since"] = serde_json::json!(s);
            }
            if let Some(u) = until {
                p["until"] = serde_json::json!(u);
            }
            if let Some(o) = offset {
                p["offset"] = serde_json::json!(o);
            }
            ("memory.query", p)
        }
        Export { scope } => {
            let mut p = serde_json::json!({});
            if let Some(tok) = resolve_scope(scope) {
                p["scope"] = serde_json::json!(tok);
            }
            ("memory.export", p)
        }
        Import { file, replace } => {
            let raw = match std::fs::read_to_string(file) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!(
                        "{}",
                        tasty_i18n::t_fmt2("cli.memory.import_read_failed", file, &e.to_string())
                    );
                    std::process::exit(1);
                }
            };
            let parsed: serde_json::Value = match serde_json::from_str(&raw) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!(
                        "{}",
                        tasty_i18n::t_fmt2("cli.memory.import_not_json", file, &e.to_string())
                    );
                    std::process::exit(1);
                }
            };
            // 입력은 배열이거나 `{ "entries": [...] }` 형태 둘 다 허용.
            let entries = if parsed.is_array() {
                parsed
            } else if let Some(arr) = parsed.get("entries") {
                arr.clone()
            } else {
                eprintln!("{}", tasty_i18n::t_fmt("cli.memory.import_bad_shape", file));
                std::process::exit(1);
            };
            (
                "memory.import",
                serde_json::json!({ "entries": entries, "replace": replace }),
            )
        }
        Count { scope, prefix } => {
            let scope_token = require_scope(scope);
            let mut p = serde_json::json!({ "scope": scope_token });
            if let Some(pre) = prefix {
                p["prefix"] = serde_json::json!(pre);
            }
            ("memory.count", p)
        }
        Scopes => ("memory.scopes", serde_json::json!({})),
        Stats { scope } => {
            let mut p = serde_json::json!({});
            if let Some(tok) = resolve_scope(scope) {
                p["scope"] = serde_json::json!(tok);
            }
            ("memory.stats", p)
        }
        Gc => ("memory.gc", serde_json::json!({})),
        Secret { command } => memory_secret_command_to_method_params(command),
        Bb { command } => memory_bb_command_to_method_params(command),
        Plan { command } => memory_plan_command_to_method_params(command),
        Cache { command } => memory_cache_command_to_method_params(command),
        Goal { command } => memory_goal_command_to_method_params(command),
    }
}

pub(super) fn ttl_to_expires_at(secs: u64) -> i64 {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let add_ms = secs.saturating_mul(1000).min(i64::MAX as u64) as i64;
    now_ms.saturating_add(add_ms)
}

/// [`resolve_scope`] 와 같되, scope 가 없으면 메시지를 내고 종료한다.
pub(super) fn require_scope(a: &ScopeArgs) -> String {
    match resolve_scope(a) {
        Some(s) => s,
        None => {
            eprintln!("{}", tasty_i18n::t("cli.memory.scope_required"));
            std::process::exit(1);
        }
    }
}

mod bb;
mod cache;
mod goal;
mod plan;
mod secret;

use bb::memory_bb_command_to_method_params;
use cache::memory_cache_command_to_method_params;
use goal::memory_goal_command_to_method_params;
use plan::memory_plan_command_to_method_params;
use secret::memory_secret_command_to_method_params;
