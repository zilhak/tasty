use serde_json::json;

use crate::ipc::protocol::JsonRpcResponse;
use crate::state::AppState;

pub fn handle_tab_list(state: &AppState, id: serde_json::Value) -> JsonRpcResponse {
    let tabs: Vec<_> = if let Some(pane) = state.focused_pane() {
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
        vec![]
    };
    JsonRpcResponse::success(id, json!(tabs))
}

pub fn handle_tab_create(state: &mut AppState, id: serde_json::Value, params: &serde_json::Value) -> JsonRpcResponse {
    let cwd = params.get("cwd").and_then(|v| v.as_str()).map(std::path::PathBuf::from);
    match state.add_tab_background_with_cwd(cwd) {
        Ok(_) => {
            let (tab_count, active_tab) = state
                .focused_pane()
                .map(|p| (p.tabs.len(), p.active_tab))
                .unwrap_or((0, 0));
            JsonRpcResponse::success(
                id,
                json!({
                    "tab_count": tab_count,
                    "active_tab": active_tab,
                }),
            )
        }
        Err(e) => JsonRpcResponse::internal_error(id, e.to_string()),
    }
}

pub fn handle_tab_close(state: &mut AppState, id: serde_json::Value) -> JsonRpcResponse {
    if state.close_active_tab() {
        JsonRpcResponse::success(id, json!({ "closed": true }))
    } else {
        JsonRpcResponse::success(id, json!({ "closed": false, "reason": "cannot close the last tab" }))
    }
}

pub fn handle_open_markdown(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let file_path = match params.get("file_path").and_then(|v| v.as_str()) {
        Some(p) => p.to_string(),
        None => return JsonRpcResponse::invalid_params(id, "Missing 'file_path' parameter"),
    };

    if let Some(pane_id) = params.get("pane_id").and_then(|v| v.as_u64()) {
        state.focus_pane(pane_id as u32);
    }

    match state.add_markdown_tab(file_path.clone()) {
        Ok(_) => JsonRpcResponse::success(
            id,
            json!({
                "ok": true,
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
    let path = params
        .get("path")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            directories::BaseDirs::new()
                .map(|d| d.home_dir().to_string_lossy().to_string())
                .unwrap_or_else(|| ".".to_string())
        });

    if let Some(pane_id) = params.get("pane_id").and_then(|v| v.as_u64()) {
        state.focus_pane(pane_id as u32);
    }

    match state.add_explorer_tab(path.clone()) {
        Ok(_) => JsonRpcResponse::success(
            id,
            json!({
                "ok": true,
                "path": path,
            }),
        ),
        Err(e) => JsonRpcResponse::internal_error(id, format!("Failed to open explorer: {}", e)),
    }
}
