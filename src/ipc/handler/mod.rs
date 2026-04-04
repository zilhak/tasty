use serde_json::json;

use crate::ipc::protocol::{JsonRpcRequest, JsonRpcResponse};
use crate::state::AppState;

mod claude;
mod hooks;
mod message;
mod meta;
mod notification;
mod pane;
mod surface;
mod tab;
mod workspace;

/// Handle a JSON-RPC request against the application state.
/// Returns a JSON-RPC response.
pub fn handle(state: &mut AppState, request: &JsonRpcRequest) -> JsonRpcResponse {
    let id = request.id.clone().unwrap_or(serde_json::Value::Null);

    match request.method.as_str() {
        "system.info" => handle_system_info(state, id),
        "workspace.list" => workspace::handle_workspace_list(state, id),
        "workspace.create" => workspace::handle_workspace_create(state, id, &request.params),
        "workspace.update" => workspace::handle_workspace_update(state, id, &request.params),
        "workspace.select" => workspace::handle_workspace_select(state, id, &request.params),
        "pane.list" => pane::handle_pane_list(state, id),
        "split" => pane::handle_split(state, id, &request.params),
        "tab.list" => tab::handle_tab_list(state, id),
        "tab.create" => tab::handle_tab_create(state, id, &request.params),
        "tab.close" => tab::handle_tab_close(state, id),
        "pane.close" => pane::handle_pane_close(state, id),
        "surface.close" => surface::handle_surface_close(state, id),
        "surface.list" => surface::handle_surface_list(state, id),
        "surface.send" => surface::handle_surface_send(state, id, &request.params),
        "surface.send_key" => surface::handle_surface_send_key(state, id, &request.params),
        "surface.send_combo" => surface::handle_surface_send_combo(state, id, &request.params),
        "surface.send_to" => surface::handle_surface_send_to(state, id, &request.params),
        "surface.focus" => surface::handle_surface_focus(state, id, &request.params),
        "pane.focus" => surface::handle_pane_focus(state, id, &request.params),
        "notification.list" => notification::handle_notification_list(state, id),
        "notification.create" => notification::handle_notification_create(state, id, &request.params),
        "tree" => handle_tree(state, id),
        "hook.set" => hooks::handle_hook_set(state, id, &request.params),
        "hook.list" => hooks::handle_hook_list(state, id, &request.params),
        "hook.unset" => hooks::handle_hook_unset(state, id, &request.params),
        "surface.set_mark" => surface::handle_set_mark(state, id, &request.params),
        "surface.read_since_mark" => surface::handle_read_since_mark(state, id, &request.params),
        "surface.screen_text" => surface::handle_screen_text(state, id, &request.params),
        "surface.cursor_position" => surface::handle_cursor_position(state, id, &request.params),
        "surface.is_typing" => handle_is_typing(state, id, &request.params),
        "surface.send_wait_idle" => handle_send_wait_idle(state, id, &request.params),
        "claude.launch" => claude::handle_claude_launch(state, id, &request.params),
        "claude.spawn" => claude::handle_claude_spawn(state, id, &request.params),
        "claude.children" => claude::handle_claude_children(state, id, &request.params),
        "claude.parent" => claude::handle_claude_parent(state, id, &request.params),
        "claude.kill" => claude::handle_claude_kill(state, id, &request.params),
        "claude.respawn" => claude::handle_claude_respawn(state, id, &request.params),
        "claude.set_idle_state" => claude::handle_claude_set_idle_state(state, id, &request.params),
        "claude.set_needs_input" => claude::handle_claude_set_needs_input(state, id, &request.params),
        "claude.broadcast" => claude::handle_claude_broadcast(state, id, &request.params),
        "claude.wait" => claude::handle_claude_wait(state, id, &request.params),
        "surface.fire_hook" => hooks::handle_surface_fire_hook(state, id, &request.params),
        "global_hook.set" => hooks::handle_global_hook_set(state, id, &request.params),
        "global_hook.list" => hooks::handle_global_hook_list(state, id),
        "global_hook.unset" => hooks::handle_global_hook_unset(state, id, &request.params),
        "surface.meta_set" => meta::handle_surface_meta_set(state, id, &request.params),
        "surface.meta_get" => meta::handle_surface_meta_get(state, id, &request.params),
        "surface.meta_unset" => meta::handle_surface_meta_unset(state, id, &request.params),
        "surface.meta_list" => meta::handle_surface_meta_list(state, id, &request.params),
        "focus.direction" => pane::handle_focus_direction(state, id, &request.params),
        "tab.open_markdown" => tab::handle_open_markdown(state, id, &request.params),
        "tab.open_explorer" => tab::handle_open_explorer(state, id, &request.params),
        "ui.state" => handle_ui_state(state, id),
        "message.send" => message::handle_message_send(state, id, &request.params),
        "message.read" => message::handle_message_read(state, id, &request.params),
        "message.count" => message::handle_message_count(state, id, &request.params),
        "message.clear" => message::handle_message_clear(state, id, &request.params),
        _ => JsonRpcResponse::method_not_found(id, &request.method),
    }
}

