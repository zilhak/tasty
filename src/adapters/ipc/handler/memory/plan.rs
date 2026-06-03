//! `memory.plan.*` IPC handlers.

use serde_json::{Value, json};
use tasty_memory::plan as plan_mod;

use crate::core::Core;
use crate::state::AppState;
use tasty_ipc::caller::CallerContext;
use tasty_ipc::protocol::JsonRpcResponse;

use super::{map_error, require_str, require_workspace_id};

fn parse_plan_step(v: &Value, id: &Value) -> Result<plan_mod::PlanStep, JsonRpcResponse> {
    serde_json::from_value(v.clone())
        .map_err(|e| JsonRpcResponse::invalid_params(id.clone(), format!("invalid step JSON: {e}")))
}

fn parse_plan_step_state(s: &str, id: &Value) -> Result<plan_mod::PlanStepState, JsonRpcResponse> {
    let v = Value::String(s.to_string());
    serde_json::from_value(v).map_err(|_| {
        JsonRpcResponse::invalid_params(
            id.clone(),
            format!("invalid state '{s}' (expected pending|in_progress|completed|failed|skipped)"),
        )
    })
}

pub fn handle_plan_create(
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
    let plan_id = match require_str(params, "plan_id", &id) {
        Ok(s) => s.to_string(),
        Err(e) => return e,
    };
    let title = match require_str(params, "title", &id) {
        Ok(s) => s.to_string(),
        Err(e) => return e,
    };
    let steps = match params.get("steps") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(arr)) => {
            let mut out = Vec::with_capacity(arr.len());
            for v in arr {
                match parse_plan_step(v, &id) {
                    Ok(step) => out.push(step),
                    Err(e) => return e,
                }
            }
            out
        }
        Some(_) => {
            return JsonRpcResponse::invalid_params(id, "'steps' must be an array");
        }
    };
    let owner = caller.owner().to_string();
    match core
        .with_memory(|s| plan_mod::plan_create(s, &owner, workspace_id, &plan_id, &title, steps))
    {
        Ok(version) => JsonRpcResponse::success(id, json!({ "ok": true, "version": version })),
        Err(e) => map_error(id, e),
    }
}

pub fn handle_plan_get(
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
    let plan_id = match require_str(params, "plan_id", &id) {
        Ok(s) => s.to_string(),
        Err(e) => return e,
    };
    match core.with_memory(|s| plan_mod::plan_get(s, workspace_id, &plan_id)) {
        Ok(Some(plan)) => match serde_json::to_value(&plan) {
            Ok(v) => JsonRpcResponse::success(id, v),
            Err(e) => JsonRpcResponse::internal_error(id, format!("serialize: {e}")),
        },
        Ok(None) => JsonRpcResponse::success(id, Value::Null),
        Err(e) => map_error(id, e),
    }
}

pub fn handle_plan_list(
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
    match core.with_memory(|s| plan_mod::plan_list(s, workspace_id)) {
        Ok(plans) => JsonRpcResponse::success(id, json!({ "plans": plans, "count": plans.len() })),
        Err(e) => map_error(id, e),
    }
}

pub fn handle_plan_delete(
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
    let plan_id = match require_str(params, "plan_id", &id) {
        Ok(s) => s.to_string(),
        Err(e) => return e,
    };
    let owner = caller.owner().to_string();
    match core.with_memory(|s| plan_mod::plan_delete(s, &owner, workspace_id, &plan_id)) {
        Ok(()) => JsonRpcResponse::success(id, json!({ "ok": true })),
        Err(e) => map_error(id, e),
    }
}

pub fn handle_plan_add_step(
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
    let plan_id = match require_str(params, "plan_id", &id) {
        Ok(s) => s.to_string(),
        Err(e) => return e,
    };
    let step_value = match params.get("step") {
        Some(v) => v,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'step' object"),
    };
    let step = match parse_plan_step(step_value, &id) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let position = params
        .get("position")
        .and_then(|v| v.as_u64())
        .map(|n| n as usize);
    let cas = params.get("cas").and_then(|v| v.as_u64());
    let owner = caller.owner().to_string();
    match core.with_memory(|s| {
        plan_mod::plan_add_step(s, &owner, workspace_id, &plan_id, step, position, cas)
    }) {
        Ok(version) => JsonRpcResponse::success(id, json!({ "ok": true, "version": version })),
        Err(e) => map_error(id, e),
    }
}

pub fn handle_plan_remove_step(
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
    let plan_id = match require_str(params, "plan_id", &id) {
        Ok(s) => s.to_string(),
        Err(e) => return e,
    };
    let step_id = match require_str(params, "step_id", &id) {
        Ok(s) => s.to_string(),
        Err(e) => return e,
    };
    let cas = params.get("cas").and_then(|v| v.as_u64());
    let owner = caller.owner().to_string();
    match core.with_memory(|s| {
        plan_mod::plan_remove_step(s, &owner, workspace_id, &plan_id, &step_id, cas)
    }) {
        Ok(version) => JsonRpcResponse::success(id, json!({ "ok": true, "version": version })),
        Err(e) => map_error(id, e),
    }
}

pub fn handle_plan_update_step(
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
    let plan_id = match require_str(params, "plan_id", &id) {
        Ok(s) => s.to_string(),
        Err(e) => return e,
    };
    let step_id = match require_str(params, "step_id", &id) {
        Ok(s) => s.to_string(),
        Err(e) => return e,
    };
    let new_state = match params.get("state") {
        Some(Value::String(s)) => match parse_plan_step_state(s, &id) {
            Ok(st) => Some(st),
            Err(e) => return e,
        },
        Some(Value::Null) | None => None,
        Some(_) => {
            return JsonRpcResponse::invalid_params(id, "'state' must be a string");
        }
    };
    let clear_notes = params
        .get("clear_notes")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let notes_arg: Option<Option<String>> = if clear_notes {
        Some(None)
    } else {
        match params.get("notes") {
            Some(Value::String(s)) => Some(Some(s.clone())),
            Some(Value::Null) | None => None,
            Some(_) => {
                return JsonRpcResponse::invalid_params(id, "'notes' must be a string");
            }
        }
    };
    let cas = params.get("cas").and_then(|v| v.as_u64());
    let owner = caller.owner().to_string();
    match core.with_memory(|s| {
        plan_mod::plan_update_step(
            s,
            plan_mod::PlanUpdateStepOpts {
                owner: &owner,
                workspace_id,
                plan_id: &plan_id,
                step_id: &step_id,
                new_state,
                notes: notes_arg,
                cas,
            },
        )
    }) {
        Ok(version) => JsonRpcResponse::success(id, json!({ "ok": true, "version": version })),
        Err(e) => map_error(id, e),
    }
}

// ============================================================
// Cache `memory.cache_*` — Phase 7.3
// ============================================================
