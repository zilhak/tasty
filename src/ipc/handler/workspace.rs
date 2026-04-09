use serde_json::json;

use crate::ipc::protocol::JsonRpcResponse;
use crate::state::AppState;

pub fn handle_workspace_list(state: &AppState, id: serde_json::Value) -> JsonRpcResponse {
    let workspaces: Vec<_> = state
        .engine.workspaces
        .iter()
        .enumerate()
        .map(|(i, ws)| {
            json!({
                "id": ws.id,
                "name": ws.name,
                "subtitle": ws.subtitle,
                "description": ws.description,
                "active": i == state.active_workspace,
                "pane_count": ws.pane_layout().all_pane_ids().len(),
            })
        })
        .collect();
    JsonRpcResponse::success(id, json!(workspaces))
}

pub fn handle_workspace_create(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let cwd = params.get("cwd").and_then(|v| v.as_str()).map(std::path::PathBuf::from);
    match state.add_workspace_background(cwd) {
        Ok(idx) => {
            if let Some(name) = params.get("name").and_then(|v| v.as_str()) {
                if !name.is_empty() {
                    state.engine.workspaces[idx].name = name.to_string();
                }
            }
            if let Some(subtitle) = params.get("subtitle").and_then(|v| v.as_str()) {
                state.engine.workspaces[idx].subtitle = subtitle.to_string();
            }
            if let Some(desc) = params.get("description").and_then(|v| v.as_str()) {
                state.engine.workspaces[idx].description = desc.to_string();
            }
            let ws = &state.engine.workspaces[idx];
            JsonRpcResponse::success(
                id,
                json!({
                    "id": ws.id,
                    "name": ws.name,
                    "subtitle": ws.subtitle,
                    "description": ws.description,
                    "index": idx,
                }),
            )
        }
        Err(e) => JsonRpcResponse::internal_error(id, e.to_string()),
    }
}

pub fn handle_workspace_update(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let idx = if let Some(i) = params.get("index").and_then(|v| v.as_u64()) {
        i as usize
    } else if let Some(ws_id) = params.get("id").and_then(|v| v.as_u64()) {
        match state.engine.workspaces.iter().position(|ws| ws.id == ws_id as u32) {
            Some(i) => i,
            None => return JsonRpcResponse::invalid_params(id, format!("Workspace id {} not found", ws_id)),
        }
    } else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'id' or 'index' parameter");
    };

    if idx >= state.engine.workspaces.len() {
        return JsonRpcResponse::invalid_params(
            id,
            format!("Workspace index {} out of range (0..{})", idx, state.engine.workspaces.len()),
        );
    }

    let ws = &mut state.engine.workspaces[idx];
    if let Some(name) = params.get("name").and_then(|v| v.as_str()) {
        ws.name = name.to_string();
    }
    if let Some(subtitle) = params.get("subtitle").and_then(|v| v.as_str()) {
        ws.subtitle = subtitle.to_string();
    }
    if let Some(desc) = params.get("description").and_then(|v| v.as_str()) {
        ws.description = desc.to_string();
    }

    JsonRpcResponse::success(
        id,
        json!({
            "id": ws.id,
            "name": ws.name,
            "subtitle": ws.subtitle,
            "description": ws.description,
            "index": idx,
        }),
    )
}

pub fn handle_workspace_select(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let index = params
        .get("index")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize);
    match index {
        Some(idx) if idx < state.engine.workspaces.len() => {
            state.switch_workspace(idx);
            JsonRpcResponse::success(id, json!({ "active_workspace": idx }))
        }
        Some(idx) => JsonRpcResponse::invalid_params(
            id,
            format!("Workspace index {} out of range (0..{})", idx, state.engine.workspaces.len()),
        ),
        None => JsonRpcResponse::invalid_params(id, "Missing 'index' parameter"),
    }
}
