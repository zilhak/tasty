use serde_json::json;

use crate::ipc::protocol::{JsonRpcRequest, JsonRpcResponse};
use crate::state::AppState;

mod claude;
mod clipboard;
mod hooks;
pub mod ime;
#[cfg(target_os = "macos")]
mod input_source;
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
        "workspace.move" => workspace::handle_workspace_move(state, id, &request.params),
        // workspace.select removed: focus is user-only (shortcuts/clicks).
        "pane.list" => pane::handle_pane_list(state, id),
        "split" => pane::handle_split(state, id, &request.params),
        "tab.list" => tab::handle_tab_list(state, id, &request.params),
        "tab.create" => tab::handle_tab_create(state, id, &request.params),
        "tab.close" => tab::handle_tab_close(state, id, &request.params),
        "tab.move" => tab::handle_tab_move(state, id, &request.params),
        "pane.close" => pane::handle_pane_close(state, id, &request.params),
        "surface.close" => surface::handle_surface_close(state, id, &request.params),
        "surface.close_self" => surface::handle_surface_close_self(state, id, &request.params),
        "surface.list" => surface::handle_surface_list(state, id),
        "surface.send" => surface::handle_surface_send(state, id, &request.params),
        "surface.send_key" => surface::handle_surface_send_key(state, id, &request.params),
        "surface.send_combo" => surface::handle_surface_send_combo(state, id, &request.params),
        "surface.send_to" => surface::handle_surface_send_to(state, id, &request.params),
        // surface.focus / pane.focus removed: focus is user-only (shortcuts/clicks).
        "notification.list" => notification::handle_notification_list(state, id),
        "notification.create" => {
            notification::handle_notification_create(state, id, &request.params)
        }
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
        "claude.set_needs_input" => {
            claude::handle_claude_set_needs_input(state, id, &request.params)
        }
        "claude.broadcast" => claude::handle_claude_broadcast(state, id, &request.params),
        "claude.tell" => claude::handle_claude_tell(state, id, &request.params),
        "claude.wait" => claude::handle_claude_wait(state, id, &request.params),
        "surface.fire_hook" => hooks::handle_surface_fire_hook(state, id, &request.params),
        "global_hook.set" => hooks::handle_global_hook_set(state, id, &request.params),
        "global_hook.list" => hooks::handle_global_hook_list(state, id),
        "global_hook.unset" => hooks::handle_global_hook_unset(state, id, &request.params),
        "surface.meta_set" => meta::handle_surface_meta_set(state, id, &request.params),
        "surface.meta_get" => meta::handle_surface_meta_get(state, id, &request.params),
        "surface.meta_unset" => meta::handle_surface_meta_unset(state, id, &request.params),
        "surface.meta_list" => meta::handle_surface_meta_list(state, id, &request.params),
        // focus.direction removed: focus is user-only (shortcuts/clicks).
        // tab.open_markdown / tab.open_explorer removed: use tab.create with type parameter
        #[cfg(debug_assertions)]
        "ui.state" => handle_ui_state(state, id),
        #[cfg(debug_assertions)]
        "debug.cell_info" => handle_debug_cell_info(state, id, &request.params),
        #[cfg(debug_assertions)]
        "debug.screen_attrs" => handle_debug_screen_attrs(state, id, &request.params),
        #[cfg(debug_assertions)]
        "debug.inject_mouse" => handle_debug_inject_mouse(state, id, &request.params),
        #[cfg(debug_assertions)]
        "debug.inject_key" => handle_debug_inject_key(state, id, &request.params),
        "message.send" => message::handle_message_send(state, id, &request.params),
        "message.read" => message::handle_message_read(state, id, &request.params),
        "message.count" => message::handle_message_count(state, id, &request.params),
        "message.clear" => message::handle_message_clear(state, id, &request.params),
        #[cfg(target_os = "macos")]
        "surface.switch_input_source" => {
            input_source::handle_switch_input_source(id, &request.params)
        }
        #[cfg(target_os = "macos")]
        "surface.raw_key" => input_source::handle_raw_key(id, &request.params),
        "tool.clipboard.list" => clipboard::handle_list(state, id, &request.params),
        "tool.clipboard.get" => clipboard::handle_get(state, id, &request.params),
        "tool.clipboard.paste" => clipboard::handle_paste(state, id, &request.params),
        "tool.clipboard.remove" => clipboard::handle_remove(state, id, &request.params),
        "tool.clipboard.clear" => clipboard::handle_clear(state, id),
        "tool.clipboard.viewer_open" => clipboard::handle_viewer_open(state, id),
        _ => JsonRpcResponse::method_not_found(id, &request.method),
    }
}

