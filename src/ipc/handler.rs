use serde_json::json;

use crate::hooks::HookEvent;
use crate::ipc::protocol::{JsonRpcRequest, JsonRpcResponse};
use crate::model::SplitDirection;
use crate::state::AppState;

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
        "surface.list" => handle_surface_list(state, id),
        "surface.send" => handle_surface_send(state, id, &request.params),
        "surface.send_key" => handle_surface_send_key(state, id, &request.params),
        "notification.list" => handle_notification_list(state, id),
        "notification.create" => handle_notification_create(state, id, &request.params),
        "tree" => handle_tree(state, id),
        "hook.set" => handle_hook_set(state, id, &request.params),
        "hook.list" => handle_hook_list(state, id, &request.params),
        "hook.unset" => handle_hook_unset(state, id, &request.params),
        "surface.set_mark" => handle_set_mark(state, id, &request.params),
        "surface.read_since_mark" => handle_read_since_mark(state, id, &request.params),
        "claude.launch" => handle_claude_launch(state, id, &request.params),
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
                // Set the name on the newly created workspace
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
        _ => SplitDirection::Vertical, // default to vertical
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

fn handle_surface_list(state: &AppState, id: serde_json::Value) -> JsonRpcResponse {
    let ws = state.active_workspace();
    let mut surfaces = Vec::new();
    for &pane_id in &ws.pane_layout().all_pane_ids() {
        if let Some(pane) = ws.pane_layout().find_pane(pane_id) {
            for (tab_idx, tab) in pane.tabs.iter().enumerate() {
                collect_surface_info(tab.panel(), pane_id, tab_idx, &mut surfaces);
            }
        }
    }
    JsonRpcResponse::success(id, json!(surfaces))
}

fn collect_surface_info(
    panel: &crate::model::Panel,
    pane_id: u32,
    tab_idx: usize,
    out: &mut Vec<serde_json::Value>,
) {
    match panel {
        crate::model::Panel::Terminal(node) => {
            out.push(json!({
                "id": node.id,
                "pane_id": pane_id,
                "tab_index": tab_idx,
                "cols": node.terminal.cols(),
                "rows": node.terminal.rows(),
            }));
        }
        crate::model::Panel::SurfaceGroup(group) => {
            collect_surface_layout_info(group.layout(), pane_id, tab_idx, out);
        }
    }
}

fn collect_surface_layout_info(
    layout: &crate::model::SurfaceGroupLayout,
    pane_id: u32,
    tab_idx: usize,
    out: &mut Vec<serde_json::Value>,
) {
    match layout {
        crate::model::SurfaceGroupLayout::Single(node) => {
            out.push(json!({
                "id": node.id,
                "pane_id": pane_id,
                "tab_index": tab_idx,
                "cols": node.terminal.cols(),
                "rows": node.terminal.rows(),
            }));
        }
        crate::model::SurfaceGroupLayout::Split { first, second, .. } => {
            collect_surface_layout_info(first, pane_id, tab_idx, out);
            collect_surface_layout_info(second, pane_id, tab_idx, out);
        }
    }
}

fn handle_surface_send(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let text = match params.get("text").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'text' parameter"),
    };
    // Send to the focused terminal by default
    if let Some(terminal) = state.focused_terminal_mut() {
        terminal.send_key(text);
    }
    JsonRpcResponse::success(id, json!({ "sent": true }))
}

