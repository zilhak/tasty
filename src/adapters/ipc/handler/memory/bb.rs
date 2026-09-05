//! `memory.bb.*` (blackboard) IPC handlers.

use crate::adapters::ipc::handler::params::{self, p_try};
use serde_json::{Value, json};
use tasty_memory::blackboard;

use crate::core::Core;
use crate::state::AppState;
use tasty_ipc::caller::CallerContext;
use tasty_ipc::protocol::JsonRpcResponse;

use super::{entry_to_json, map_error, parse_value, require_str, require_workspace_id};

pub fn handle_bb_create(
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
    let name = match require_str(params, "name", &id) {
        Ok(s) => s.to_string(),
        Err(e) => return e,
    };
    let schema = params.get("schema").cloned();
    let owner = caller.owner().to_string();
    match core.with_memory(|s| blackboard::bb_create(s, &owner, workspace_id, &name, schema)) {
        Ok(version) => JsonRpcResponse::success(id, json!({ "ok": true, "version": version })),
        Err(e) => map_error(id, e),
    }
}

pub fn handle_bb_put(
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
    let name = match require_str(params, "name", &id) {
        Ok(s) => s.to_string(),
        Err(e) => return e,
    };
    let field = match require_str(params, "field", &id) {
        Ok(s) => s.to_string(),
        Err(e) => return e,
    };
    let value = match parse_value(params, &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let cas = p_try!(params::opt_int::<u64>(params, "cas", &id));
    let owner = caller.owner().to_string();
    match core
        .with_memory(|s| blackboard::bb_put(s, &owner, workspace_id, &name, &field, &value, cas))
    {
        Ok(version) => JsonRpcResponse::success(id, json!({ "ok": true, "version": version })),
        Err(e) => map_error(id, e),
    }
}

pub fn handle_bb_get(
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
    let name = match require_str(params, "name", &id) {
        Ok(s) => s.to_string(),
        Err(e) => return e,
    };
    let field = match require_str(params, "field", &id) {
        Ok(s) => s.to_string(),
        Err(e) => return e,
    };
    match core.with_memory(|s| blackboard::bb_get(s, workspace_id, &name, &field)) {
        Ok(Some(entry)) => JsonRpcResponse::success(id, entry_to_json(&entry)),
        Ok(None) => JsonRpcResponse::success(id, Value::Null),
        Err(e) => map_error(id, e),
    }
}

pub fn handle_bb_get_all(
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
    let name = match require_str(params, "name", &id) {
        Ok(s) => s.to_string(),
        Err(e) => return e,
    };
    match core.with_memory(|s| blackboard::bb_get_all(s, workspace_id, &name)) {
        Ok(entries) => {
            let arr: Vec<Value> = entries.iter().map(entry_to_json).collect();
            JsonRpcResponse::success(id, json!({ "entries": arr, "count": arr.len() }))
        }
        Err(e) => map_error(id, e),
    }
}

pub fn handle_bb_get_meta(
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
    let name = match require_str(params, "name", &id) {
        Ok(s) => s.to_string(),
        Err(e) => return e,
    };
    match core.with_memory(|s| blackboard::bb_get_meta(s, workspace_id, &name)) {
        Ok(Some(entry)) => JsonRpcResponse::success(id, entry_to_json(&entry)),
        Ok(None) => JsonRpcResponse::success(id, Value::Null),
        Err(e) => map_error(id, e),
    }
}

pub fn handle_bb_delete_field(
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
    let name = match require_str(params, "name", &id) {
        Ok(s) => s.to_string(),
        Err(e) => return e,
    };
    let field = match require_str(params, "field", &id) {
        Ok(s) => s.to_string(),
        Err(e) => return e,
    };
    let cas = p_try!(params::opt_int::<u64>(params, "cas", &id));
    let owner = caller.owner().to_string();
    match core
        .with_memory(|s| blackboard::bb_delete_field(s, &owner, workspace_id, &name, &field, cas))
    {
        Ok(()) => JsonRpcResponse::success(id, json!({ "ok": true })),
        Err(e) => map_error(id, e),
    }
}

pub fn handle_bb_delete(
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
    let name = match require_str(params, "name", &id) {
        Ok(s) => s.to_string(),
        Err(e) => return e,
    };
    let owner = caller.owner().to_string();
    match core.with_memory(|s| blackboard::bb_delete(s, &owner, workspace_id, &name)) {
        Ok(removed) => JsonRpcResponse::success(id, json!({ "ok": true, "removed": removed })),
        Err(e) => map_error(id, e),
    }
}

pub fn handle_bb_list(
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
    match core.with_memory(|s| blackboard::bb_list(s, workspace_id)) {
        Ok(names) => JsonRpcResponse::success(id, json!({ "names": names, "count": names.len() })),
        Err(e) => map_error(id, e),
    }
}

pub fn handle_bb_exists(
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
    let name = match require_str(params, "name", &id) {
        Ok(s) => s.to_string(),
        Err(e) => return e,
    };
    match core.with_memory(|s| blackboard::bb_exists(s, workspace_id, &name)) {
        Ok(exists) => JsonRpcResponse::success(id, json!({ "exists": exists })),
        Err(e) => map_error(id, e),
    }
}

// ---- blackboard snapshot ----

pub fn handle_bb_snapshot(
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
    let name = match require_str(params, "name", &id) {
        Ok(s) => s.to_string(),
        Err(e) => return e,
    };
    let snapshot_id = match require_str(params, "snapshot_id", &id) {
        Ok(s) => s.to_string(),
        Err(e) => return e,
    };
    let owner = caller.owner().to_string();
    match core
        .with_memory(|s| blackboard::bb_snapshot(s, &owner, workspace_id, &name, &snapshot_id))
    {
        Ok(version) => JsonRpcResponse::success(id, json!({ "ok": true, "version": version })),
        Err(e) => map_error(id, e),
    }
}

pub fn handle_bb_snapshot_get(
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
    let name = match require_str(params, "name", &id) {
        Ok(s) => s.to_string(),
        Err(e) => return e,
    };
    let snapshot_id = match require_str(params, "snapshot_id", &id) {
        Ok(s) => s.to_string(),
        Err(e) => return e,
    };
    match core.with_memory(|s| blackboard::bb_snapshot_get(s, workspace_id, &name, &snapshot_id)) {
        Ok(Some(snap)) => match serde_json::to_value(&snap) {
            Ok(v) => JsonRpcResponse::success(id, v),
            Err(e) => JsonRpcResponse::internal_error(id, format!("serialize: {e}")),
        },
        Ok(None) => JsonRpcResponse::success(id, Value::Null),
        Err(e) => map_error(id, e),
    }
}

pub fn handle_bb_snapshot_list(
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
    let name = match require_str(params, "name", &id) {
        Ok(s) => s.to_string(),
        Err(e) => return e,
    };
    match core.with_memory(|s| blackboard::bb_snapshot_list(s, workspace_id, &name)) {
        Ok(ids) => JsonRpcResponse::success(id, json!({ "snapshot_ids": ids, "count": ids.len() })),
        Err(e) => map_error(id, e),
    }
}

pub fn handle_bb_snapshot_delete(
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
    let name = match require_str(params, "name", &id) {
        Ok(s) => s.to_string(),
        Err(e) => return e,
    };
    let snapshot_id = match require_str(params, "snapshot_id", &id) {
        Ok(s) => s.to_string(),
        Err(e) => return e,
    };
    let owner = caller.owner().to_string();
    match core.with_memory(|s| {
        blackboard::bb_snapshot_delete(s, &owner, workspace_id, &name, &snapshot_id)
    }) {
        Ok(()) => JsonRpcResponse::success(id, json!({ "ok": true })),
        Err(e) => map_error(id, e),
    }
}

pub fn handle_bb_snapshot_restore(
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
    let name = match require_str(params, "name", &id) {
        Ok(s) => s.to_string(),
        Err(e) => return e,
    };
    let snapshot_id = match require_str(params, "snapshot_id", &id) {
        Ok(s) => s.to_string(),
        Err(e) => return e,
    };
    let owner = caller.owner().to_string();
    match core.with_memory(|s| {
        blackboard::bb_snapshot_restore(s, &owner, workspace_id, &name, &snapshot_id)
    }) {
        Ok(restored) => JsonRpcResponse::success(id, json!({ "ok": true, "restored": restored })),
        Err(e) => map_error(id, e),
    }
}

// ============================================================
// Plan `memory.plan_*`
// ============================================================
//
// 워크스페이스 단위 선언적 work breakdown. 한 plan = `tasty.plan.<plan_id>`
// JSON entry 한 개. step state 변경마다 전체 plan put 1 회.
