use serde_json::{Value, json};

use crate::adapters::ipc::handler::params::{self, p_try};
use crate::app::services::AppServices;
use tasty_ipc::caller::CallerContext;
use tasty_ipc::protocol::JsonRpcResponse;

use super::super::memory::mark_durability;
use super::{now_ms, reply_agent};

fn serialize<T: serde::Serialize>(id: Value, value: T) -> JsonRpcResponse {
    match serde_json::to_value(value) {
        Ok(v) => JsonRpcResponse::success(id, v),
        Err(e) => JsonRpcResponse::error(id, -32603, format!("serialize: {e}")),
    }
}

pub fn handle_rate_limit_set(
    core: &AppServices,
    _engine: &crate::core::CoreState,
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
    let limit = match p_try!(params::opt_int::<u64>(params, "limit", &id)) {
        Some(c) if c >= 1 && c <= u32::MAX as u64 => c as u32,
        _ => {
            return JsonRpcResponse::invalid_params(
                id,
                "Missing or invalid 'limit' (must be u32 >= 1)",
            );
        }
    };
    let per_ms = match p_try!(params::opt_int::<u64>(params, "per_ms", &id)) {
        Some(c) if c >= 1 => c,
        _ => {
            return JsonRpcResponse::invalid_params(
                id,
                "Missing or invalid 'per_ms' (must be u64 >= 1)",
            );
        }
    };
    let burst = match p_try!(params::opt_int::<u64>(params, "burst", &id)) {
        Some(c) if c <= u32::MAX as u64 => Some(c as u32),
        Some(_) => {
            return JsonRpcResponse::invalid_params(id, "'burst' exceeds u32::MAX");
        }
        None => None,
    };
    reply_agent(
        core,
        id,
        params,
        move |mem| {
            crate::app::services::ratelimit::set(
                mem,
                agent.clone(),
                metric.clone(),
                limit,
                per_ms,
                burst,
                now_ms(),
            )
        },
        move |core, id, rl| mark_durability(core, serialize(id, rl)),
    )
}

pub fn handle_rate_limit_list(
    core: &AppServices,
    _engine: &crate::core::CoreState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    reply_agent(
        core,
        id,
        params,
        move |mem| crate::app::services::ratelimit::status(mem, now_ms()),
        move |_, id, all| {
            JsonRpcResponse::success(id, json!({ "total": all.len(), "rate_limits": all }))
        },
    )
}

pub fn handle_rate_limit_remove(
    core: &AppServices,
    _engine: &crate::core::CoreState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let rl_id = match params.get("id").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return JsonRpcResponse::invalid_params(id, "Missing or empty 'id'"),
    };
    reply_agent(
        core,
        id,
        params,
        move |mem| crate::app::services::ratelimit::remove(mem, &rl_id),
        move |core, id, ()| {
            mark_durability(core, JsonRpcResponse::success(id, json!({ "ok": true })))
        },
    )
}

pub fn handle_rate_limit_status(
    core: &AppServices,
    _engine: &crate::core::CoreState,
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
    reply_agent(
        core,
        id,
        params,
        move |mem| crate::app::services::ratelimit::status(mem, now_ms()),
        move |_, id, all| {
            let filtered: Vec<_> = all
                .into_iter()
                .filter(|r| agent.as_ref().is_none_or(|a| &r.agent == a))
                .filter(|r| metric.as_ref().is_none_or(|m| &r.metric == m))
                .collect();
            JsonRpcResponse::success(
                id,
                json!({ "total": filtered.len(), "rate_limits": filtered }),
            )
        },
    )
}
