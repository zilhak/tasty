use serde_json::Value;

use crate::core::Core;
use crate::ipc::caller::CallerContext;
use crate::ipc::protocol::JsonRpcResponse;
use crate::state::AppState;
use tasty_agent::{AgentError, SemaphoreStore};

use super::{agent_err_to_response, name_param, now_ms, workspace_id_param};

fn run_semaphore<F, R>(core: &Core, id: Value, f: F) -> JsonRpcResponse
where
    F: FnOnce(&mut SemaphoreStore<'_>) -> Result<R, AgentError>,
    R: serde::Serialize,
{
    let result = core.with_memory(|mem| {
        let mut store = SemaphoreStore::new(mem, tasty_memory::HOST_OWNER);
        f(&mut store)
    });
    match result {
        Ok(v) => match serde_json::to_value(v) {
            Ok(json) => JsonRpcResponse::success(id, json),
            Err(e) => JsonRpcResponse::error(id, -32603, &format!("serialize: {e}")),
        },
        Err(e) => agent_err_to_response(id, e),
    }
}

pub fn handle_semaphore_create(
    core: &Core,
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
    let permits = match params.get("permits").and_then(|v| v.as_u64()) {
        Some(c) if c <= u32::MAX as u64 => c as u32,
        _ => {
            return JsonRpcResponse::invalid_params(
                id,
                "Missing or invalid 'permits' (must be u32 >= 1)",
            );
        }
    };
    let now = now_ms();
    run_semaphore(core, id, move |store| {
        store.create(workspace_id, name, permits, now)
    })
}

pub fn handle_semaphore_acquire(
    core: &Core,
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
    let holder = match params.get("holder").and_then(|v| v.as_str()) {
        Some(h) if !h.is_empty() => h.to_string(),
        _ => return JsonRpcResponse::invalid_params(id, "Missing or empty 'holder'"),
    };
    run_semaphore(core, id, move |store| {
        store.acquire(workspace_id, &name, &holder)
    })
}

pub fn handle_semaphore_release(
    core: &Core,
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
    let holder = match params.get("holder").and_then(|v| v.as_str()) {
        Some(h) if !h.is_empty() => h.to_string(),
        _ => return JsonRpcResponse::invalid_params(id, "Missing or empty 'holder'"),
    };
    run_semaphore(core, id, move |store| {
        store.release(workspace_id, &name, &holder)
    })
}

// ============================================================
// agent.lease_*
// ============================================================
