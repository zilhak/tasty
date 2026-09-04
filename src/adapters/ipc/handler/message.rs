use super::params::require_u32;
use serde_json::json;

use crate::state::AppState;
use tasty_ipc::protocol::JsonRpcResponse;

use super::require_surface_id;

pub fn handle_message_send(
    core: &mut crate::core::Core,
    _state: &mut AppState,
    engine: &mut crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let to = match require_u32(params, "to_surface_id", &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let content = match params.get("content").and_then(|v| v.as_str()) {
        Some(v) => v.to_string(),
        None => return JsonRpcResponse::invalid_params(id, "Missing 'content'"),
    };
    let from = match require_u32(params, "from_surface_id", &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let msg_id = core.send_surface_message(engine, from, to, content);
    JsonRpcResponse::success(
        id,
        json!({ "id": msg_id, "from_surface_id": from, "to_surface_id": to }),
    )
}

pub fn handle_message_read(
    core: &mut crate::core::Core,
    _state: &mut AppState,
    engine: &mut crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    let from = match super::params::optional_u32(params, "from_surface_id", &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let peek = params
        .get("peek")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let messages = core.read_surface_messages(engine, surface_id, from, peek);
    let result: Vec<_> = messages
        .iter()
        .map(|m| json!({ "id": m.id, "from_surface_id": m.from_surface_id, "content": m.content }))
        .collect();
    JsonRpcResponse::success(id, json!({ "surface_id": surface_id, "messages": result }))
}

pub fn handle_message_count(
    _state: &AppState,
    engine: &crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    let count = engine.message_count(surface_id);
    JsonRpcResponse::success(id, json!({ "count": count, "surface_id": surface_id }))
}

pub fn handle_message_clear(
    core: &mut crate::core::Core,
    _state: &mut AppState,
    engine: &mut crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    core.clear_surface_messages(engine, surface_id);
    JsonRpcResponse::success(id, json!({ "cleared": true, "surface_id": surface_id }))
}
