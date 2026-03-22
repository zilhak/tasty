use serde_json::json;

use crate::ipc::protocol::{JsonRpcRequest, JsonRpcResponse};
use crate::model::SplitDirection;
use crate::state::AppState;

mod hooks;
mod surface;

/// Handle a JSON-RPC request against the application state.
/// Returns a JSON-RPC response.
pub fn handle(state: &mut AppState, request: &JsonRpcRequest) -> JsonRpcResponse {
    let id = request.id.clone().unwrap_or(serde_json::Value::Null);

    match request.method.as_str() {
        "system.info" => handle_system_info(state, id),
        "workspace.list" => handle_workspace_list(state, id),
        "workspace.create" => handle_workspace_create(state, id, &request.params),
        "workspace.select" => handle_workspace_select(state, id, &request.params),
        "pane.list" => handle_pane_list(state, id),
        "pane.split" => handle_pane_split(state, id, &request.params),
        "tab.list" => handle_tab_list(state, id),
        "tab.create" => handle_tab_create(state, id),
        "tab.close" => handle_tab_close(state, id),
        "pane.close" => handle_pane_close(state, id),
        "surface.close" => surface::handle_surface_close(state, id),
        "surface.list" => surface::handle_surface_list(state, id),
        "surface.send" => surface::handle_surface_send(state, id, &request.params),
        "surface.send_key" => surface::handle_surface_send_key(state, id, &request.params),
        "notification.list" => handle_notification_list(state, id),
        "notification.create" => handle_notification_create(state, id, &request.params),
        "tree" => handle_tree(state, id),
        "hook.set" => hooks::handle_hook_set(state, id, &request.params),
        "hook.list" => hooks::handle_hook_list(state, id, &request.params),
        "hook.unset" => hooks::handle_hook_unset(state, id, &request.params),
        "surface.set_mark" => surface::handle_set_mark(state, id, &request.params),
        "surface.read_since_mark" => surface::handle_read_since_mark(state, id, &request.params),
        "claude.launch" => hooks::handle_claude_launch(state, id, &request.params),
        _ => JsonRpcResponse::method_not_found(id, &request.method),
    }
}

fn handle_system_info(state: &AppState, id: serde_json::Value) -> JsonRpcResponse {
    JsonRpcResponse::success(
        id,
        json!({
            "version": env!("CARGO_PKG_VERSION"),
            "workspace_count": state.workspaces.len(),
            "active_workspace": state.active_workspace,
        }),
    )
}

fn handle_workspace_list(state: &AppState, id: serde_json::Value) -> JsonRpcResponse {
    let workspaces: Vec<_> = state
        .workspaces
        .iter()
        .enumerate()
        .map(|(i, ws)| {
            json!({
                "id": ws.id,
                "name": ws.name,
                "active": i == state.active_workspace,
                "pane_count": ws.pane_layout().all_pane_ids().len(),
            })
        })
        .collect();
    JsonRpcResponse::success(id, json!(workspaces))
}

fn handle_workspace_create(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    match state.add_workspace() {
        Ok(_) => {
            let name = params
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if !name.is_empty() {
                let idx = state.active_workspace;
                state.workspaces[idx].name = name.to_string();
            }
            let ws = state.active_workspace();
            JsonRpcResponse::success(
                id,
                json!({
                    "id": ws.id,
                    "name": ws.name,
                    "index": state.active_workspace,
                }),
            )
        }
        Err(e) => JsonRpcResponse::internal_error(id, e.to_string()),
    }
}

fn handle_workspace_select(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let index = params
        .get("index")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize);
    match index {
        Some(idx) if idx < state.workspaces.len() => {
            state.switch_workspace(idx);
            JsonRpcResponse::success(id, json!({ "active_workspace": idx }))
        }
        Some(idx) => JsonRpcResponse::invalid_params(
            id,
            format!("Workspace index {} out of range (0..{})", idx, state.workspaces.len()),
        ),
        None => JsonRpcResponse::invalid_params(id, "Missing 'index' parameter"),
    }
}

