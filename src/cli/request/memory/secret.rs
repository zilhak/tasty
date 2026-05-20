use crate::cli::commands::MemorySecretCommands;

use super::{read_value_arg, require_scope, resolve_scope, ttl_to_expires_at};

pub(super) fn memory_secret_command_to_method_params(
    command: &MemorySecretCommands,
) -> (&'static str, serde_json::Value) {
    use MemorySecretCommands::*;
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
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&raw) {
                    params["value"] = parsed;
                } else {
                    params["value"] = serde_json::Value::String(raw);
                }
            } else {
                eprintln!("Error: 'memory secret put' requires --value or --value-b64");
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
            ("memory.secret.put", params)
        }
        Get { scope, surface, workspace, window, account, global, key } => {
            let scope_token = require_scope(scope.as_deref(), *surface, *workspace, *window, account.as_deref(), *global);
            (
                "memory.secret.get",
                serde_json::json!({ "scope": scope_token, "key": key }),
            )
        }
        Delete { scope, surface, workspace, window, account, global, key, cas } => {
            let scope_token = require_scope(scope.as_deref(), *surface, *workspace, *window, account.as_deref(), *global);
            let mut p = serde_json::json!({ "scope": scope_token, "key": key });
            if let Some(c) = cas {
                p["cas"] = serde_json::json!(c);
            }
            ("memory.secret.delete", p)
        }
        Exists { scope, surface, workspace, window, account, global, key } => {
            let scope_token = require_scope(scope.as_deref(), *surface, *workspace, *window, account.as_deref(), *global);
            (
                "memory.secret.exists",
                serde_json::json!({ "scope": scope_token, "key": key }),
            )
        }
        List { scope, surface, workspace, window, account, global, prefix, limit } => {
            let scope_token = require_scope(scope.as_deref(), *surface, *workspace, *window, account.as_deref(), *global);
            let mut p = serde_json::json!({ "scope": scope_token });
            if let Some(pre) = prefix {
                p["prefix"] = serde_json::json!(pre);
            }
            if let Some(l) = limit {
                p["limit"] = serde_json::json!(l);
            }
            ("memory.secret.list", p)
        }
        Count { scope, surface, workspace, window, account, global, prefix } => {
            let scope_token = require_scope(scope.as_deref(), *surface, *workspace, *window, account.as_deref(), *global);
            let mut p = serde_json::json!({ "scope": scope_token });
            if let Some(pre) = prefix {
                p["prefix"] = serde_json::json!(pre);
            }
            ("memory.secret.count", p)
        }
        Scopes => ("memory.secret.scopes", serde_json::json!({})),
        Stats { scope, surface, workspace, window, account, global } => {
            let mut p = serde_json::json!({});
            if let Some(tok) = resolve_scope(scope.as_deref(), *surface, *workspace, *window, account.as_deref(), *global) {
                p["scope"] = serde_json::json!(tok);
            }
            ("memory.secret.stats", p)
        }
    }
}

