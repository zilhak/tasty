use serde_json::json;

use crate::ipc::protocol::JsonRpcResponse;
use crate::model::SplitDirection;
use crate::state::AppState;

use super::{apply_meta, require_pane_id, resolve_target_param};

pub fn handle_pane_list(state: &AppState, id: serde_json::Value) -> JsonRpcResponse {
    let mut panes = Vec::new();
    for ws in &state.engine.workspaces {
        let pane_ids = ws.pane_layout().all_pane_ids();
        let focused = ws.focused_pane;
        for &pid in &pane_ids {
            let tab_count = ws
                .pane_layout()
                .find_pane(pid)
                .map(|p| p.tabs.len())
                .unwrap_or(0);
            panes.push(json!({
                "id": pid,
                "workspace_id": ws.id,
                "focused": pid == focused,
                "tab_count": tab_count,
            }));
        }
    }
    JsonRpcResponse::success(id, json!(panes))
}

pub fn handle_pane_close(state: &mut AppState, id: serde_json::Value, params: &serde_json::Value) -> JsonRpcResponse {
    let pane_id = match require_pane_id(params, &id) {
        Ok(pid) => pid,
        Err(e) => return e,
    };

    // Prevent closing a pane that contains the caller
    if let Some(caller) = super::caller_surface_id(params) {
        if super::surface_belongs_to_pane(state, caller, pane_id) {
            return JsonRpcResponse::invalid_params(id,
                "Cannot close a pane that contains your own surface. Close all other surfaces in the pane first, then use 'tasty close self'.");
        }
    }

    if state.find_pane_by_id(pane_id).is_none() {
        return JsonRpcResponse::invalid_params(id, format!("Pane {} not found", pane_id));
    }

    let closed = state.close_pane_by_id(pane_id);

    if closed {
        JsonRpcResponse::success(id, json!({ "closed": true, "pane_id": pane_id }))
    } else {
        JsonRpcResponse::success(id, json!({ "closed": false, "pane_id": pane_id, "reason": "cannot close the last pane" }))
    }
}

pub fn handle_split(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let level = match params.get("level").and_then(|v| v.as_str()) {
        Some("pane-group") | Some("pane") => "pane",
        Some("surface") => "surface",
        Some(other) => {
            return JsonRpcResponse::invalid_params(
                id,
                format!("Invalid level '{}'. Use: pane, surface", other),
            )
        }
        None => return JsonRpcResponse::invalid_params(id, "Missing 'level' parameter"),
    };

    let direction = match params.get("direction").and_then(|v| v.as_str()) {
        Some("horizontal") | Some("h") => SplitDirection::Horizontal,
        _ => SplitDirection::Vertical,
    };

    let target_id = resolve_target_param(params.get("target"), level);
    if target_id.is_none() {
        return JsonRpcResponse::invalid_params(id, "Missing required 'target' parameter (numeric ID or nickname)");
    }

    let meta = params.get("meta").and_then(|v| v.as_object());
    let cwd = params.get("cwd").and_then(|v| v.as_str()).map(std::path::PathBuf::from);
    let surface_type = match params.get("type").and_then(|v| v.as_str()).unwrap_or("terminal") {
        "markdown" => {
            let file = params.get("file").and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if file.is_empty() {
                return JsonRpcResponse::invalid_params(id, "Missing 'file' parameter for markdown type");
            }
            crate::model::SurfaceType::Markdown { file }
        }
        "explorer" => {
            let path = params.get("path").and_then(|v| v.as_str()).map(|s| s.to_string());
            crate::model::SurfaceType::Explorer { path }
        }
        _ => crate::model::SurfaceType::Terminal,
    };

    match level {
        "pane" => match state.split_pane_targeted(target_id, direction, cwd, surface_type) {
            Ok((new_pane_id, new_surface_id)) => {
                apply_meta(new_surface_id, meta);
                JsonRpcResponse::success(
                    id,
                    json!({
                        "new_pane_id": new_pane_id,
                        "new_surface_id": new_surface_id,
                    }),
                )
            }
            Err(e) => JsonRpcResponse::internal_error(id, e.to_string()),
        },
        "surface" => match state.split_surface_targeted(target_id, direction, cwd) {
            Ok(new_surface_id) => {
                apply_meta(new_surface_id, meta);
                JsonRpcResponse::success(
                    id,
                    json!({
                        "new_surface_id": new_surface_id,
                    }),
                )
            }
            Err(e) => JsonRpcResponse::internal_error(id, e.to_string()),
        },
        _ => unreachable!(),
    }
}

// focus.direction removed: focus is user-only (shortcuts/clicks).