/// Extract a required surface_id from params. Returns Err(JsonRpcResponse) if missing.
fn require_surface_id(
    params: &serde_json::Value,
    id: &serde_json::Value,
) -> Result<u32, JsonRpcResponse> {
    params
        .get("surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .ok_or_else(|| {
            JsonRpcResponse::invalid_params(id.clone(), "Missing required 'surface_id' parameter")
        })
}

/// Extract a required pane_id from params. Returns Err(JsonRpcResponse) if missing.
fn require_pane_id(
    params: &serde_json::Value,
    id: &serde_json::Value,
) -> Result<u32, JsonRpcResponse> {
    params
        .get("pane_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .ok_or_else(|| {
            JsonRpcResponse::invalid_params(id.clone(), "Missing required 'pane_id' parameter")
        })
}

/// Extract optional caller_surface_id from params.
fn caller_surface_id(params: &serde_json::Value) -> Option<u32> {
    params
        .get("caller_surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
}

/// Check if a surface belongs to a pane (directly or in any tab).
fn surface_belongs_to_pane(state: &AppState, surface_id: u32, pane_id: u32) -> bool {
    state.find_pane_for_surface(surface_id) == Some(pane_id)
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

#[cfg(debug_assertions)]
fn handle_ui_state(state: &AppState, id: serde_json::Value) -> JsonRpcResponse {
    let ws = state.active_workspace();
    let pane_count = ws.pane_layout().all_pane_ids().len();
    let focused_pane_id = ws.focused_pane;
    let tab_count = ws
        .pane_layout()
        .find_pane(focused_pane_id)
        .map(|p| p.tabs.len())
        .unwrap_or(0);
    JsonRpcResponse::success(
        id,
        json!({
            "settings_open": state.settings_open,
            "notification_panel_open": state.popups.is_open("notifications"),
            "active_workspace": state.active_workspace,
            "workspace_count": state.engine.workspaces.len(),
            "pane_count": pane_count,
            "tab_count": tab_count,
        }),
    )
}

#[cfg(debug_assertions)]
fn handle_debug_cell_info(
    state: &AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    let row = match params.get("row").and_then(|v| v.as_u64()) {
        Some(r) => r as usize,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'row' parameter"),
    };
    let col = match params.get("col").and_then(|v| v.as_u64()) {
        Some(c) => c as usize,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'col' parameter"),
    };
    if let Some(terminal) = state.find_terminal_by_id(surface_id) {
        if let Some(info) = terminal.cell_info(row, col) {
            JsonRpcResponse::success(
                id,
                json!({
                    "text": info.text,
                    "fg": info.fg,
                    "bg": info.bg,
                    "bold": info.bold,
                    "italic": info.italic,
                    "underline": info.underline,
                    "strikethrough": info.strikethrough,
                    "inverse": info.inverse,
                    "width": info.width,
                }),
            )
        } else {
            JsonRpcResponse::success(id, json!({"text": "", "fg": "default", "bg": "default"}))
        }
    } else {
        JsonRpcResponse::invalid_params(id, format!("Surface {} not found", surface_id))
    }
}

#[cfg(debug_assertions)]
fn handle_debug_screen_attrs(
    state: &AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    let row = match params.get("row").and_then(|v| v.as_u64()) {
        Some(r) => r as usize,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'row' parameter"),
    };
    if let Some(terminal) = state.find_terminal_by_id(surface_id) {
        let cells: Vec<_> = terminal
            .row_cells(row)
            .into_iter()
            .map(|(col, info)| {
                json!({
                    "col": col,
                    "text": info.text,
                    "fg": info.fg,
                    "bg": info.bg,
                    "bold": info.bold,
                    "italic": info.italic,
                    "underline": info.underline,
                    "strikethrough": info.strikethrough,
                    "inverse": info.inverse,
                    "width": info.width,
                })
            })
            .collect();
        JsonRpcResponse::success(id, json!({"row": row, "cells": cells}))
    } else {
        JsonRpcResponse::invalid_params(id, format!("Surface {} not found", surface_id))
    }
}

#[cfg(debug_assertions)]
fn require_input_simulation(
    state: &AppState,
    id: &serde_json::Value,
) -> Result<(), JsonRpcResponse> {
    if !state.engine.input_simulation_enabled {
        Err(JsonRpcResponse::error(
            id.clone(),
            -32001,
            "input simulation not enabled. Launch tasty with --enable-input-simulation",
        ))
    } else {
        Ok(())
    }
}

/// Inject a mouse event into a surface's PTY as if the terminal received it.
/// Encodes as SGR mouse (mode 1006) bytes: ESC [ < Cb ; Cx ; Cy M/m
#[cfg(debug_assertions)]
fn handle_debug_inject_mouse(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    if let Err(e) = require_input_simulation(state, &id) {
        return e;
    }
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    // col, row: 1-indexed cell coordinates
    let col = match params.get("col").and_then(|v| v.as_u64()) {
        Some(c) => c,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'col' parameter"),
    };
    let row = match params.get("row").and_then(|v| v.as_u64()) {
        Some(r) => r,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'row' parameter"),
    };
    // button: 0=left, 1=middle, 2=right. Default: 0
    let button = params
        .get("button")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    // event_type: "press", "release", "move". Default: "press"
    let event_type = params
        .get("event_type")
        .and_then(|v| v.as_str())
        .unwrap_or("press");

    let cb = match event_type {
        "press" => button as u8,
        "release" => button as u8,
        "move" => 32 + button as u8,
        _ => return JsonRpcResponse::invalid_params(id, "event_type must be press/release/move"),
    };
    let suffix = if event_type == "release" { "m" } else { "M" };

    // SGR mouse encoding: ESC [ < Cb ; Cx ; Cy M/m (1-indexed)
    let seq = format!("\x1b[<{};{};{}{}", cb, col + 1, row + 1, suffix);

    if let Some(terminal) = state.find_terminal_by_id_mut(surface_id) {
        terminal.send_key(&seq);
        JsonRpcResponse::success(id, json!({"sent": true}))
    } else {
        JsonRpcResponse::invalid_params(id, format!("Surface {} not found", surface_id))
    }
}

/// Inject a key event into a surface's PTY.
#[cfg(debug_assertions)]
fn handle_debug_inject_key(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    if let Err(e) = require_input_simulation(state, &id) {
        return e;
    }
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    let bytes = match params.get("bytes").and_then(|v| v.as_str()) {
        Some(hex) => {
            let hex = hex.trim();
            let mut result = Vec::new();
            for i in (0..hex.len()).step_by(2) {
                match u8::from_str_radix(&hex[i..i.min(hex.len()).max(i + 2)], 16) {
                    Ok(b) => result.push(b),
                    Err(_) => {
                        return JsonRpcResponse::invalid_params(id, "Invalid hex in 'bytes'")
                    }
                }
            }
            result
        }
        None => match params.get("text").and_then(|v| v.as_str()) {
            Some(t) => t.as_bytes().to_vec(),
            None => {
                return JsonRpcResponse::invalid_params(id, "Missing 'bytes' or 'text' parameter")
            }
        },
    };

    if let Some(terminal) = state.find_terminal_by_id_mut(surface_id) {
        terminal.send_key(&String::from_utf8_lossy(&bytes));
        JsonRpcResponse::success(id, json!({"sent": true}))
    } else {
        JsonRpcResponse::invalid_params(id, format!("Surface {} not found", surface_id))
    }
}

fn handle_tree(state: &AppState, id: serde_json::Value) -> JsonRpcResponse {
    let tree: Vec<_> = state
        .engine
        .workspaces
        .iter()
        .enumerate()
        .map(|(i, ws)| {
            let mut t = ws.to_tree_json();
            t["active"] = json!(i == state.active_workspace);
            t["busy_count"] = json!(state.busy_count(&ws.all_surface_ids()));
            annotate_tree_busy(&mut t, state);
            t
        })
        .collect();
    JsonRpcResponse::success(id, json!(tree))
}

/// Walk a workspace tree JSON value and annotate every node that owns surface
/// ids with a `busy_count` field. Surface-leaf nodes also get a `busy` boolean.
fn annotate_tree_busy(node: &mut serde_json::Value, state: &AppState) {
    if let Some(obj) = node.as_object_mut() {
        // Surface leaf: has "id" but no "tabs"/"panes"/"first"/"second"
        let is_leaf = !obj.contains_key("tabs")
            && !obj.contains_key("panes")
            && !obj.contains_key("first")
            && !obj.contains_key("second")
            && obj.get("id").is_some();
        if is_leaf {
            if let Some(sid) = obj.get("id").and_then(|v| v.as_u64()) {
                obj.insert("busy".into(), json!(state.is_surface_busy(sid as u32)));
            }
            return;
        }

        // Recurse into children.
        for key in ["panes", "tabs"] {
            if let Some(arr) = obj.get_mut(key).and_then(|v| v.as_array_mut()) {
                for child in arr.iter_mut() {
                    annotate_tree_busy(child, state);
                }
            }
        }
        for key in ["first", "second", "surface"] {
            if let Some(child) = obj.get_mut(key) {
                annotate_tree_busy(child, state);
            }
        }

        // After children are annotated, sum descendant busy counts.
        let mut count: u64 = 0;
        for key in ["panes", "tabs"] {
            if let Some(arr) = obj.get(key).and_then(|v| v.as_array()) {
                for child in arr {
                    count += child.get("busy_count").and_then(|v| v.as_u64()).unwrap_or(0);
                    if child.get("busy").and_then(|v| v.as_bool()).unwrap_or(false)
                        && child.get("busy_count").is_none()
                    {
                        count += 1;
                    }
                }
            }
        }
        for key in ["first", "second", "surface"] {
            if let Some(child) = obj.get(key) {
                count += child.get("busy_count").and_then(|v| v.as_u64()).unwrap_or(0);
                if child.get("busy").and_then(|v| v.as_bool()).unwrap_or(false)
                    && child.get("busy_count").is_none()
                {
                    count += 1;
                }
            }
        }
        // Workspaces already had busy_count set by the caller; only insert if missing.
        if !obj.contains_key("busy_count") {
            obj.insert("busy_count".into(), json!(count));
        }
    }
}

fn handle_is_typing(
    state: &AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    let typing = state.is_typing(surface_id);
    let idle_seconds = if let Some(last) = state.engine.last_key_input.get(&surface_id) {
        last.elapsed().as_secs_f64()
    } else {
        f64::MAX
    };
    let idle_seconds_capped = if idle_seconds == f64::MAX {
        -1.0
    } else {
        idle_seconds
    };
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
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    let text = match params.get("text").and_then(|v| v.as_str()) {
        Some(t) => t.to_string(),
        None => return JsonRpcResponse::invalid_params(id, "Missing 'text' parameter"),
    };
    if state.is_typing(surface_id) {
        return JsonRpcResponse::success(id, json!({ "sent": false, "reason": "typing" }));
    }
    if let Some(terminal) = state.find_terminal_by_id_mut(surface_id) {
        terminal.send_key(&text);
        JsonRpcResponse::success(id, json!({ "sent": true }))
    } else {
        JsonRpcResponse::invalid_params(id, format!("Surface {} not found", surface_id))
    }
}
