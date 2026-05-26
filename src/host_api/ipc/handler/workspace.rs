use serde_json::json;

use crate::ipc::protocol::JsonRpcResponse;
use crate::state::AppState;

pub fn handle_workspace_list(
    state: &AppState,
    engine: &crate::engine_state::EngineState,
    id: serde_json::Value,
) -> JsonRpcResponse {
    let workspaces: Vec<_> = engine
        .workspaces
        .iter()
        .enumerate()
        .map(|(i, ws)| {
            let sids = ws.all_surface_ids();
            json!({
                "id": ws.id,
                "name": ws.name,
                "subtitle": ws.subtitle,
                "description": ws.description,
                "active": i == state.active_workspace,
                "pane_count": ws.pane_layout().all_pane_ids().len(),
                "busy_count": engine.busy_count(&sids),
            })
        })
        .collect();
    JsonRpcResponse::success(id, json!(workspaces))
}

pub fn handle_workspace_create(
    state: &mut AppState,
    engine: &mut crate::engine_state::EngineState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let cwd = params
        .get("cwd")
        .and_then(|v| v.as_str())
        .map(std::path::PathBuf::from);
    let kind = params
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("terminal");

    // 필수 파라미터 검증 (registry create 함수도 검사하지만, 명확한 에러 메시지를 위해 선검증)
    match kind {
        "markdown" => {
            if params
                .get("file")
                .and_then(|v| v.as_str())
                .map(str::is_empty)
                .unwrap_or(true)
            {
                return JsonRpcResponse::invalid_params(
                    id,
                    "Missing 'file' parameter for markdown type",
                );
            }
        }
        _ => {}
    }
    match state.add_workspace_background(engine, cwd, kind, params) {
        Ok(idx) => {
            let mut renamed_name: Option<String> = None;
            let mut renamed_subtitle: Option<String> = None;
            let mut renamed_description: Option<String> = None;
            if let Some(name) = params.get("name").and_then(|v| v.as_str()) {
                if !name.is_empty() {
                    engine.workspaces[idx].name = name.to_string();
                    renamed_name = Some(name.to_string());
                }
            }
            if let Some(subtitle) = params.get("subtitle").and_then(|v| v.as_str()) {
                engine.workspaces[idx].subtitle = subtitle.to_string();
                renamed_subtitle = Some(subtitle.to_string());
            }
            if let Some(desc) = params.get("description").and_then(|v| v.as_str()) {
                engine.workspaces[idx].description = desc.to_string();
                renamed_description = Some(desc.to_string());
            }
            engine.mark_layout_dirty();
            let workspace_id = engine.workspaces[idx].id;
            if renamed_name.is_some() || renamed_subtitle.is_some() || renamed_description.is_some()
            {
                state.enqueue_host_event(crate::state::PendingHostEvent::WorkspaceRenamed {
                    workspace_id,
                    name: renamed_name,
                    subtitle: renamed_subtitle,
                    description: renamed_description,
                    user_direct: false,
                });
            }
            let ws = &engine.workspaces[idx];
            let surface_id = {
                let pane_id = ws.focused_pane;
                ws.pane_layout()
                    .find_pane(pane_id)
                    .and_then(|pane| pane.tabs.get(pane.active_tab))
                    .and_then(|tab| tab.focused_surface_id())
            };
            JsonRpcResponse::success(
                id,
                json!({
                    "id": ws.id,
                    "name": ws.name,
                    "subtitle": ws.subtitle,
                    "description": ws.description,
                    "index": idx,
                    "surface_id": surface_id,
                }),
            )
        }
        Err(e) => JsonRpcResponse::internal_error(id, e.to_string()),
    }
}

pub fn handle_workspace_update(
    state: &mut AppState,
    engine: &mut crate::engine_state::EngineState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let idx = if let Some(i) = params.get("index").and_then(|v| v.as_u64()) {
        i as usize
    } else if let Some(ws_id) = params.get("id").and_then(|v| v.as_u64()) {
        match engine
            .workspaces
            .iter()
            .position(|ws| ws.id == ws_id as u32)
        {
            Some(i) => i,
            None => {
                return JsonRpcResponse::invalid_params(
                    id,
                    format!("Workspace id {} not found", ws_id),
                );
            }
        }
    } else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'id' or 'index' parameter");
    };

    if idx >= engine.workspaces.len() {
        return JsonRpcResponse::invalid_params(
            id,
            format!(
                "Workspace index {} out of range (0..{})",
                idx,
                engine.workspaces.len()
            ),
        );
    }

    let mut renamed_name: Option<String> = None;
    let mut renamed_subtitle: Option<String> = None;
    let mut renamed_description: Option<String> = None;
    let workspace_id;
    {
        let ws = &mut engine.workspaces[idx];
        workspace_id = ws.id;
        if let Some(name) = params.get("name").and_then(|v| v.as_str()) {
            ws.name = name.to_string();
            renamed_name = Some(name.to_string());
        }
        if let Some(subtitle) = params.get("subtitle").and_then(|v| v.as_str()) {
            ws.subtitle = subtitle.to_string();
            renamed_subtitle = Some(subtitle.to_string());
        }
        if let Some(desc) = params.get("description").and_then(|v| v.as_str()) {
            ws.description = desc.to_string();
            renamed_description = Some(desc.to_string());
        }
    }
    if renamed_name.is_some() || renamed_subtitle.is_some() || renamed_description.is_some() {
        state.enqueue_host_event(crate::state::PendingHostEvent::WorkspaceRenamed {
            workspace_id,
            name: renamed_name,
            subtitle: renamed_subtitle,
            description: renamed_description,
            user_direct: false,
        });
    }
    engine.mark_layout_dirty();
    let ws = &engine.workspaces[idx];
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

pub fn handle_workspace_move(
    state: &mut AppState,
    engine: &mut crate::engine_state::EngineState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let from = match params.get("from_index").and_then(|v| v.as_u64()) {
        Some(f) => f as usize,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'from_index' parameter"),
    };
    let to = match params.get("to_index").and_then(|v| v.as_u64()) {
        Some(t) => t as usize,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'to_index' parameter"),
    };

    let moved = state.move_workspace(engine, from, to);
    if moved {
        engine.mark_layout_dirty();
    }
    JsonRpcResponse::success(id, json!({ "moved": moved }))
}

// workspace.select removed: focus is user-only (shortcuts/clicks).
