//! `tasty memory ...` CLI → JsonRpcRequest 매핑 + 공용 scope/value/TTL 헬퍼.

use crate::cli::commands::{
    MemoryBbCommands, MemoryCacheCommands, MemoryCommands, MemoryPlanCommands,
    MemorySecretCommands,
};

fn resolve_scope(
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
fn read_value_arg(value: &str) -> std::io::Result<String> {
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
            let scope_token = require_scope(scope.as_deref(), *surface, *workspace, *window, account.as_deref(), *global);
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
        Get { scope, surface, workspace, window, account, global, key } => {
            let scope_token = require_scope(scope.as_deref(), *surface, *workspace, *window, account.as_deref(), *global);
            (
                "memory.get",
                serde_json::json!({ "scope": scope_token, "key": key }),
            )
        }
        Delete { scope, surface, workspace, window, account, global, key, cas } => {
            let scope_token = require_scope(scope.as_deref(), *surface, *workspace, *window, account.as_deref(), *global);
            let mut p = serde_json::json!({ "scope": scope_token, "key": key });
            if let Some(c) = cas {
                p["cas"] = serde_json::json!(c);
            }
            ("memory.delete", p)
        }
        Exists { scope, surface, workspace, window, account, global, key } => {
            let scope_token = require_scope(scope.as_deref(), *surface, *workspace, *window, account.as_deref(), *global);
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
            let scope_token = require_scope(scope.as_deref(), *surface, *workspace, *window, account.as_deref(), *global);
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
            let scope_token = require_scope(scope.as_deref(), *surface, *workspace, *window, account.as_deref(), *global);
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
        Export { scope, surface, workspace, window, account, global } => {
            let mut p = serde_json::json!({});
            if let Some(tok) = resolve_scope(scope.as_deref(), *surface, *workspace, *window, account.as_deref(), *global) {
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
        Count { scope, surface, workspace, window, account, global, prefix } => {
            let scope_token = require_scope(scope.as_deref(), *surface, *workspace, *window, account.as_deref(), *global);
            let mut p = serde_json::json!({ "scope": scope_token });
            if let Some(pre) = prefix {
                p["prefix"] = serde_json::json!(pre);
            }
            ("memory.count", p)
        }
        Scopes => ("memory.scopes", serde_json::json!({})),
        Stats { scope, surface, workspace, window, account, global } => {
            let mut p = serde_json::json!({});
            if let Some(tok) = resolve_scope(scope.as_deref(), *surface, *workspace, *window, account.as_deref(), *global) {
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

fn memory_cache_command_to_method_params(
    command: &MemoryCacheCommands,
) -> (&'static str, serde_json::Value) {
    use MemoryCacheCommands::*;
    match command {
        Put { workspace, key, value, value_b64, content_type, ttl } => {
            let mut p = serde_json::json!({
                "workspace_id": workspace,
                "key": key,
                "ttl_secs": ttl,
            });
            if let Some(b64) = value_b64.as_deref() {
                p["value_b64"] = serde_json::json!(b64);
            } else if let Some(v) = value.as_deref() {
                let raw = match read_value_arg(v) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("Error: failed to read value file: {e}");
                        std::process::exit(1);
                    }
                };
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&raw) {
                    p["value"] = parsed;
                } else {
                    p["value"] = serde_json::Value::String(raw);
                }
            } else {
                eprintln!("Error: 'memory cache put' requires --value or --value-b64");
                std::process::exit(1);
            }
            if let Some(ct) = content_type.as_deref() {
                p["content_type"] = serde_json::json!(ct);
            }
            ("memory.cache_put", p)
        }
        Get { workspace, key } => (
            "memory.cache_get",
            serde_json::json!({ "workspace_id": workspace, "key": key }),
        ),
        Invalidate { workspace, key } => (
            "memory.cache_invalidate",
            serde_json::json!({ "workspace_id": workspace, "key": key }),
        ),
        Clear { workspace } => (
            "memory.cache_clear",
            serde_json::json!({ "workspace_id": workspace }),
        ),
        List { workspace } => (
            "memory.cache_list",
            serde_json::json!({ "workspace_id": workspace }),
        ),
    }
}

fn memory_plan_command_to_method_params(
    command: &MemoryPlanCommands,
) -> (&'static str, serde_json::Value) {
    use MemoryPlanCommands::*;
    match command {
        Create { workspace, plan_id, title, steps } => {
            let mut p = serde_json::json!({
                "workspace_id": workspace,
                "plan_id": plan_id,
                "title": title,
            });
            if let Some(raw) = steps.as_deref() {
                let arr: serde_json::Value = match serde_json::from_str(raw) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("Error: --steps is not valid JSON: {e}");
                        std::process::exit(1);
                    }
                };
                p["steps"] = arr;
            }
            ("memory.plan_create", p)
        }
        Get { workspace, plan_id } => (
            "memory.plan_get",
            serde_json::json!({ "workspace_id": workspace, "plan_id": plan_id }),
        ),
        List { workspace } => (
            "memory.plan_list",
            serde_json::json!({ "workspace_id": workspace }),
        ),
        Delete { workspace, plan_id } => (
            "memory.plan_delete",
            serde_json::json!({ "workspace_id": workspace, "plan_id": plan_id }),
        ),
        AddStep { workspace, plan_id, step, position, cas } => {
            let step_v: serde_json::Value = match serde_json::from_str(step) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("Error: --step is not valid JSON: {e}");
                    std::process::exit(1);
                }
            };
            let mut p = serde_json::json!({
                "workspace_id": workspace,
                "plan_id": plan_id,
                "step": step_v,
            });
            if let Some(pos) = position {
                p["position"] = serde_json::json!(pos);
            }
            if let Some(c) = cas {
                p["cas"] = serde_json::json!(c);
            }
            ("memory.plan_add_step", p)
        }
        RemoveStep { workspace, plan_id, step_id, cas } => {
            let mut p = serde_json::json!({
                "workspace_id": workspace,
                "plan_id": plan_id,
                "step_id": step_id,
            });
            if let Some(c) = cas {
                p["cas"] = serde_json::json!(c);
            }
            ("memory.plan_remove_step", p)
        }
        UpdateStep { workspace, plan_id, step_id, state, notes, clear_notes, cas } => {
            let mut p = serde_json::json!({
                "workspace_id": workspace,
                "plan_id": plan_id,
                "step_id": step_id,
            });
            if let Some(s) = state.as_deref() {
                p["state"] = serde_json::json!(s);
            }
            if *clear_notes {
                p["clear_notes"] = serde_json::json!(true);
            } else if let Some(n) = notes.as_deref() {
                p["notes"] = serde_json::json!(n);
            }
            if let Some(c) = cas {
                p["cas"] = serde_json::json!(c);
            }
            ("memory.plan_update_step", p)
        }
    }
}

