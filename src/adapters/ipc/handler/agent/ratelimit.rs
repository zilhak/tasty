use serde_json::{Value, json};

use crate::ipc::caller::CallerContext;
use crate::ipc::protocol::JsonRpcResponse;
use crate::state::AppState;
use tasty_agent::{AgentError, RateLimitStore};
use tasty_memory::with_store;

use super::{agent_err_to_response, now_ms};

fn run_rate_limit<F, R>(id: Value, f: F) -> JsonRpcResponse
where
    F: FnOnce(&mut RateLimitStore<'_>) -> Result<R, AgentError>,
    R: serde::Serialize,
{
    let result = with_store(|mem| {
        let mut store = RateLimitStore::new(mem, tasty_memory::HOST_OWNER);
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

pub fn handle_rate_limit_set(
    _state: &mut AppState,
    _engine: &mut crate::engine_state::EngineState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let agent = match params.get("agent").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return JsonRpcResponse::invalid_params(id, "Missing or empty 'agent'"),
    };
    let metric = match params.get("metric").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return JsonRpcResponse::invalid_params(id, "Missing or empty 'metric'"),
    };
    let limit = match params.get("limit").and_then(|v| v.as_u64()) {
        Some(c) if c >= 1 && c <= u32::MAX as u64 => c as u32,
        _ => {
            return JsonRpcResponse::invalid_params(
                id,
                "Missing or invalid 'limit' (must be u32 >= 1)",
            );
        }
    };
    let per_ms = match params.get("per_ms").and_then(|v| v.as_u64()) {
        Some(c) if c >= 1 => c,
        _ => {
            return JsonRpcResponse::invalid_params(
                id,
                "Missing or invalid 'per_ms' (must be u64 >= 1)",
            );
        }
    };
    let burst = match params.get("burst").and_then(|v| v.as_u64()) {
        Some(c) if c <= u32::MAX as u64 => Some(c as u32),
        Some(_) => {
            return JsonRpcResponse::invalid_params(id, "'burst' exceeds u32::MAX");
        }
        None => None,
    };
    let now = now_ms();
    run_rate_limit(id, move |store| {
        store.set(agent, metric, limit, per_ms, burst, now)
    })
}

pub fn handle_rate_limit_list(
    _state: &mut AppState,
    _engine: &mut crate::engine_state::EngineState,
    _caller: &CallerContext,
    id: Value,
    _params: &Value,
) -> JsonRpcResponse {
    let now = now_ms();
    run_rate_limit(id, move |store| {
        let all = store.status(now)?;
        Ok(json!({ "total": all.len(), "rate_limits": all }))
    })
}

pub fn handle_rate_limit_remove(
    _state: &mut AppState,
    _engine: &mut crate::engine_state::EngineState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let rl_id = match params.get("id").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return JsonRpcResponse::invalid_params(id, "Missing or empty 'id'"),
    };
    run_rate_limit(id, move |store| {
        store.remove(&rl_id)?;
        Ok(json!({ "ok": true }))
    })
}

pub fn handle_rate_limit_status(
    _state: &mut AppState,
    _engine: &mut crate::engine_state::EngineState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let agent = params
        .get("agent")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let metric = params
        .get("metric")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let now = now_ms();
    run_rate_limit(id, move |store| {
        let all = store.status(now)?;
        let filtered: Vec<_> = all
            .into_iter()
            .filter(|r| agent.as_ref().is_none_or(|a| &r.agent == a))
            .filter(|r| metric.as_ref().is_none_or(|m| &r.metric == m))
            .collect();
        Ok(json!({ "total": filtered.len(), "rate_limits": filtered }))
    })
}
