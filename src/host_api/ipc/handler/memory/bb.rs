//! `memory.bb.*` (blackboard) IPC handlers.

use serde_json::{Value, json};
use tasty_memory::{blackboard, with_store};

use crate::ipc::caller::CallerContext;
use crate::ipc::protocol::JsonRpcResponse;
use crate::state::AppState;

use super::{
    entry_to_json, map_error, parse_value, require_store, require_str, require_workspace_id,
};

pub fn handle_bb_create(
    _state: &mut AppState,
    _engine: &mut crate::engine_state::EngineState,
    caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    if let Err(e) = require_store(&id) {
        return e;
    }
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
    let result = with_store(|s| blackboard::bb_create(s, &owner, workspace_id, &name, schema))
        .expect("memory store present");
    match result {
        Ok(version) => JsonRpcResponse::success(id, json!({ "ok": true, "version": version })),
        Err(e) => map_error(id, e),
    }
}

pub fn handle_bb_put(
    _state: &mut AppState,
    _engine: &mut crate::engine_state::EngineState,
    caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    if let Err(e) = require_store(&id) {
        return e;
    }
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
    let cas = params.get("cas").and_then(|v| v.as_u64());
    let owner = caller.owner().to_string();
    let result =
        with_store(|s| blackboard::bb_put(s, &owner, workspace_id, &name, &field, &value, cas))
            .expect("memory store present");
    match result {
        Ok(version) => JsonRpcResponse::success(id, json!({ "ok": true, "version": version })),
        Err(e) => map_error(id, e),
    }
}

pub fn handle_bb_get(
    _state: &mut AppState,
    _engine: &mut crate::engine_state::EngineState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    if let Err(e) = require_store(&id) {
        return e;
    }
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
    let result = with_store(|s| blackboard::bb_get(s, workspace_id, &name, &field))
        .expect("memory store present");
    match result {
        Ok(Some(entry)) => JsonRpcResponse::success(id, entry_to_json(&entry)),
        Ok(None) => JsonRpcResponse::success(id, Value::Null),
        Err(e) => map_error(id, e),
    }
}

