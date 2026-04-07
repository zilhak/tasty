use serde_json::json;

use crate::gpu::ImePreeditState;
use crate::ipc::protocol::JsonRpcResponse;
use crate::tasty_window::TastyWindow;

/// Handle IME simulation IPC methods.
/// These require window-local state (ime_active, ime_preedit) so they are
/// dispatched from App::process_ipc() rather than the AppState-level handler.
pub fn handle_ime_method(
    w: &mut TastyWindow,
    method: &str,
    params: &serde_json::Value,
    id: serde_json::Value,
) -> JsonRpcResponse {
    match method {
        "surface.ime_enable" => handle_ime_enable(w, id),
        "surface.ime_disable" => handle_ime_disable(w, id),
        "surface.ime_preedit" => handle_ime_preedit(w, params, id),
        "surface.ime_commit" => handle_ime_commit(w, params, id),
        "surface.ime_status" => handle_ime_status(w, id),
        _ => JsonRpcResponse::method_not_found(id, method),
    }
}

fn handle_ime_enable(w: &mut TastyWindow, id: serde_json::Value) -> JsonRpcResponse {
    w.ime_active = true;
    w.mark_dirty();
    JsonRpcResponse::success(id, json!({ "active": true }))
}

fn handle_ime_disable(w: &mut TastyWindow, id: serde_json::Value) -> JsonRpcResponse {
    w.ime_active = false;
    w.ime_preedit = None;
    w.mark_dirty();
    JsonRpcResponse::success(id, json!({ "active": false, "preedit_cleared": true }))
}

fn handle_ime_preedit(
    w: &mut TastyWindow,
    params: &serde_json::Value,
    id: serde_json::Value,
) -> JsonRpcResponse {
    let text = match params.get("text").and_then(|v| v.as_str()) {
        Some(t) => t.to_string(),
        None => return JsonRpcResponse::invalid_params(id, "Missing 'text' parameter"),
    };

    // Empty text clears the preedit
    if text.is_empty() {
        w.ime_preedit = None;
        w.mark_dirty();
        return JsonRpcResponse::success(id, json!({ "preedit_active": false }));
    }

    let cursor = params
        .get("cursor")
        .and_then(|v| v.as_u64())
        .map(|c| (c as usize, (c as usize) + text.len()));

    let surface_id = w.state.focused_surface_id();
    let cursor_pos = w.state.focused_terminal().map(|terminal| {
        let (col, row) = terminal.surface().cursor_position();
        let cols = terminal.cols();
        // Reconcile advance with PTY echo
        if w.ime_cursor_advance > 0 {
            let (base_col, base_row) = w.ime_advance_base;
            let raw_advance = if row > base_row {
                (row - base_row) * cols + col - base_col
            } else if col >= base_col {
                col - base_col
            } else {
                0
            };
            if raw_advance >= w.ime_cursor_advance {
                w.ime_cursor_advance = 0;
            } else {
                w.ime_cursor_advance -= raw_advance;
            }
            w.ime_advance_base = (col, row);
        }
        let adjusted_col = col + w.ime_cursor_advance;
        if cols > 0 && adjusted_col >= cols {
            (adjusted_col % cols, row + adjusted_col / cols)
        } else {
            (adjusted_col, row)
        }
    });

    match (surface_id, cursor_pos) {
        (Some(surface_id), Some((anchor_col, anchor_row))) => {
            w.ime_preedit = Some(ImePreeditState {
                text: text.clone(),
                cursor,
                anchor_col,
                anchor_row,
                surface_id,
            });
            w.update_ime_cursor_area();
            w.mark_dirty();
            JsonRpcResponse::success(
                id,
                json!({
                    "preedit_active": true,
                    "text": text,
                    "anchor_col": anchor_col,
                    "anchor_row": anchor_row,
                    "surface_id": surface_id,
                }),
            )
        }
        _ => JsonRpcResponse::internal_error(id, "No focused terminal"),
    }
}

fn handle_ime_commit(
    w: &mut TastyWindow,
    params: &serde_json::Value,
    id: serde_json::Value,
) -> JsonRpcResponse {
    let text = match params.get("text").and_then(|v| v.as_str()) {
        Some(t) => t.to_string(),
        None => return JsonRpcResponse::invalid_params(id, "Missing 'text' parameter"),
    };

    // Record base cursor position before accumulating advance
    if w.ime_cursor_advance == 0 {
        if let Some(terminal) = w.state.focused_terminal() {
            w.ime_advance_base = terminal.surface().cursor_position();
        }
    }
    // Accumulate display width for cursor advance (same as real IME commit)
    for ch in text.chars() {
        w.ime_cursor_advance += crate::renderer::unicode_width(ch);
    }
    w.ime_preedit = None; // clear preedit but keep ime_cursor_advance

    let sid = w.state.focused_surface_id();
    if let Some(terminal) = w.state.focused_terminal_mut() {
        terminal.send_key(&text);
    }
    if let Some(sid) = sid {
        w.state.record_typing(sid);
    }
    w.mark_dirty();

    JsonRpcResponse::success(id, json!({ "committed": true, "text": text }))
}

fn handle_ime_status(w: &TastyWindow, id: serde_json::Value) -> JsonRpcResponse {
    let preedit_text = w.ime_preedit.as_ref().map(|p| p.text.as_str());
    JsonRpcResponse::success(
        id,
        json!({
            "active": w.ime_active,
            "preedit_text": preedit_text,
            "has_preedit": w.ime_preedit.is_some(),
        }),
    )
}
