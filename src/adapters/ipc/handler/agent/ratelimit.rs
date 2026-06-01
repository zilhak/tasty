use serde_json::{Value, json};

use crate::core::Core;
use crate::ipc::caller::CallerContext;
use crate::ipc::protocol::JsonRpcResponse;
use crate::state::AppState;

use super::{agent_err_to_response, now_ms};

fn serialize<T: serde::Serialize>(id: Value, value: T) -> JsonRpcResponse {
    match serde_json::to_value(value) {
        Ok(v) => JsonRpcResponse::success(id, v),
        Err(e) => JsonRpcResponse::error(id, -32603, &format!("serialize: {e}")),
    }
}

pub fn handle_rate_limit_set(
    core: &Core,
    _state: &mut AppState,
    _engine: &mut crate::engine_state::CoreState,
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
    match core.rate_limit_set(agent, metric, limit, per_ms, burst, now_ms()) {
        Ok(rl) => serialize(id, rl),
        Err(e) => agent_err_to_response(id, e),
    }
}

pub fn handle_rate_limit_list(
    core: &Core,
    _state: &mut AppState,
    _engine: &mut crate::engine_state::CoreState,
    _caller: &CallerContext,
    id: Value,
    _params: &Value,
) -> JsonRpcResponse {
    match core.rate_limit_status(now_ms()) {
        Ok(all) => JsonRpcResponse::success(id, json!({ "total": all.len(), "rate_limits": all })),
        Err(e) => agent_err_to_response(id, e),
    }
}

pub fn handle_rate_limit_remove(
    core: &Core,
    _state: &mut AppState,
    _engine: &mut crate::engine_state::CoreState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let rl_id = match params.get("id").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return JsonRpcResponse::invalid_params(id, "Missing or empty 'id'"),
    };
    match core.rate_limit_remove(&rl_id) {
        Ok(()) => JsonRpcResponse::success(id, json!({ "ok": true })),
        Err(e) => agent_err_to_response(id, e),
    }
}

pub fn handle_rate_limit_status(
    core: &Core,
    _state: &mut AppState,
    _engine: &mut crate::engine_state::CoreState,
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
    match core.rate_limit_status(now_ms()) {
        Ok(all) => {
            let filtered: Vec<_> = all
                .into_iter()
                .filter(|r| agent.as_ref().is_none_or(|a| &r.agent == a))
                .filter(|r| metric.as_ref().is_none_or(|m| &r.metric == m))
                .collect();
            JsonRpcResponse::success(
                id,
                json!({ "total": filtered.len(), "rate_limits": filtered }),
            )
        }
        Err(e) => agent_err_to_response(id, e),
    }
}