pub fn handle_bb_get_all(
    _state: &mut AppState,
    _engine: &mut crate::engine_state::EngineState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    if let Err(e) = require_store(&id) {
        return e;
    }
    let workspace_id = match require_workspace_id(params, &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let name = match require_str(params, "name", &id) {
        Ok(s) => s.to_string(),
        Err(e) => return e,
    };
    let result = with_store(|s| blackboard::bb_get_all(s, workspace_id, &name))
        .expect("memory store present");
    match result {
        Ok(entries) => {
            let arr: Vec<Value> = entries.iter().map(entry_to_json).collect();
            JsonRpcResponse::success(id, json!({ "entries": arr, "count": arr.len() }))
        }
        Err(e) => map_error(id, e),
    }
}

pub fn handle_bb_get_meta(
    _state: &mut AppState,
    _engine: &mut crate::engine_state::EngineState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    if let Err(e) = require_store(&id) {
        return e;
    }
    let workspace_id = match require_workspace_id(params, &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let name = match require_str(params, "name", &id) {
        Ok(s) => s.to_string(),
        Err(e) => return e,
    };
    let result = with_store(|s| blackboard::bb_get_meta(s, workspace_id, &name))
        .expect("memory store present");
    match result {
        Ok(Some(entry)) => JsonRpcResponse::success(id, entry_to_json(&entry)),
        Ok(None) => JsonRpcResponse::success(id, Value::Null),
        Err(e) => map_error(id, e),
    }
}

pub fn handle_bb_delete_field(
    _state: &mut AppState,
    _engine: &mut crate::engine_state::EngineState,
    caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    if let Err(e) = require_store(&id) {
        return e;
    }
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
    let cas = params.get("cas").and_then(|v| v.as_u64());
    let owner = caller.owner().to_string();
    let result =
        with_store(|s| blackboard::bb_delete_field(s, &owner, workspace_id, &name, &field, cas))
            .expect("memory store present");
    match result {
        Ok(()) => JsonRpcResponse::success(id, json!({ "ok": true })),
        Err(e) => map_error(id, e),
    }
}

pub fn handle_bb_delete(
    _state: &mut AppState,
    _engine: &mut crate::engine_state::EngineState,
    caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    if let Err(e) = require_store(&id) {
        return e;
    }
    let workspace_id = match require_workspace_id(params, &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let name = match require_str(params, "name", &id) {
        Ok(s) => s.to_string(),
        Err(e) => return e,
    };
    let owner = caller.owner().to_string();
    let result = with_store(|s| blackboard::bb_delete(s, &owner, workspace_id, &name))
        .expect("memory store present");
    match result {
        Ok(removed) => JsonRpcResponse::success(id, json!({ "ok": true, "removed": removed })),
        Err(e) => map_error(id, e),
    }
}

pub fn handle_bb_list(
    _state: &mut AppState,
    _engine: &mut crate::engine_state::EngineState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    if let Err(e) = require_store(&id) {
        return e;
    }
    let workspace_id = match require_workspace_id(params, &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let result =
        with_store(|s| blackboard::bb_list(s, workspace_id)).expect("memory store present");
    match result {
        Ok(names) => JsonRpcResponse::success(id, json!({ "names": names, "count": names.len() })),
        Err(e) => map_error(id, e),
    }
}

pub fn handle_bb_exists(
    _state: &mut AppState,
    _engine: &mut crate::engine_state::EngineState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    if let Err(e) = require_store(&id) {
        return e;
    }
    let workspace_id = match require_workspace_id(params, &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let name = match require_str(params, "name", &id) {
        Ok(s) => s.to_string(),
        Err(e) => return e,
    };
    let result = with_store(|s| blackboard::bb_exists(s, workspace_id, &name))
        .expect("memory store present");
    match result {
        Ok(exists) => JsonRpcResponse::success(id, json!({ "exists": exists })),
        Err(e) => map_error(id, e),
    }
}

// ---- blackboard snapshot (Phase 7.4) ----

pub fn handle_bb_snapshot(
    _state: &mut AppState,
    _engine: &mut crate::engine_state::EngineState,
    caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    if let Err(e) = require_store(&id) {
        return e;
    }
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
    let result =
        with_store(|s| blackboard::bb_snapshot(s, &owner, workspace_id, &name, &snapshot_id))
            .expect("memory store present");
    match result {
        Ok(version) => JsonRpcResponse::success(id, json!({ "ok": true, "version": version })),
        Err(e) => map_error(id, e),
    }
}

pub fn handle_bb_snapshot_get(
    _state: &mut AppState,
    _engine: &mut crate::engine_state::EngineState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    if let Err(e) = require_store(&id) {
        return e;
    }
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
    let result = with_store(|s| blackboard::bb_snapshot_get(s, workspace_id, &name, &snapshot_id))
        .expect("memory store present");
    match result {
        Ok(Some(snap)) => match serde_json::to_value(&snap) {
            Ok(v) => JsonRpcResponse::success(id, v),
            Err(e) => JsonRpcResponse::internal_error(id, format!("serialize: {e}")),
        },
        Ok(None) => JsonRpcResponse::success(id, Value::Null),
        Err(e) => map_error(id, e),
    }
}

pub fn handle_bb_snapshot_list(
    _state: &mut AppState,
    _engine: &mut crate::engine_state::EngineState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    if let Err(e) = require_store(&id) {
        return e;
    }
    let workspace_id = match require_workspace_id(params, &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let name = match require_str(params, "name", &id) {
        Ok(s) => s.to_string(),
        Err(e) => return e,
    };
    let result = with_store(|s| blackboard::bb_snapshot_list(s, workspace_id, &name))
        .expect("memory store present");
    match result {
        Ok(ids) => JsonRpcResponse::success(id, json!({ "snapshot_ids": ids, "count": ids.len() })),
        Err(e) => map_error(id, e),
    }
}

pub fn handle_bb_snapshot_delete(
    _state: &mut AppState,
    _engine: &mut crate::engine_state::EngineState,
    caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    if let Err(e) = require_store(&id) {
        return e;
    }
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
    let result = with_store(|s| {
        blackboard::bb_snapshot_delete(s, &owner, workspace_id, &name, &snapshot_id)
    })
    .expect("memory store present");
    match result {
        Ok(()) => JsonRpcResponse::success(id, json!({ "ok": true })),
        Err(e) => map_error(id, e),
    }
}

pub fn handle_bb_snapshot_restore(
    _state: &mut AppState,
    _engine: &mut crate::engine_state::EngineState,
    caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    if let Err(e) = require_store(&id) {
        return e;
    }
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
    let result = with_store(|s| {
        blackboard::bb_snapshot_restore(s, &owner, workspace_id, &name, &snapshot_id)
    })
    .expect("memory store present");
    match result {
        Ok(restored) => JsonRpcResponse::success(id, json!({ "ok": true, "restored": restored })),
        Err(e) => map_error(id, e),
    }
}

// ============================================================
// Plan `memory.plan_*` — Phase 7.2
// ============================================================
//
// 워크스페이스 단위 선언적 work breakdown. 한 plan = `tasty.plan.<plan_id>`
// JSON entry 한 개. step state 변경마다 전체 plan put 1 회.
