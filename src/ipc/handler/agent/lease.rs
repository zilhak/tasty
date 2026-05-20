use serde_json::{Value, json};

use crate::ipc::caller::CallerContext;
use crate::ipc::protocol::JsonRpcResponse;
use crate::state::AppState;
use tasty_agent::{AgentError, LeaseMode, LeaseStore};
use tasty_memory::with_store;

use super::{agent_err_to_response, now_ms, workspace_id_param};

fn run_lease<F, R>(id: Value, f: F) -> JsonRpcResponse
where
    F: FnOnce(&mut LeaseStore<'_>) -> Result<R, AgentError>,
    R: serde::Serialize,
{
    let result = with_store(|mem| {
        let mut store = LeaseStore::new(mem, tasty_memory::HOST_OWNER);
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

fn resource_param(params: &Value, id: &Value) -> Result<String, JsonRpcResponse> {
    params
        .get("resource")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .ok_or_else(|| JsonRpcResponse::invalid_params(id.clone(), "Missing or empty 'resource'"))
}

fn holder_param(params: &Value, id: &Value) -> Result<String, JsonRpcResponse> {
    params
        .get("holder")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .ok_or_else(|| JsonRpcResponse::invalid_params(id.clone(), "Missing or empty 'holder'"))
}

pub fn handle_lease_acquire(
    _state: &mut AppState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let workspace_id = match workspace_id_param(params, &id) {
        Ok(w) => w,
        Err(e) => return e,
    };
    let resource = match resource_param(params, &id) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let holder = match holder_param(params, &id) {
        Ok(h) => h,
        Err(e) => return e,
    };
    let ttl_ms = params.get("ttl_ms").and_then(|v| v.as_u64());
    let mode = match params
        .get("mode")
        .and_then(|v| v.as_str())
        .unwrap_or("fail")
    {
        "fail" => LeaseMode::Fail,
        "block" => LeaseMode::Block,
        other => {
            return JsonRpcResponse::invalid_params(
                id,
                format!("'mode' must be 'fail' or 'block', got '{other}'"),
            );
        }
    };
    let now = now_ms();
    run_lease(id, move |store| {
        store.acquire(workspace_id, &resource, &holder, ttl_ms, mode, now)
    })
}

pub fn handle_lease_release(
    _state: &mut AppState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let workspace_id = match workspace_id_param(params, &id) {
        Ok(w) => w,
        Err(e) => return e,
    };
    let resource = match resource_param(params, &id) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let holder = match holder_param(params, &id) {
        Ok(h) => h,
        Err(e) => return e,
    };
    run_lease(id, move |store| {
        store.release(workspace_id, &resource, &holder)
    })
}

pub fn handle_lease_list(
    _state: &mut AppState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let workspace_id = match workspace_id_param(params, &id) {
        Ok(w) => w,
        Err(e) => return e,
    };
    let now = now_ms();
    run_lease(id, move |store| {
        let leases = store.list(workspace_id, Some(now))?;
        Ok(json!({ "total": leases.len(), "leases": leases }))
    })
}

// ============================================================
// agent.task_reduce — 다른 task 결과 합성
// ============================================================
