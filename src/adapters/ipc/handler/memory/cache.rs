//! `memory.cache.*` IPC handlers.

use serde_json::{Value, json};
use tasty_memory::cache as cache_mod;

use crate::core::Core;
use crate::ipc::caller::CallerContext;
use crate::ipc::protocol::JsonRpcResponse;
use crate::state::AppState;

use super::{entry_to_json, map_error, parse_value, require_str, require_workspace_id};

pub fn handle_cache_put(
    core: &Core,
    _state: &mut AppState,
    _engine: &mut crate::core::CoreState,
    caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let workspace_id = match require_workspace_id(params, &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let key = match require_str(params, "key", &id) {
        Ok(s) => s.to_string(),
        Err(e) => return e,
    };
    let value = match parse_value(params, &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let ttl_secs = match params.get("ttl_secs").and_then(|v| v.as_u64()) {
        Some(n) => n,
        None => {
            return JsonRpcResponse::invalid_params(id, "Missing 'ttl_secs' (>0)");
        }
    };
    let owner = caller.owner().to_string();
    match core
        .with_memory(|s| cache_mod::cache_put(s, &owner, workspace_id, &key, &value, ttl_secs))
    {
        Ok(version) => JsonRpcResponse::success(id, json!({ "ok": true, "version": version })),
        Err(e) => map_error(id, e),
    }
}

pub fn handle_cache_get(
    core: &Core,
    _state: &mut AppState,
    _engine: &mut crate::core::CoreState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let workspace_id = match require_workspace_id(params, &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let key = match require_str(params, "key", &id) {
        Ok(s) => s.to_string(),
        Err(e) => return e,
    };
    match core.with_memory(|s| cache_mod::cache_get(s, workspace_id, &key)) {
        Ok(Some(entry)) => JsonRpcResponse::success(id, entry_to_json(&entry)),
        Ok(None) => JsonRpcResponse::success(id, Value::Null),
        Err(e) => map_error(id, e),
    }
}

pub fn handle_cache_invalidate(
    core: &Core,
    _state: &mut AppState,
    _engine: &mut crate::core::CoreState,
    caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let workspace_id = match require_workspace_id(params, &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let key = match require_str(params, "key", &id) {
        Ok(s) => s.to_string(),
        Err(e) => return e,
    };
    let owner = caller.owner().to_string();
    match core.with_memory(|s| cache_mod::cache_invalidate(s, &owner, workspace_id, &key)) {
        Ok(()) => JsonRpcResponse::success(id, json!({ "ok": true })),
        Err(e) => map_error(id, e),
    }
}

pub fn handle_cache_clear(
    core: &Core,
    _state: &mut AppState,
    _engine: &mut crate::core::CoreState,
    caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let workspace_id = match require_workspace_id(params, &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let owner = caller.owner().to_string();
    match core.with_memory(|s| cache_mod::cache_clear(s, &owner, workspace_id)) {
        Ok(removed) => JsonRpcResponse::success(id, json!({ "ok": true, "removed": removed })),
        Err(e) => map_error(id, e),
    }
}

pub fn handle_cache_list(
    core: &Core,
    _state: &mut AppState,
    _engine: &mut crate::core::CoreState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let workspace_id = match require_workspace_id(params, &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    match core.with_memory(|s| cache_mod::cache_list(s, workspace_id)) {
        Ok(keys) => JsonRpcResponse::success(id, json!({ "keys": keys, "count": keys.len() })),
        Err(e) => map_error(id, e),
    }
}