fn memory_bb_command_to_method_params(
    command: &MemoryBbCommands,
) -> (&'static str, serde_json::Value) {
    use MemoryBbCommands::*;
    match command {
        Create { workspace, name, schema } => {
            let mut p = serde_json::json!({ "workspace_id": workspace, "name": name });
            if let Some(raw) = schema.as_deref() {
                let v: serde_json::Value = match serde_json::from_str(raw) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("Error: --schema is not valid JSON: {e}");
                        std::process::exit(1);
                    }
                };
                p["schema"] = v;
            }
            ("memory.bb_create", p)
        }
        Put {
            workspace,
            name,
            field,
            value,
            value_b64,
            content_type,
            cas,
        } => {
            let mut p = serde_json::json!({
                "workspace_id": workspace,
                "name": name,
                "field": field,
            });
            if let Some(b64) = value_b64.as_deref() {
                p["value_b64"] = serde_json::json!(b64);
            } else if let Some(v) = value.as_deref() {
                let raw = match read_value_arg(v) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("Error: failed to read value file: {e}");
                        std::process::exit(1);
                    }
                };
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&raw) {
                    p["value"] = parsed;
                } else {
                    p["value"] = serde_json::Value::String(raw);
                }
            } else {
                eprintln!("Error: 'memory bb put' requires --value or --value-b64");
                std::process::exit(1);
            }
            if let Some(ct) = content_type.as_deref() {
                p["content_type"] = serde_json::json!(ct);
            }
            if let Some(c) = cas {
                p["cas"] = serde_json::json!(c);
            }
            ("memory.bb_put", p)
        }
        Get { workspace, name, field } => (
            "memory.bb_get",
            serde_json::json!({ "workspace_id": workspace, "name": name, "field": field }),
        ),
        GetAll { workspace, name } => (
            "memory.bb_get_all",
            serde_json::json!({ "workspace_id": workspace, "name": name }),
        ),
        GetMeta { workspace, name } => (
            "memory.bb_get_meta",
            serde_json::json!({ "workspace_id": workspace, "name": name }),
        ),
        DeleteField { workspace, name, field, cas } => {
            let mut p = serde_json::json!({
                "workspace_id": workspace,
                "name": name,
                "field": field,
            });
            if let Some(c) = cas {
                p["cas"] = serde_json::json!(c);
            }
            ("memory.bb_delete_field", p)
        }
        Delete { workspace, name } => (
            "memory.bb_delete",
            serde_json::json!({ "workspace_id": workspace, "name": name }),
        ),
        List { workspace } => (
            "memory.bb_list",
            serde_json::json!({ "workspace_id": workspace }),
        ),
        Exists { workspace, name } => (
            "memory.bb_exists",
            serde_json::json!({ "workspace_id": workspace, "name": name }),
        ),
        Snapshot { workspace, name, snapshot_id } => (
            "memory.bb_snapshot",
            serde_json::json!({
                "workspace_id": workspace,
                "name": name,
                "snapshot_id": snapshot_id,
            }),
        ),
        SnapshotGet { workspace, name, snapshot_id } => (
            "memory.bb_snapshot_get",
            serde_json::json!({
                "workspace_id": workspace,
                "name": name,
                "snapshot_id": snapshot_id,
            }),
        ),
        SnapshotList { workspace, name } => (
            "memory.bb_snapshot_list",
            serde_json::json!({ "workspace_id": workspace, "name": name }),
        ),
        SnapshotDelete { workspace, name, snapshot_id } => (
            "memory.bb_snapshot_delete",
            serde_json::json!({
                "workspace_id": workspace,
                "name": name,
                "snapshot_id": snapshot_id,
            }),
        ),
        SnapshotRestore { workspace, name, snapshot_id } => (
            "memory.bb_snapshot_restore",
            serde_json::json!({
                "workspace_id": workspace,
                "name": name,
                "snapshot_id": snapshot_id,
            }),
        ),
    }
}

/// 상대 TTL(초)을 절대 expires_at(unix ms)으로 환산.
fn ttl_to_expires_at(secs: u64) -> i64 {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let add_ms = secs.saturating_mul(1000).min(i64::MAX as u64) as i64;
    now_ms.saturating_add(add_ms)
}

fn memory_secret_command_to_method_params(
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

/// scope가 반드시 필요한 메서드용. 없으면 즉시 에러 exit.
fn require_scope(
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
