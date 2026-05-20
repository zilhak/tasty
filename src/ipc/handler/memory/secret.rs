//! `memory.secret.*` IPC handlers (secret 영역).

use serde_json::{Value, json};
use tasty_memory::{ListOpts, PutOpts, with_store};

use crate::ipc::caller::CallerContext;
use crate::ipc::protocol::JsonRpcResponse;
use crate::state::AppState;

use super::{
    map_error, optional_scope, parse_value, require_key, require_scope, require_store,
    secret_entry_to_json, stats_to_json,
};

pub fn handle_secret_put(
    _state: &mut AppState,
    caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    if let Err(e) = require_store(&id) {
        return e;
    }
    let scope = match require_scope(params, &id) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let key = match require_key(params, &id) {
        Ok(k) => k.to_string(),
        Err(e) => return e,
    };
    let value = match parse_value(params, &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let opts = PutOpts {
        expires_at: params.get("expires_at").and_then(|v| v.as_i64()),
        cas: params.get("cas").and_then(|v| v.as_u64()),
    };
    let owner = caller.owner().to_string();

    let result = with_store(|s| s.put_secret(&owner, &scope, &key, &value, &opts))
        .expect("memory store present");
    match result {
        Ok(version) => JsonRpcResponse::success(id, json!({ "ok": true, "version": version })),
        Err(e) => map_error(id, e),
    }
}

pub fn handle_secret_get(
    _state: &mut AppState,
    caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    if let Err(e) = require_store(&id) {
        return e;
    }
    let scope = match require_scope(params, &id) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let key = match require_key(params, &id) {
        Ok(k) => k.to_string(),
        Err(e) => return e,
    };
    let owner = caller.owner().to_string();
    let result = with_store(|s| s.get_secret(&owner, &scope, &key)).expect("memory store present");
    match result {
        Ok(Some(entry)) => JsonRpcResponse::success(id, secret_entry_to_json(&entry)),
        Ok(None) => JsonRpcResponse::success(id, Value::Null),
        Err(e) => map_error(id, e),
    }
}

pub fn handle_secret_delete(
    _state: &mut AppState,
    caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    if let Err(e) = require_store(&id) {
        return e;
    }
    let scope = match require_scope(params, &id) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let key = match require_key(params, &id) {
        Ok(k) => k.to_string(),
        Err(e) => return e,
    };
    let cas = params.get("cas").and_then(|v| v.as_u64());
    let owner = caller.owner().to_string();
    let result =
        with_store(|s| s.delete_secret(&owner, &scope, &key, cas)).expect("memory store present");
    match result {
        Ok(()) => JsonRpcResponse::success(id, json!({ "ok": true })),
        Err(e) => map_error(id, e),
    }
}

pub fn handle_secret_list(
    _state: &mut AppState,
    caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    if let Err(e) = require_store(&id) {
        return e;
    }
    let scope = match require_scope(params, &id) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let opts = ListOpts {
        prefix: params
            .get("prefix")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        limit: params
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize),
        since: params.get("since").and_then(|v| v.as_i64()),
        until: params.get("until").and_then(|v| v.as_i64()),
        offset: params
            .get("offset")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize),
    };
    let owner = caller.owner().to_string();
    let result =
        with_store(|s| s.list_secret(&owner, &scope, &opts)).expect("memory store present");
    match result {
        Ok(entries) => {
            let arr: Vec<Value> = entries.iter().map(secret_entry_to_json).collect();
            JsonRpcResponse::success(id, json!({ "entries": arr, "count": arr.len() }))
        }
        Err(e) => map_error(id, e),
    }
}

pub fn handle_secret_exists(
    _state: &mut AppState,
    caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    if let Err(e) = require_store(&id) {
        return e;
    }
    let scope = match require_scope(params, &id) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let key = match require_key(params, &id) {
        Ok(k) => k.to_string(),
        Err(e) => return e,
    };
    let owner = caller.owner().to_string();
    let result =
        with_store(|s| s.exists_secret(&owner, &scope, &key)).expect("memory store present");
    match result {
        Ok(b) => JsonRpcResponse::success(id, json!({ "exists": b })),
        Err(e) => map_error(id, e),
    }
}

pub fn handle_secret_count(
    _state: &mut AppState,
    caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    if let Err(e) = require_store(&id) {
        return e;
    }
    let scope = match require_scope(params, &id) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let prefix = params.get("prefix").and_then(|v| v.as_str());
    let owner = caller.owner().to_string();
    let result =
        with_store(|s| s.count_secret(&owner, &scope, prefix)).expect("memory store present");
    match result {
        Ok(n) => JsonRpcResponse::success(id, json!({ "count": n })),
        Err(e) => map_error(id, e),
    }
}

pub fn handle_secret_scopes(
    _state: &mut AppState,
    caller: &CallerContext,
    id: Value,
    _params: &Value,
) -> JsonRpcResponse {
    if let Err(e) = require_store(&id) {
        return e;
    }
    let owner = caller.owner().to_string();
    let result = with_store(|s| s.scopes_secret(&owner)).expect("memory store present");
    match result {
        Ok(list) => JsonRpcResponse::success(id, json!({ "scopes": list })),
        Err(e) => map_error(id, e),
    }
}

pub fn handle_secret_stats(
    _state: &mut AppState,
    caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    if let Err(e) = require_store(&id) {
        return e;
    }
    let scope = match optional_scope(params, &id) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let owner = caller.owner().to_string();
    let result =
        with_store(|s| s.stats_secret(&owner, scope.as_ref())).expect("memory store present");
    match result {
        Ok(stats) => JsonRpcResponse::success(id, stats_to_json(&stats)),
        Err(e) => map_error(id, e),
    }
}

// ============================================================
// Blackboard `memory.bb_*` — Phase 7.1
// ============================================================
//
// 워크스페이스 단위 키-값 컬렉션. `Scope::Workspace(workspace_id)` 한정.
// 키 컨벤션: `tasty.bb.<name>._meta` / `tasty.bb.<name>.fields.<field>`.
