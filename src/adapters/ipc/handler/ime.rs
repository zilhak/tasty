use serde_json::json;

use super::params::{self, p_try};
use crate::view::main::MainView;
use crate::view::main::ime as window_ime;
use crate::view::ui::View as _;
use tasty_ipc::protocol::JsonRpcResponse;

/// Handle IME simulation IPC methods.
/// These require window-local state (ime_active, ime_preedit) so they are
/// dispatched from App::process_ipc() rather than the AppState-level handler.
pub fn handle_ime_method(
    w: &mut MainView,
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

fn handle_ime_enable(w: &mut MainView, id: serde_json::Value) -> JsonRpcResponse {
    w.ime_active = true;
    w.mark_dirty();
    JsonRpcResponse::success(id, json!({ "active": true }))
}

fn handle_ime_disable(w: &mut MainView, id: serde_json::Value) -> JsonRpcResponse {
    w.ime_active = false;
    w.ime_preedit = None;
    w.mark_dirty();
    JsonRpcResponse::success(id, json!({ "active": false, "preedit_cleared": true }))
}

fn handle_ime_preedit(
    w: &mut MainView,
    params: &serde_json::Value,
    id: serde_json::Value,
) -> JsonRpcResponse {
    let text = match params.get("text").and_then(|v| v.as_str()) {
        Some(t) => t.to_string(),
        None => return JsonRpcResponse::invalid_params(id, "Missing 'text' parameter"),
    };

    if text.is_empty() {
        w.ime_preedit = None;
        w.mark_dirty();
        return JsonRpcResponse::success(id, json!({ "preedit_active": false }));
    }

    let cursor = p_try!(params::opt_int::<u64>(params, "cursor", &id))
        .map(|c| (c as usize, (c as usize) + text.len()));

    let text_for_response = text.clone();
    match window_ime::ipc_set_preedit(w, text, cursor) {
        Some((anchor_col, anchor_row, surface_id)) => JsonRpcResponse::success(
            id,
            json!({
                "preedit_active": true,
                "text": text_for_response,
                "anchor_col": anchor_col,
                "anchor_row": anchor_row,
                "surface_id": surface_id,
            }),
        ),
        None => JsonRpcResponse::internal_error(id, "No focused terminal"),
    }
}

fn handle_ime_commit(
    w: &mut MainView,
    params: &serde_json::Value,
    id: serde_json::Value,
) -> JsonRpcResponse {
    let text = match params.get("text").and_then(|v| v.as_str()) {
        Some(t) => t.to_string(),
        None => return JsonRpcResponse::invalid_params(id, "Missing 'text' parameter"),
    };

    window_ime::ipc_commit(w, &text);

    JsonRpcResponse::success(id, json!({ "committed": true, "text": text }))
}

fn handle_ime_status(w: &MainView, id: serde_json::Value) -> JsonRpcResponse {
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
