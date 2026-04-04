use serde_json::json;

use crate::ipc::protocol::JsonRpcResponse;
use crate::state::AppState;

pub fn handle_message_send(
    state: &mut AppState,
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
    let from = if let Some(f) = params.get("from_surface_id").and_then(|v| v.as_u64()) {
        f as u32
    } else {
        state.focused_surface_id().unwrap_or(0)
    };
    let msg_id = state.send_message(from, to, content);
    JsonRpcResponse::success(id, json!({ "id": msg_id }))
}

pub fn handle_message_read(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = params
        .get("surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .or_else(|| state.focused_surface_id())
        .unwrap_or(0);
    let from = params
        .get("from_surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);
    let peek = params
        .get("peek")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let messages = state.read_messages(surface_id, from, peek);
    let result: Vec<_> = messages
        .iter()
        .map(|m| json!({ "id": m.id, "from_surface_id": m.from_surface_id, "content": m.content }))
        .collect();
    JsonRpcResponse::success(id, json!(result))
}

pub fn handle_message_count(
    state: &AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = params
        .get("surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .or_else(|| state.focused_surface_id())
        .unwrap_or(0);
    let count = state.message_count(surface_id);
    JsonRpcResponse::success(id, json!({ "count": count }))
}

pub fn handle_message_clear(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = params
        .get("surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .or_else(|| state.focused_surface_id())
        .unwrap_or(0);
    state.clear_messages(surface_id);
    JsonRpcResponse::success(id, json!({ "cleared": true }))
}
