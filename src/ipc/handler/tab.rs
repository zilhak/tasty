use serde_json::json;

use crate::ipc::protocol::JsonRpcResponse;
use crate::state::AppState;

use super::require_pane_id;

fn require_tab_id(params: &serde_json::Value, id: &serde_json::Value) -> Result<u32, JsonRpcResponse> {
    params
        .get("tab_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .ok_or_else(|| JsonRpcResponse::invalid_params(id.clone(), "Missing required 'tab_id' parameter"))
}

pub fn handle_tab_list(state: &AppState, id: serde_json::Value, params: &serde_json::Value) -> JsonRpcResponse {
    let pane_id = match require_pane_id(params, &id) {
        Ok(pid) => pid,
        Err(e) => return e,
    };
    let tabs: Vec<_> = if let Some(pane) = state.find_pane_by_id(pane_id) {
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
        return JsonRpcResponse::invalid_params(id, format!("Pane {} not found", pane_id));
    };
    JsonRpcResponse::success(id, json!({ "pane_id": pane_id, "tabs": tabs }))
}

pub fn handle_tab_create(state: &mut AppState, id: serde_json::Value, params: &serde_json::Value) -> JsonRpcResponse {
    let pane_id = match require_pane_id(params, &id) {
        Ok(pid) => pid,
        Err(e) => return e,
    };
    let cwd = params.get("cwd").and_then(|v| v.as_str()).map(std::path::PathBuf::from);

    if state.find_pane_by_id(pane_id).is_none() {
        return JsonRpcResponse::invalid_params(id, format!("Pane {} not found", pane_id));
    }

    let result = state.add_tab_to_pane(pane_id, cwd);

    match result {
        Ok(_) => {
            let (tab_count, active_tab) = state.find_pane_by_id(pane_id)
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

pub fn handle_tab_close(state: &mut AppState, id: serde_json::Value, params: &serde_json::Value) -> JsonRpcResponse {
    let tab_id = match require_tab_id(params, &id) {
        Ok(tid) => tid,
        Err(e) => return e,
    };

    // Prevent closing a tab that contains the caller
    if let Some(caller) = super::caller_surface_id(params) {
        // Find which pane contains this tab
        if let Some(pane_id) = state.find_pane_for_tab(tab_id) {
            if super::surface_belongs_to_pane(state, caller, pane_id) {
                return JsonRpcResponse::invalid_params(id,
                    "Cannot close a tab that contains your own surface. Use 'tasty close self' instead.");
            }
        }
    }

    let closed = state.close_tab_by_tab_id(tab_id);

    if closed {
        JsonRpcResponse::success(id, json!({ "closed": true, "tab_id": tab_id }))
    } else {
        JsonRpcResponse::success(id, json!({ "closed": false, "tab_id": tab_id, "reason": "tab not found or cannot close the last tab" }))
    }
}

pub fn handle_open_markdown(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let pane_id = match require_pane_id(params, &id) {
        Ok(pid) => pid,
        Err(e) => return e,
    };
    let file_path = match params.get("file_path").and_then(|v| v.as_str()) {
        Some(p) => p.to_string(),
        None => return JsonRpcResponse::invalid_params(id, "Missing 'file_path' parameter"),
    };

    if state.find_pane_by_id(pane_id).is_none() {
        return JsonRpcResponse::invalid_params(id, format!("Pane {} not found", pane_id));
    }

    let result = state.add_markdown_tab_to_pane(pane_id, file_path.clone());

    match result {
        Ok(_) => JsonRpcResponse::success(
            id,
            json!({
                "ok": true,
                "pane_id": pane_id,
                "file_path": file_path,
            }),
        ),
        Err(e) => JsonRpcResponse::internal_error(id, format!("Failed to open markdown: {}", e)),
    }
}

pub fn handle_open_explorer(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let pane_id = match require_pane_id(params, &id) {
        Ok(pid) => pid,
        Err(e) => return e,
    };
    let path = params
        .get("path")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            directories::BaseDirs::new()
                .map(|d| d.home_dir().to_string_lossy().to_string())
                .unwrap_or_else(|| ".".to_string())
        });

    if state.find_pane_by_id(pane_id).is_none() {
        return JsonRpcResponse::invalid_params(id, format!("Pane {} not found", pane_id));
    }

    let result = state.add_explorer_tab_to_pane(pane_id, path.clone());

    match result {
        Ok(_) => JsonRpcResponse::success(
            id,
            json!({
                "ok": true,
                "pane_id": pane_id,
                "path": path,
            }),
        ),
        Err(e) => JsonRpcResponse::internal_error(id, format!("Failed to open explorer: {}", e)),
    }
}