/// Apply metadata key-value pairs to a surface.
fn apply_meta(surface_id: u32, meta: Option<&serde_json::Map<String, serde_json::Value>>) {
    if let Some(map) = meta {
        for (key, value) in map {
            if let Some(v) = value.as_str() {
                crate::surface_meta::SurfaceMetaStore::set(surface_id, key, v);
            }
        }
    }
}

/// Resolve a target parameter to a numeric ID.
fn resolve_target_param(value: Option<&serde_json::Value>, level: &str) -> Option<u32> {
    let val = value?;
    if let Some(n) = val.as_u64() {
        return Some(n as u32);
    }
    if let Some(s) = val.as_str() {
        if s.is_empty() {
            return None;
        }
        if let Ok(n) = s.parse::<u32>() {
            return Some(n);
        }
        if level == "surface" {
            return crate::surface_meta::SurfaceMetaStore::find_by_value("nickname", s);
        }
    }
    None
}

fn handle_system_info(state: &AppState, id: serde_json::Value) -> JsonRpcResponse {
    JsonRpcResponse::success(
        id,
        json!({
            "version": env!("CARGO_PKG_VERSION"),
            "workspace_count": state.engine.workspaces.len(),
            "active_workspace": state.active_workspace,
        }),
    )
}

fn handle_ui_state(state: &AppState, id: serde_json::Value) -> JsonRpcResponse {
    let ws = state.active_workspace();
    let pane_count = ws.pane_layout().all_pane_ids().len();
    let tab_count = state.focused_pane().map(|p| p.tabs.len()).unwrap_or(0);
    JsonRpcResponse::success(
        id,
        json!({
            "settings_open": state.settings_open,
            "notification_panel_open": state.notification_panel_open,
            "active_workspace": state.active_workspace,
            "workspace_count": state.engine.workspaces.len(),
            "pane_count": pane_count,
            "tab_count": tab_count,
        }),
    )
}

fn handle_tree(state: &AppState, id: serde_json::Value) -> JsonRpcResponse {
    let mut tree = Vec::new();
    for (i, ws) in state.engine.workspaces.iter().enumerate() {
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

fn handle_is_typing(
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
    let typing = state.is_typing(surface_id);
    let idle_seconds = if let Some(last) = state.engine.last_key_input.get(&surface_id) {
        last.elapsed().as_secs_f64()
    } else {
        f64::MAX
    };
    let idle_seconds_capped = if idle_seconds == f64::MAX { -1.0 } else { idle_seconds };
    JsonRpcResponse::success(
        id,
        json!({
            "typing": typing,
            "idle_seconds": idle_seconds_capped,
        }),
    )
}

fn handle_send_wait_idle(
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
    let text = match params.get("text").and_then(|v| v.as_str()) {
        Some(t) => t.to_string(),
        None => return JsonRpcResponse::invalid_params(id, "Missing 'text' parameter"),
    };
    if state.is_typing(surface_id) {
        return JsonRpcResponse::success(
            id,
            json!({ "sent": false, "reason": "typing" }),
        );
    }
    if let Some(terminal) = state.find_terminal_by_id_mut(surface_id) {
        terminal.send_key(&text);
        JsonRpcResponse::success(id, json!({ "sent": true }))
    } else {
        JsonRpcResponse::invalid_params(
            id,
            format!("Surface {} not found", surface_id),
        )
    }
}
