use serde_json::json;

use crate::ipc::protocol::JsonRpcResponse;
use crate::state::AppState;

use super::require_pane_id;

fn require_tab_id(
    params: &serde_json::Value,
    id: &serde_json::Value,
) -> Result<u32, JsonRpcResponse> {
    params
        .get("tab_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .ok_or_else(|| {
            JsonRpcResponse::invalid_params(id.clone(), "Missing required 'tab_id' parameter")
        })
}

pub fn handle_tab_list(
    state: &AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let pane_id = match require_pane_id(params, &id) {
        Ok(pid) => pid,
        Err(e) => return e,
    };
    let tabs: Vec<_> = if let Some(pane) = state.find_pane_by_id(pane_id) {
        pane.tabs
            .iter()
            .enumerate()
            .map(|(i, tab)| {
                let surface = tab.surface();
                let surface_type = surface.type_name();
                let surface_id = surface.surface_id();
                let mut entry = json!({
                    "id": tab.id,
                    "name": tab.name,
                    "active": i == pane.active_tab,
                    "type": surface_type,
                });
                if let Some(sid) = surface_id {
                    entry["surface_id"] = json!(sid);
                }
                entry
            })
            .collect()
    } else {
        return JsonRpcResponse::invalid_params(id, format!("Pane {} not found", pane_id));
    };
    JsonRpcResponse::success(id, json!({ "pane_id": pane_id, "tabs": tabs }))
}

pub fn handle_tab_create(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let pane_id = match require_pane_id(params, &id) {
        Ok(pid) => pid,
        Err(e) => return e,
    };

    if state.find_pane_by_id(pane_id).is_none() {
        return JsonRpcResponse::invalid_params(id, format!("Pane {} not found", pane_id));
    }

    let surface_type = params
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("terminal");

    let panel_id = state.engine.next_ids.next_surface();
    let result = match surface_type {
        "markdown" => {
            let file_path = match params
                .get("file")
                .or_else(|| params.get("file_path"))
                .and_then(|v| v.as_str())
            {
                Some(p) => p.to_string(),
                None => {
                    return JsonRpcResponse::invalid_params(
                        id,
                        "Missing 'file' parameter for markdown type",
                    );
                }
            };
            let name = file_path
                .split(['/', '\\'])
                .last()
                .unwrap_or("Markdown")
                .to_string();
            let surface: Box<dyn crate::model::Surface> =
                Box::new(crate::model::MarkdownPanel::new(panel_id, file_path));
            state.add_surface_tab_to_pane(pane_id, name, surface);
            Ok(())
        }
        "html" => {
            let url = match params.get("url").and_then(|v| v.as_str()) {
                Some(u) => u.to_string(),
                None => {
                    return JsonRpcResponse::invalid_params(
                        id,
                        "Missing 'url' parameter for html type",
                    );
                }
            };
            let surface: Box<dyn crate::model::Surface> =
                Box::new(crate::model::HtmlPanel::new(panel_id, url));
            state.add_surface_tab_to_pane(pane_id, "HTML".to_string(), surface);
            Ok(())
        }
        "explorer" => {
            let path = params
                .get("path")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| {
                    directories::BaseDirs::new()
                        .map(|d| d.home_dir().to_string_lossy().to_string())
                        .unwrap_or_else(|| ".".to_string())
                });
            let name = path
                .split(['/', '\\'])
                .last()
                .unwrap_or("Explorer")
                .to_string();
            let surface: Box<dyn crate::model::Surface> =
                Box::new(crate::model::ExplorerPanel::new(panel_id, path));
            state.add_surface_tab_to_pane(pane_id, name, surface);
            Ok(())
        }
        "image" => {
            let file_path = params
                .get("file")
                .or_else(|| params.get("file_path"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let name = file_path
                .as_ref()
                .and_then(|p| p.split(['/', '\\']).last().map(|s| s.to_string()))
                .unwrap_or_else(|| "Image".to_string());
            let surface: Box<dyn crate::model::Surface> = match file_path {
                Some(path) => Box::new(crate::model::ImagePanel::new(panel_id, path)),
                None => Box::new(crate::model::ImagePanel::new_blank(panel_id)),
            };
            state.add_surface_tab_to_pane(pane_id, name, surface);
            Ok(())
        }
        "terminal" | _ => {
            let cwd = params
                .get("cwd")
                .and_then(|v| v.as_str())
                .map(std::path::PathBuf::from);
            state.add_tab_to_pane(pane_id, cwd)
        }
    };

    match result {
        Ok(_) => {
            let (tab_count, active_tab) = state
                .find_pane_by_id(pane_id)
                .map(|p| (p.tabs.len(), p.active_tab))
                .unwrap_or((0, 0));
            JsonRpcResponse::success(
                id,
                json!({
                    "pane_id": pane_id,
                    "tab_count": tab_count,
                    "active_tab": active_tab,
                }),
            )
        }
        Err(e) => JsonRpcResponse::internal_error(id, e.to_string()),
    }
}

pub fn handle_tab_close(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let tab_id = match require_tab_id(params, &id) {
        Ok(tid) => tid,
        Err(e) => return e,
    };

    // Prevent closing a tab that contains the caller
    if let Some(caller) = super::caller_surface_id(params) {
        // Find which pane contains this tab
        if let Some(pane_id) = state.find_pane_for_tab(tab_id) {
            if super::surface_belongs_to_pane(state, caller, pane_id) {
                return JsonRpcResponse::invalid_params(
                    id,
                    "Cannot close a tab that contains your own surface. Use 'tasty close self' instead.",
                );
            }
        }
    }

    let closed = state.close_tab_by_tab_id(tab_id);

    if closed {
        JsonRpcResponse::success(id, json!({ "closed": true, "tab_id": tab_id }))
    } else {
        JsonRpcResponse::success(
            id,
            json!({ "closed": false, "tab_id": tab_id, "reason": "tab not found or cannot close the last tab" }),
        )
    }
}

pub fn handle_tab_move(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let pane_id = match require_pane_id(params, &id) {
        Ok(pid) => pid,
        Err(e) => return e,
    };
    let from = match params.get("from_index").and_then(|v| v.as_u64()) {
        Some(f) => f as usize,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'from_index' parameter"),
    };
    let to = match params.get("to_index").and_then(|v| v.as_u64()) {
        Some(t) => t as usize,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'to_index' parameter"),
    };

    if let Some(pane) = state
        .active_workspace_mut()
        .pane_layout_mut()
        .find_pane_mut(pane_id)
    {
        let moved = pane.move_tab(from, to);
        JsonRpcResponse::success(id, json!({ "moved": moved, "pane_id": pane_id }))
    } else {
        JsonRpcResponse::invalid_params(id, format!("Pane {} not found", pane_id))
    }
}

// handle_open_markdown / handle_open_explorer removed: use handle_tab_create with type parameter
