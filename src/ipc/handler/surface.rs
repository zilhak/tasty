use serde_json::json;

use crate::ipc::protocol::JsonRpcResponse;
use crate::state::AppState;

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
    if let Some(terminal) = state.focused_terminal_mut() {
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