fn handle_surface_send_key(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let key = match params.get("key").and_then(|v| v.as_str()) {
        Some(k) => k,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'key' parameter"),
    };
    // Map named keys to escape sequences
    let bytes: &[u8] = match key {
        "enter" => b"\r",
        "tab" => b"\t",
        "escape" | "esc" => b"\x1b",
        "backspace" => b"\x7f",
        "up" => b"\x1b[A",
        "down" => b"\x1b[B",
        "right" => b"\x1b[C",
        "left" => b"\x1b[D",
        "home" => b"\x1b[H",
        "end" => b"\x1b[F",
        "pageup" => b"\x1b[5~",
        "pagedown" => b"\x1b[6~",
        "delete" => b"\x1b[3~",
        "insert" => b"\x1b[2~",
        other => {
            // Send as-is (handles Ctrl sequences like "\x03" for Ctrl+C)
            if let Some(terminal) = state.focused_terminal_mut() {
                terminal.send_key(other);
            }
            return JsonRpcResponse::success(id, json!({ "sent": true }));
        }
    };
    if let Some(terminal) = state.focused_terminal_mut() {
        terminal.send_bytes(bytes);
    }
    JsonRpcResponse::success(id, json!({ "sent": true }))
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

// ---- Hook methods ----

fn handle_hook_set(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = params
        .get("surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .unwrap_or(0); // 0 means "focused" - we won't resolve here, hooks fire by surface ID

    let event_str = match params.get("event").and_then(|v| v.as_str()) {
        Some(e) => e,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'event' parameter"),
    };

    let event = match HookEvent::parse(event_str) {
        Some(e) => e,
        None => {
            return JsonRpcResponse::invalid_params(
                id,
                format!("Unknown event type: '{}'. Use: process-exit, bell, notification, output-match:PATTERN, idle-timeout:SECS", event_str),
            )
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

    let hook_id = state.hook_manager.add_hook(surface_id, event, command, once);
    JsonRpcResponse::success(id, json!({ "hook_id": hook_id }))
}

fn handle_hook_list(
    state: &AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = params
        .get("surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);

    let hooks: Vec<_> = state
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

fn handle_hook_unset(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let hook_id = match params.get("hook_id").and_then(|v| v.as_u64()) {
        Some(h) => h,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'hook_id' parameter"),
    };

    let removed = state.hook_manager.remove_hook(hook_id);
    JsonRpcResponse::success(id, json!({ "removed": removed }))
}

// ---- Read Mark methods ----

fn handle_set_mark(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = params
        .get("surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);

    state.set_mark(surface_id);
    JsonRpcResponse::success(id, json!({ "ok": true }))
}

fn handle_read_since_mark(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = params
        .get("surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);

    let strip_ansi = params
        .get("strip_ansi")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let text = state.read_since_mark(surface_id, strip_ansi);
    JsonRpcResponse::success(id, json!({ "text": text }))
}

// ---- Claude Launch ----

fn handle_claude_launch(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let workspace_name = params
        .get("workspace")
        .and_then(|v| v.as_str())
        .unwrap_or("claude");
    let directory = params.get("directory").and_then(|v| v.as_str());
    let task = params.get("task").and_then(|v| v.as_str());

    // Create new workspace
    match state.add_workspace() {
        Ok(_) => {}
        Err(e) => return JsonRpcResponse::internal_error(id, e.to_string()),
    }

    // Rename it
    let ws_idx = state.active_workspace;
    state.workspaces[ws_idx].name = workspace_name.to_string();

    // Send cd command if directory specified (shell-escape to prevent injection)
    if let Some(dir) = directory {
        if let Some(terminal) = state.focused_terminal_mut() {
            let escaped = shell_escape::escape(dir.into());
            terminal.send_key(&format!("cd {}\r", escaped));
        }
    }

    // Build and send claude command (shell-escape task parameter)
    let mut cmd = "claude".to_string();
    if let Some(t) = task {
        let escaped = shell_escape::escape(t.into());
        cmd.push_str(&format!(" --task {}", escaped));
    }
    if let Some(terminal) = state.focused_terminal_mut() {
        terminal.send_key(&format!("{}\r", cmd));
    }

    let ws_id = state.workspaces[ws_idx].id;
    JsonRpcResponse::success(
        id,
        json!({
            "workspace_id": ws_id,
            "workspace_name": workspace_name,
        }),
    )
}