fn handle_pane_list(state: &AppState, id: serde_json::Value) -> JsonRpcResponse {
    let ws = state.active_workspace();
    let pane_ids = ws.pane_layout().all_pane_ids();
    let focused = ws.focused_pane;

    let panes: Vec<_> = pane_ids
        .iter()
        .map(|&pid| {
            let tab_count = ws
                .pane_layout()
                .find_pane(pid)
                .map(|p| p.tabs.len())
                .unwrap_or(0);
            json!({
                "id": pid,
                "focused": pid == focused,
                "tab_count": tab_count,
            })
        })
        .collect();
    JsonRpcResponse::success(id, json!(panes))
}

fn handle_pane_split(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let direction = match params.get("direction").and_then(|v| v.as_str()) {
        Some("horizontal") | Some("h") => SplitDirection::Horizontal,
        _ => SplitDirection::Vertical,
    };
    match state.split_pane(direction) {
        Ok(_) => {
            let ws = state.active_workspace();
            JsonRpcResponse::success(
                id,
                json!({
                    "focused_pane": ws.focused_pane,
                    "pane_count": ws.pane_layout().all_pane_ids().len(),
                }),
            )
        }
        Err(e) => JsonRpcResponse::internal_error(id, e.to_string()),
    }
}

fn handle_tab_list(state: &AppState, id: serde_json::Value) -> JsonRpcResponse {
    let tabs: Vec<_> = if let Some(pane) = state.focused_pane() {
        pane.tabs
            .iter()
            .enumerate()
            .map(|(i, tab)| {
                json!({
                    "id": tab.id,
                    "name": tab.name,
                    "active": i == pane.active_tab,
                })
            })
            .collect()
    } else {
        vec![]
    };
    JsonRpcResponse::success(id, json!(tabs))
}

fn handle_tab_create(state: &mut AppState, id: serde_json::Value) -> JsonRpcResponse {
    match state.add_tab() {
        Ok(_) => {
            let (tab_count, active_tab) = state
                .focused_pane()
                .map(|p| (p.tabs.len(), p.active_tab))
                .unwrap_or((0, 0));
            JsonRpcResponse::success(
                id,
                json!({
                    "tab_count": tab_count,
                    "active_tab": active_tab,
                }),
            )
        }
        Err(e) => JsonRpcResponse::internal_error(id, e.to_string()),
    }
}

fn handle_tab_close(state: &mut AppState, id: serde_json::Value) -> JsonRpcResponse {
    if state.close_active_tab() {
        JsonRpcResponse::success(id, json!({ "closed": true }))
    } else {
        JsonRpcResponse::success(id, json!({ "closed": false, "reason": "cannot close the last tab" }))
    }
}

fn handle_pane_close(state: &mut AppState, id: serde_json::Value) -> JsonRpcResponse {
    if state.close_active_pane() {
        JsonRpcResponse::success(id, json!({ "closed": true }))
    } else {
        JsonRpcResponse::success(id, json!({ "closed": false, "reason": "cannot close the last pane" }))
    }
}

fn handle_notification_list(state: &AppState, id: serde_json::Value) -> JsonRpcResponse {
    let notifications: Vec<_> = state
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

fn handle_notification_create(
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
    state.notifications.add(ws_id, 0, title, body);
    JsonRpcResponse::success(id, json!({ "created": true }))
}

fn handle_tree(state: &AppState, id: serde_json::Value) -> JsonRpcResponse {
    let mut tree = Vec::new();
    for (i, ws) in state.workspaces.iter().enumerate() {
        let active = i == state.active_workspace;
        let pane_ids = ws.pane_layout().all_pane_ids();
        let mut panes_info = Vec::new();

        for &pid in &pane_ids {
            if let Some(pane) = ws.pane_layout().find_pane(pid) {
                let tabs: Vec<_> = pane
                    .tabs
                    .iter()
                    .enumerate()
                    .map(|(ti, tab)| {
                        json!({
                            "id": tab.id,
                            "name": tab.name,
                            "active": ti == pane.active_tab,
                        })
                    })
                    .collect();
                panes_info.push(json!({
                    "id": pid,
                    "focused": pid == ws.focused_pane,
                    "tabs": tabs,
                }));
            }
        }

        tree.push(json!({
            "id": ws.id,
            "name": ws.name,
            "active": active,
            "panes": panes_info,
        }));
    }
    JsonRpcResponse::success(id, json!(tree))
}
