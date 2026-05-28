use serde_json::Value;

use crate::ipc::caller::CallerContext;
use crate::ipc::protocol::JsonRpcResponse;
use crate::state::AppState;
use tasty_agent::{AgentError, BarrierStore};
use tasty_memory::with_store;

use super::{agent_err_to_response, name_param, now_ms, workspace_id_param};

fn run_barrier<F, R>(id: Value, f: F) -> JsonRpcResponse
where
    F: FnOnce(&mut BarrierStore<'_>) -> Result<R, AgentError>,
    R: serde::Serialize,
{
    let result = with_store(|mem| {
        let mut store = BarrierStore::new(mem, tasty_memory::HOST_OWNER);
        f(&mut store)
    });
    match result {
        None => JsonRpcResponse::error(id, -32603, "memory store not initialized"),
        Some(Ok(v)) => match serde_json::to_value(v) {
            Ok(json) => JsonRpcResponse::success(id, json),
            Err(e) => JsonRpcResponse::error(id, -32603, &format!("serialize: {e}")),
        },
        Some(Err(e)) => agent_err_to_response(id, e),
    }
}

pub fn handle_barrier_create(
    _state: &mut AppState,
    _engine: &mut crate::engine_state::CoreState,
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
    let count_required = match params.get("count_required").and_then(|v| v.as_u64()) {
        Some(c) if c <= u32::MAX as u64 => c as u32,
        _ => {
            return JsonRpcResponse::invalid_params(
                id,
                "Missing or invalid 'count_required' (must be u32 >= 1)",
            );
        }
    };
    let timeout_ms = params.get("timeout_ms").and_then(|v| v.as_u64());
    let now = now_ms();
    run_barrier(id, move |store| {
        store.create(workspace_id, name, count_required, timeout_ms, now)
    })
}

pub fn handle_barrier_signal(
    _state: &mut AppState,
    _engine: &mut crate::engine_state::CoreState,
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
    let now = now_ms();
    run_barrier(id, move |store| store.signal(workspace_id, &name, now))
}

pub fn handle_barrier_state(
    _state: &mut AppState,
    _engine: &mut crate::engine_state::CoreState,
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
    let now = now_ms();
    run_barrier(id, move |store| store.state(workspace_id, &name, now))
}

/// Phase 5.2 단계: poll-based — 상태 조회와 동일. 추후 blocking + wakeup 도입.
pub fn handle_barrier_await(
    state: &mut AppState,
    engine: &mut crate::engine_state::CoreState,
    caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    handle_barrier_state(state, engine, caller, id, params)
}

// ============================================================
// agent.semaphore_*
// ============================================================
