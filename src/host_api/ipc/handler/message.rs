use serde_json::json;

use crate::ipc::protocol::JsonRpcResponse;
use crate::state::AppState;

use super::require_surface_id;

pub fn handle_message_send(
    _state: &mut AppState,
    engine: &mut crate::engine_state::EngineState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let to = match params.get("to_surface_id").and_then(|v| v.as_u64()) {
        Some(v) => v as u32,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'to_surface_id'"),
    };
    let content = match params.get("content").and_then(|v| v.as_str()) {
        Some(v) => v.to_string(),
        None => return JsonRpcResponse::invalid_params(id, "Missing 'content'"),
    };
    let from = match params.get("from_surface_id").and_then(|v| v.as_u64()) {
        Some(f) => f as u32,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'from_surface_id'"),
    };
    let msg_id = engine.send_message(from, to, content);
    JsonRpcResponse::success(
        id,
        json!({ "id": msg_id, "from_surface_id": from, "to_surface_id": to }),
    )
}

pub fn handle_message_read(
    _state: &mut AppState,
    engine: &mut crate::engine_state::EngineState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    let from = params
        .get("from_surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);
    let peek = params
        .get("peek")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let messages = engine.read_messages(surface_id, from, peek);
    let result: Vec<_> = messages
        .iter()
        .map(|m| json!({ "id": m.id, "from_surface_id": m.from_surface_id, "content": m.content }))
        .collect();
    JsonRpcResponse::success(id, json!({ "surface_id": surface_id, "messages": result }))
}

pub fn handle_message_count(
    _state: &AppState,
    engine: &crate::engine_state::EngineState,
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
    _state: &mut AppState,
    engine: &mut crate::engine_state::EngineState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    engine.clear_messages(surface_id);
    JsonRpcResponse::success(id, json!({ "cleared": true, "surface_id": surface_id }))
}
