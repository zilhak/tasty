use serde_json::json;

use crate::ipc::protocol::JsonRpcResponse;
use crate::state::AppState;

pub fn handle_notification_list(state: &AppState, id: serde_json::Value) -> JsonRpcResponse {
    let notifications: Vec<_> = state
        .engine
        .notifications
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
    let surface_id = params
        .get("surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .unwrap_or(0);

    // workspace_id 결정 — CLAUDE.md "포커스 독립성" 원칙: IPC는 사용자 포커스에
    // 의존하지 않아야 한다. workspace_id를 명시하지 않으면 다음 순으로 결정.
    let ws_id = match params.get("workspace_id").and_then(|v| v.as_u64()) {
        Some(v) => v as u32,
        None => {
            // 1. surface_id가 주어지면 그 surface가 속한 워크스페이스로 라우팅
            if surface_id > 0 {
                if let Some((idx, _)) = state.find_workspace_index_for_surface(surface_id) {
                    state.engine.workspaces[idx].id
                } else {
                    return JsonRpcResponse::invalid_params(
                        id,
                        format!(
                            "surface_id {} not found; cannot determine workspace",
                            surface_id
                        ),
                    );
                }
            // 2. 워크스페이스가 정확히 1개면 자동 사용 (호환성 폴백, 1버전 유지 예정)
            } else if state.engine.workspaces.len() == 1 {
                tracing::warn!(
                    "notification.create called without 'workspace_id' or 'surface_id'; \
                     auto-routing to the only workspace. \
                     This fallback will be removed in a future version."
                );
                state.engine.workspaces[0].id
            } else {
                return JsonRpcResponse::invalid_params(
                    id,
                    "Missing required 'workspace_id' parameter \
                     (focus-independent IPC requires explicit workspace_id, or surface_id \
                     to route by ownership)",
                );
            }
        }
    };
    state.engine.notifications.add(ws_id, surface_id, title, body);
    JsonRpcResponse::success(id, json!({ "created": true }))
}
