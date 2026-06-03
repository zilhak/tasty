use serde_json::Value;

use crate::core::Core;
use crate::state::AppState;
use tasty_ipc::caller::CallerContext;
use tasty_ipc::protocol::JsonRpcResponse;

use super::{agent_err_to_response, name_param, now_ms, workspace_id_param};

fn serialize<T: serde::Serialize>(id: Value, value: T) -> JsonRpcResponse {
    match serde_json::to_value(value) {
        Ok(v) => JsonRpcResponse::success(id, v),
        Err(e) => JsonRpcResponse::error(id, -32603, format!("serialize: {e}")),
    }
}

pub fn handle_semaphore_create(
    core: &Core,
    _state: &mut AppState,
    _engine: &mut crate::core::CoreState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let workspace_id = match workspace_id_param(params, &id) {
        Ok(w) => w,
        Err(e) => return e,
    };
    let name = match name_param(params, &id) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let permits = match params.get("permits").and_then(|v| v.as_u64()) {
        Some(c) if c <= u32::MAX as u64 => c as u32,
        _ => {
            return JsonRpcResponse::invalid_params(
                id,
                "Missing or invalid 'permits' (must be u32 >= 1)",
            );
        }
    };
    match core.semaphore_create(workspace_id, name, permits, now_ms()) {
        Ok(s) => serialize(id, s),
        Err(e) => agent_err_to_response(id, e),
    }
}

pub fn handle_semaphore_acquire(
    core: &Core,
    _state: &mut AppState,
    _engine: &mut crate::core::CoreState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let workspace_id = match workspace_id_param(params, &id) {
        Ok(w) => w,
        Err(e) => return e,
    };
    let name = match name_param(params, &id) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let holder = match params.get("holder").and_then(|v| v.as_str()) {
        Some(h) if !h.is_empty() => h.to_string(),
        _ => return JsonRpcResponse::invalid_params(id, "Missing or empty 'holder'"),
    };
    match core.semaphore_acquire(workspace_id, &name, &holder) {
        Ok(outcome) => serialize(id, outcome),
        Err(e) => agent_err_to_response(id, e),
    }
}

pub fn handle_semaphore_release(
    core: &Core,
    _state: &mut AppState,
    _engine: &mut crate::core::CoreState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let workspace_id = match workspace_id_param(params, &id) {
        Ok(w) => w,
        Err(e) => return e,
    };
    let name = match name_param(params, &id) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let holder = match params.get("holder").and_then(|v| v.as_str()) {
        Some(h) if !h.is_empty() => h.to_string(),
        _ => return JsonRpcResponse::invalid_params(id, "Missing or empty 'holder'"),
    };
    match core.semaphore_release(workspace_id, &name, &holder) {
        Ok(outcome) => serialize(id, outcome),
        Err(e) => agent_err_to_response(id, e),
    }
}

// ============================================================
// agent.lease_*
// ============================================================
