use serde_json::json;

use crate::ipc::protocol::JsonRpcResponse;
use crate::state::AppState;
use crate::terminal::Terminal;

pub(crate) fn handle_surface_list(state: &AppState, id: serde_json::Value) -> JsonRpcResponse {
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
        crate::model::Panel::Markdown(_) | crate::model::Panel::Explorer(_) => {
            // Non-terminal panels have no surfaces to list.
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

pub(crate) fn handle_surface_send(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let text = match params.get("text").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'text' parameter"),
    };
    let surface_id = params.get("surface_id").and_then(|v| v.as_u64()).map(|v| v as u32);
    let terminal = if let Some(sid) = surface_id {
        state.find_terminal_by_id_mut(sid)
    } else {
        state.focused_terminal_mut()
    };
    if let Some(terminal) = terminal {
        terminal.send_key(text);
    }
    JsonRpcResponse::success(id, json!({ "sent": true }))
}

pub(crate) fn handle_surface_send_key(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let key = match params.get("key").and_then(|v| v.as_str()) {
        Some(k) => k,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'key' parameter"),
    };
    let surface_id = params.get("surface_id").and_then(|v| v.as_u64()).map(|v| v as u32);

    let bytes: Vec<u8> = match key {
        "enter" => b"\r".to_vec(),
        "tab" => b"\t".to_vec(),
        "escape" | "esc" => b"\x1b".to_vec(),
        "backspace" => b"\x7f".to_vec(),
        "up" => b"\x1b[A".to_vec(),
        "down" => b"\x1b[B".to_vec(),
        "right" => b"\x1b[C".to_vec(),
        "left" => b"\x1b[D".to_vec(),
        "home" => b"\x1b[H".to_vec(),
        "end" => b"\x1b[F".to_vec(),
        "pageup" => b"\x1b[5~".to_vec(),
        "pagedown" => b"\x1b[6~".to_vec(),
        "delete" => b"\x1b[3~".to_vec(),
        "insert" => b"\x1b[2~".to_vec(),
        "f1" => b"\x1bOP".to_vec(),
        "f2" => b"\x1bOQ".to_vec(),
        "f3" => b"\x1bOR".to_vec(),
        "f4" => b"\x1bOS".to_vec(),
        "f5" => b"\x1b[15~".to_vec(),
        "f6" => b"\x1b[17~".to_vec(),
        "f7" => b"\x1b[18~".to_vec(),
        "f8" => b"\x1b[19~".to_vec(),
        "f9" => b"\x1b[20~".to_vec(),
        "f10" => b"\x1b[21~".to_vec(),
        "f11" => b"\x1b[23~".to_vec(),
        "f12" => b"\x1b[24~".to_vec(),
        other => {
            let terminal = resolve_terminal(state, surface_id);
            if let Some(terminal) = terminal {
                terminal.send_key(other);
            }
            return JsonRpcResponse::success(id, json!({ "sent": true }));
        }
    };
    let terminal = resolve_terminal(state, surface_id);
    if let Some(terminal) = terminal {
        terminal.send_bytes(&bytes);
    }
    JsonRpcResponse::success(id, json!({ "sent": true }))
}

/// Helper to resolve a terminal by optional surface_id, falling back to focused.
fn resolve_terminal(state: &mut AppState, surface_id: Option<u32>) -> Option<&mut Terminal> {
    if let Some(sid) = surface_id {
        state.find_terminal_by_id_mut(sid)
    } else {
        state.focused_terminal_mut()
    }
}

pub(crate) fn handle_surface_close(state: &mut AppState, id: serde_json::Value) -> JsonRpcResponse {
    if state.close_active_surface() {
        JsonRpcResponse::success(id, json!({ "closed": true }))
    } else {
        JsonRpcResponse::success(id, json!({ "closed": false, "reason": "cannot close a single terminal surface" }))
    }
}

pub(crate) fn handle_set_mark(
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

pub(crate) fn handle_read_since_mark(
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

pub(crate) fn handle_screen_text(
    state: &AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = params.get("surface_id").and_then(|v| v.as_u64()).map(|v| v as u32);
    let text = if let Some(sid) = surface_id {
        state.find_terminal_by_id(sid).map(|t| t.screen_text()).unwrap_or_default()
    } else {
        state.focused_terminal().map(|t| t.screen_text()).unwrap_or_default()
    };
    JsonRpcResponse::success(id, json!({ "text": text }))
}

pub(crate) fn handle_cursor_position(
    state: &AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = params.get("surface_id").and_then(|v| v.as_u64()).map(|v| v as u32);
    let terminal = if let Some(sid) = surface_id {
        state.find_terminal_by_id(sid)
    } else {
        state.focused_terminal()
    };
    if let Some(terminal) = terminal {
        let (x, y) = terminal.surface().cursor_position();
        JsonRpcResponse::success(id, json!({ "x": x, "y": y }))
    } else {
        JsonRpcResponse::success(id, json!({ "x": 0, "y": 0 }))
    }
}

pub(crate) fn handle_surface_send_combo(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let key = match params.get("key").and_then(|v| v.as_str()) {
        Some(k) => k,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'key' parameter"),
    };
    let modifiers = params.get("modifiers")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect::<Vec<_>>())
        .unwrap_or_default();
    let surface_id = params.get("surface_id").and_then(|v| v.as_u64()).map(|v| v as u32);

    let has_ctrl = modifiers.iter().any(|m| m == "ctrl");
    let has_alt = modifiers.iter().any(|m| m == "alt");

    let mut bytes_to_send: Vec<u8> = Vec::new();

    if has_ctrl && key.len() == 1 {
        let ch = key.chars().next().unwrap().to_ascii_lowercase();
        if ch >= 'a' && ch <= 'z' {
            bytes_to_send.push(ch as u8 - b'a' + 1);
        } else if ch == '[' {
            bytes_to_send.push(0x1B);
        } else if ch == '\\' {
            bytes_to_send.push(0x1C);
        } else if ch == ']' {
            bytes_to_send.push(0x1D);
        }
    } else {
        if has_alt {
            bytes_to_send.push(0x1B);
        }
        bytes_to_send.extend_from_slice(key.as_bytes());
    }

    // Get the terminal: either by surface_id or focused
    let terminal = if let Some(sid) = surface_id {
        state.find_terminal_by_id_mut(sid)
    } else {
        state.focused_terminal_mut()
    };

    if let Some(terminal) = terminal {
        terminal.send_bytes(&bytes_to_send);
        JsonRpcResponse::success(id, json!({ "sent": true }))
    } else {
        JsonRpcResponse::internal_error(id, "No terminal found".to_string())
    }
}


pub(crate) fn handle_pane_focus(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let pane_id = match params.get("pane_id").and_then(|v| v.as_u64()) {
        Some(pid) => pid as u32,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'pane_id' parameter"),
    };
    if state.focus_pane(pane_id) {
        JsonRpcResponse::success(id, json!({ "focused": true, "pane_id": pane_id }))
    } else {
        JsonRpcResponse::invalid_params(id, format!("Pane {} not found", pane_id))
    }
}

pub(crate) fn handle_surface_focus(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match params.get("surface_id").and_then(|v| v.as_u64()) {
        Some(sid) => sid as u32,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'surface_id' parameter"),
    };
    if state.focus_surface(surface_id) {
        JsonRpcResponse::success(id, json!({ "focused": true, "surface_id": surface_id }))
    } else {
        JsonRpcResponse::invalid_params(id, format!("Surface {} not found", surface_id))
    }
}

pub(crate) fn handle_surface_send_to(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let text = match params.get("text").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'text' parameter"),
    };
    let surface_id = match params.get("surface_id").and_then(|v| v.as_u64()) {
        Some(sid) => sid as u32,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'surface_id' parameter"),
    };
    if let Some(terminal) = state.find_terminal_by_id_mut(surface_id) {
        terminal.send_key(text);
        JsonRpcResponse::success(id, json!({ "sent": true }))
    } else {
        JsonRpcResponse::invalid_params(id, format!("Surface {} not found", surface_id))
    }
}
