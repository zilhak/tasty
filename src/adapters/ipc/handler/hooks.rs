use serde_json::json;
use tasty_hooks::HookEvent;

use crate::global_hooks::HookCondition;
use crate::ipc::protocol::JsonRpcResponse;
use crate::state::AppState;

pub(crate) fn handle_hook_set(
    _state: &mut AppState,
    engine: &mut crate::engine_state::EngineState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match super::require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };

    let event_str = match params.get("event").and_then(|v| v.as_str()) {
        Some(e) => e,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'event' parameter"),
    };

    let event = match HookEvent::parse(event_str) {
        Some(e) => e,
        None => {
            return JsonRpcResponse::invalid_params(
                id,
                format!(
                    "Unknown event type: '{}'. Use: process-exit, bell, notification, output-match:PATTERN, idle-timeout:SECS, claude-idle, needs-input, claude-error",
                    event_str
                ),
            );
        }
    };

    let command = match params.get("command").and_then(|v| v.as_str()) {
        Some(c) => c.to_string(),
        None => return JsonRpcResponse::invalid_params(id, "Missing 'command' parameter"),
    };

    let once = params
        .get("once")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let hook_id = engine
        .hook_manager
        .add_hook(surface_id, event, command, once);
    JsonRpcResponse::success(id, json!({ "hook_id": hook_id }))
}

pub(crate) fn handle_hook_list(
    _state: &AppState,
    engine: &crate::engine_state::EngineState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = params
        .get("surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);

    let hooks: Vec<_> = engine
        .hook_manager
        .list_hooks(surface_id)
        .iter()
        .map(|h| {
            json!({
                "id": h.id,
                "surface_id": h.surface_id,
                "event": h.event.to_display_string(),
                "command": h.command,
                "once": h.once,
            })
        })
        .collect();

    JsonRpcResponse::success(id, json!(hooks))
}

pub(crate) fn handle_hook_unset(
    _state: &mut AppState,
    engine: &mut crate::engine_state::EngineState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let hook_id = match params.get("hook_id").and_then(|v| v.as_u64()) {
        Some(h) => h,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'hook_id' parameter"),
    };

    let removed = engine.hook_manager.remove_hook(hook_id);
    JsonRpcResponse::success(id, json!({ "removed": removed }))
}

pub(crate) fn handle_global_hook_set(
    _state: &mut AppState,
    engine: &mut crate::engine_state::EngineState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let condition_str = match params.get("condition").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'condition' parameter"),
    };

    let condition = match HookCondition::parse(condition_str) {
        Some(c) => c,
        None => {
            return JsonRpcResponse::invalid_params(
                id,
                format!(
                    "Invalid condition '{}'. Use: interval:SECS, once:SECS",
                    condition_str
                ),
            );
        }
    };

    let command = match params.get("command").and_then(|v| v.as_str()) {
        Some(c) => c.to_string(),
        None => return JsonRpcResponse::invalid_params(id, "Missing 'command' parameter"),
    };

    let label = params
        .get("label")
        .and_then(|v| v.as_str())
        .map(String::from);

    let hook_id = engine.global_hook_manager.add(condition, command, label);
    JsonRpcResponse::success(id, json!({ "hook_id": hook_id }))
}

pub(crate) fn handle_global_hook_list(
    _state: &AppState,
    engine: &crate::engine_state::EngineState,
    id: serde_json::Value,
) -> JsonRpcResponse {
    let hooks: Vec<_> = engine
        .global_hook_manager
        .list()
        .iter()
        .map(|h| {
            json!({
                "id": h.id,
                "condition": h.condition.to_display_string(),
                "command": h.command,
                "label": h.label,
            })
        })
        .collect();
    JsonRpcResponse::success(id, json!(hooks))
}

pub(crate) fn handle_global_hook_unset(
    _state: &mut AppState,
    engine: &mut crate::engine_state::EngineState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let hook_id = match params.get("hook_id").and_then(|v| v.as_u64()) {
        Some(h) => h as u32,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'hook_id' parameter"),
    };

    let removed = engine.global_hook_manager.remove(hook_id);
    JsonRpcResponse::success(id, json!({ "removed": removed }))
}

pub(crate) fn handle_surface_fire_hook(
    state: &mut AppState,
    engine: &mut crate::engine_state::EngineState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match super::require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };

    let event_str = match params.get("event").and_then(|v| v.as_str()) {
        Some(e) => e,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'event' parameter"),
    };

    let event = match HookEvent::parse(event_str) {
        Some(e) => e,
        None => {
            return JsonRpcResponse::invalid_params(
                id,
                format!("Unknown event type: '{}'", event_str),
            );
        }
    };

    let fired = engine
        .hook_manager
        .check_and_fire(surface_id, &[event.clone()]);
    let event_kind = event.to_display_string();
    for hook_id in &fired {
        state.enqueue_host_event(crate::state::PendingHostEvent::HookFired {
            hook_id: *hook_id,
            event_kind: event_kind.clone(),
            surface_id,
        });
    }
    JsonRpcResponse::success(id, json!({ "fired": fired.len() }))
}
