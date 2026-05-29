//! `tasty memory ...` CLI → JsonRpcRequest 매핑 + 공용 scope/value/TTL 헬퍼.

use crate::cli::commands::MemoryCommands;

pub(super) fn resolve_scope(
    scope: Option<&str>,
    surface: Option<u32>,
    workspace: Option<u32>,
    window: Option<u64>,
    account: Option<&str>,
    global: bool,
) -> Option<String> {
    if let Some(s) = scope {
        return Some(s.to_string());
    }
    if let Some(id) = surface {
        return Some(format!("surface:{id}"));
    }
    if let Some(id) = workspace {
        return Some(format!("workspace:{id}"));
    }
    if let Some(id) = window {
        return Some(format!("window:{id}"));
    }
    if let Some(u) = account {
        return Some(format!("account:{u}"));
    }
    if global {
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

pub(super) fn memory_command_to_method_params(
    command: &MemoryCommands,
) -> (&'static str, serde_json::Value) {
    use MemoryCommands::*;
    match command {
        Put {
            scope,
            surface,
            workspace,
            window,
            account,
            global,
            key,
            value,
            value_b64,
            content_type,
            ttl,
            expires_at,
            cas,
        } => {
            let scope_token = require_scope(
                scope.as_deref(),
                *surface,
                *workspace,
                *window,
                account.as_deref(),
                *global,
            );
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
                        eprintln!("Error: failed to read value file: {e}");
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
                eprintln!("Error: 'memory put' requires --value or --value-b64");
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
        Get {
            scope,
            surface,
            workspace,
            window,
            account,
            global,
            key,
        } => {
            let scope_token = require_scope(
                scope.as_deref(),
                *surface,
                *workspace,
                *window,
                account.as_deref(),
                *global,
            );
            (
                "memory.get",
                serde_json::json!({ "scope": scope_token, "key": key }),
            )
        }
        Delete {
            scope,
            surface,
            workspace,
            window,
            account,
            global,
            key,
            cas,
        } => {
            let scope_token = require_scope(
                scope.as_deref(),
                *surface,
                *workspace,
                *window,
                account.as_deref(),
                *global,
            );
            let mut p = serde_json::json!({ "scope": scope_token, "key": key });
            if let Some(c) = cas {
                p["cas"] = serde_json::json!(c);
            }
            ("memory.delete", p)
        }
        Exists {
            scope,
            surface,
            workspace,
            window,
            account,
            global,
            key,
        } => {
            let scope_token = require_scope(
                scope.as_deref(),
                *surface,
                *workspace,
                *window,
                account.as_deref(),
                *global,
            );
            (
                "memory.exists",
                serde_json::json!({ "scope": scope_token, "key": key }),
            )
        }
        List {
            scope,
            surface,
            workspace,
            window,
            account,
            global,
            prefix,
            limit,
            since,
            until,
            offset,
        } => {
            let scope_token = require_scope(
                scope.as_deref(),
                *surface,
                *workspace,
                *window,
                account.as_deref(),
                *global,
            );
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
            surface,
            workspace,
            window,
            account,
            global,
            path,
            equals,
            prefix,
            limit,
            since,
            until,
            offset,
        } => {
            let scope_token = require_scope(
                scope.as_deref(),
                *surface,
                *workspace,
                *window,
                account.as_deref(),
                *global,
            );
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
        Export {
            scope,
            surface,
            workspace,
            window,
            account,
            global,
        } => {
            let mut p = serde_json::json!({});
            if let Some(tok) = resolve_scope(
                scope.as_deref(),
                *surface,
                *workspace,
                *window,
                account.as_deref(),
                *global,
            ) {
                p["scope"] = serde_json::json!(tok);
            }
            ("memory.export", p)
        }
        Import { file, replace } => {
            let raw = match std::fs::read_to_string(file) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Error: failed to read {file}: {e}");
                    std::process::exit(1);
                }
            };
            let parsed: serde_json::Value = match serde_json::from_str(&raw) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("Error: {file}: not valid JSON: {e}");
                    std::process::exit(1);
                }
            };
            // 입력은 배열이거나 `{ "entries": [...] }` 형태 둘 다 허용.
            let entries = if parsed.is_array() {
                parsed
            } else if let Some(arr) = parsed.get("entries") {
                arr.clone()
            } else {
                eprintln!("Error: {file}: expected JSON array or object with 'entries'");
                std::process::exit(1);
            };
            (
                "memory.import",
                serde_json::json!({ "entries": entries, "replace": replace }),
            )
        }
        Count {
            scope,
            surface,
            workspace,
            window,
            account,
            global,
            prefix,
        } => {
            let scope_token = require_scope(
                scope.as_deref(),
                *surface,
                *workspace,
                *window,
                account.as_deref(),
                *global,
            );
            let mut p = serde_json::json!({ "scope": scope_token });
            if let Some(pre) = prefix {
                p["prefix"] = serde_json::json!(pre);
            }
            ("memory.count", p)
        }
        Scopes => ("memory.scopes", serde_json::json!({})),
        Stats {
            scope,
            surface,
            workspace,
            window,
            account,
            global,
        } => {
            let mut p = serde_json::json!({});
            if let Some(tok) = resolve_scope(
                scope.as_deref(),
                *surface,
                *workspace,
                *window,
                account.as_deref(),
                *global,
            ) {
                p["scope"] = serde_json::json!(tok);
            }
            ("memory.stats", p)
        }
        Gc => ("memory.gc", serde_json::json!({})),
        Secret { command } => memory_secret_command_to_method_params(command),
        Bb { command } => memory_bb_command_to_method_params(command),
        Plan { command } => memory_plan_command_to_method_params(command),
        Cache { command } => memory_cache_command_to_method_params(command),
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

pub(super) fn require_scope(
    scope: Option<&str>,
    surface: Option<u32>,
    workspace: Option<u32>,
    window: Option<u64>,
    account: Option<&str>,
    global: bool,
) -> String {
    match resolve_scope(scope, surface, workspace, window, account, global) {
        Some(s) => s,
        None => {
            eprintln!(
                "Error: must specify a scope. Use --scope <token> or one of \
                 --global / --surface <id> / --workspace <id> / --window <id> / --account <userid>."
            );
            std::process::exit(1);
        }
    }
}

mod bb;
mod cache;
mod plan;
mod secret;

use bb::memory_bb_command_to_method_params;
use cache::memory_cache_command_to_method_params;
use plan::memory_plan_command_to_method_params;
use secret::memory_secret_command_to_method_params;
