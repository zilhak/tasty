use serde_json::json;

use crate::ipc::protocol::JsonRpcResponse;
use crate::state::AppState;

use super::require_pane_id;

pub fn handle_tab_list(state: &AppState, id: serde_json::Value, params: &serde_json::Value) -> JsonRpcResponse {
    let pane_id = match require_pane_id(params, &id) {
        Ok(pid) => pid,
        Err(e) => return e,
    };
    let ws = state.active_workspace();
    let tabs: Vec<_> = if let Some(pane) = ws.pane_layout().find_pane(pane_id) {
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

    // Focus the target pane so add_tab_background creates the tab there
    if !state.focus_pane(pane_id) {
        return JsonRpcResponse::invalid_params(id, format!("Pane {} not found", pane_id));
    }

    match state.add_tab_background(cwd) {
        Ok(_) => {
            let ws = state.active_workspace();
            let (tab_count, active_tab) = ws.pane_layout().find_pane(pane_id)
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
    let pane_id = match require_pane_id(params, &id) {
        Ok(pid) => pid,
        Err(e) => return e,
    };

    // Focus the target pane so close_active_tab operates on it
    if !state.focus_pane(pane_id) {
        return JsonRpcResponse::invalid_params(id, format!("Pane {} not found", pane_id));
    }

    if state.close_active_tab() {
        JsonRpcResponse::success(id, json!({ "closed": true, "pane_id": pane_id }))
    } else {
        JsonRpcResponse::success(id, json!({ "closed": false, "pane_id": pane_id, "reason": "cannot close the last tab" }))
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

    state.focus_pane(pane_id);

    match state.add_markdown_tab(file_path.clone()) {
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

    state.focus_pane(pane_id);

    match state.add_explorer_tab(path.clone()) {
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
