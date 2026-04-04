use serde_json::json;

use crate::ipc::protocol::JsonRpcResponse;
use crate::state::AppState;

pub fn handle_notification_list(state: &AppState, id: serde_json::Value) -> JsonRpcResponse {
    let notifications: Vec<_> = state
        .engine.notifications
        .all()
        .rev()
        .take(50)
        .map(|n| {
            json!({
                "id": n.id,
                "title": n.title,
                "body": n.body,
                "workspace_id": n.source_workspace,
                "surface_id": n.source_surface,
                "read": n.read,
            })
        })
        .collect();
    JsonRpcResponse::success(id, json!(notifications))
}

pub fn handle_notification_create(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let title = params
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("Notification")
        .to_string();
    let body = params
        .get("body")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let ws_id = state.active_workspace().id;
    state.engine.notifications.add(ws_id, 0, title, body);
    JsonRpcResponse::success(id, json!({ "created": true }))
}
