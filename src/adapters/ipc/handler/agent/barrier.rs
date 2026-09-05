use serde_json::{Value, json};

use crate::adapters::ipc::handler::params::{self, p_try};
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

pub fn handle_barrier_create(
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
    let count_required = match p_try!(params::opt_int::<u64>(params, "count_required", &id)) {
        Some(c) if c <= u32::MAX as u64 => c as u32,
        _ => {
            return JsonRpcResponse::invalid_params(
                id,
                "Missing or invalid 'count_required' (must be u32 >= 1)",
            );
        }
    };
    let timeout_ms = p_try!(params::opt_int::<u64>(params, "timeout_ms", &id));
    match core.barrier_create(workspace_id, name, count_required, timeout_ms, now_ms()) {
        Ok(b) => serialize(id, b),
        Err(e) => agent_err_to_response(id, e),
    }
}

pub fn handle_barrier_signal(
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
    match core.barrier_signal(workspace_id, &name, now_ms()) {
        Ok(b) => serialize(id, b),
        Err(e) => agent_err_to_response(id, e),
    }
}

pub fn handle_barrier_state(
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
    match core.barrier_state(workspace_id, &name, now_ms()) {
        Ok(b) => serialize(id, b),
        Err(e) => agent_err_to_response(id, e),
    }
}

/// poll-based — 상태 조회와 동일. 추후 blocking + wakeup 도입.
pub fn handle_barrier_await(
    core: &Core,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    handle_barrier_state(core, state, engine, caller, id, params)
}

pub fn handle_barrier_list(
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
    match core.barrier_list(workspace_id, Some(now_ms())) {
        Ok(barriers) => {
            JsonRpcResponse::success(id, json!({ "total": barriers.len(), "barriers": barriers }))
        }
        Err(e) => agent_err_to_response(id, e),
    }
}

pub fn handle_barrier_delete(
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
    match core.barrier_delete(workspace_id, &name) {
        Ok(()) => JsonRpcResponse::success(id, json!({ "deleted": true })),
        Err(e) => agent_err_to_response(id, e),
    }
}

// ============================================================
// agent.semaphore_*
// ============================================================
