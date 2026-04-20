use serde_json::json;

use crate::ipc::protocol::JsonRpcResponse;
use crate::model::SplitDirection;
use crate::state::AppState;

use super::{apply_meta, require_pane_id};

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
                "workspace_name": ws.name,
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

/// Resolve a surface target from params: tries target_surface first, then legacy "target".
/// Supports numeric ID and nickname string.
fn resolve_surface_target(params: &serde_json::Value) -> Option<u32> {
    let val = params.get("target_surface")
        .or_else(|| params.get("target")); // legacy fallback
    let val = val?;
    if val.is_null() {
        return None;
    }
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
        // Try nickname lookup
        return crate::surface_meta::SurfaceMetaStore::find_by_value("nickname", s);
    }
    None
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

    // Resolve target: new target_surface/target_pane params, with legacy "target" fallback
    let target_surface_id = resolve_surface_target(params);
    let target_pane_id = params.get("target_pane").and_then(|v| v.as_u64()).map(|v| v as u32);

    // Validate: at least one target must be specified
    if target_surface_id.is_none() && target_pane_id.is_none() {
        return JsonRpcResponse::invalid_params(id, "Missing target. Use 'target_surface' (surface ID or nickname) and/or 'target_pane' (pane ID)");
    }
    // Validate: can't specify both
    if target_surface_id.is_some() && target_pane_id.is_some() {
        return JsonRpcResponse::invalid_params(id, "Cannot specify both 'target_surface' and 'target_pane'. Use one.");
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
        "html" => {
            let url = params.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string();
            if url.is_empty() {
                return JsonRpcResponse::invalid_params(id, "Missing 'url' parameter for html type");
            }
            crate::model::SurfaceType::Html { url }
        }
        _ => crate::model::SurfaceType::Terminal,
    };

    match level {
        "pane" => {
            // For pane-level splits, resolve the pane ID from either target_pane or target_surface
            let resolved_pane_id = if let Some(pid) = target_pane_id {
                Some(pid)
            } else if let Some(sid) = target_surface_id {
                // Find the pane containing the given surface
                state.find_pane_for_surface(sid)
            } else {
                None
            };

            match state.split_pane_targeted(resolved_pane_id, direction, cwd, surface_type) {
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
            }
        },
        "surface" => {
            // Surface-level splits require a surface target
            let sid = match target_surface_id {
                Some(sid) => sid,
                None => return JsonRpcResponse::invalid_params(id, "Surface-level split requires 'target_surface', not 'target_pane'"),
            };

            match state.split_surface_targeted(Some(sid), direction, cwd, surface_type) {
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
            }
        },
        _ => unreachable!(),
    }
}

// focus.direction removed: focus is user-only (shortcuts/clicks).
