use serde_json::json;

use crate::ipc::protocol::JsonRpcResponse;
use crate::state::AppState;

pub fn handle_workspace_list(
    state: &AppState,
    engine: &crate::engine_state::CoreState,
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
    core: &mut crate::core::Core,
    state: &mut AppState,
    engine: &mut crate::engine_state::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let explicit_cwd = params
        .get("cwd")
        .and_then(|v| v.as_str())
        .map(std::path::PathBuf::from);
    let kind = params
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("terminal");

    // 필수 파라미터 검증 (registry create 함수도 검사하지만, 명확한 에러 메시지를 위해 선검증)
    if kind == "markdown"
        && params
            .get("file")
            .and_then(|v| v.as_str())
            .map(str::is_empty)
            .unwrap_or(true)
    {
        return JsonRpcResponse::invalid_params(id, "Missing 'file' parameter for markdown type");
    }

    // terminal 의 cwd inherit 은 호출자가 미리 결정해 payload 로 넘긴다 (Core 는
    // focus state 모름). 그 외 kind 는 cwd 미사용.
    let resolved_cwd = if kind == "terminal" {
        explicit_cwd.or_else(|| state.resolve_inherit_cwd(engine))
    } else {
        None
    };

    let name = params
        .get("name")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let subtitle = params
        .get("subtitle")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let description = params
        .get("description")
        .and_then(|v| v.as_str())
        .map(str::to_string);

    let intent = crate::core::intent::DomainIntent::CreateWorkspace {
        cwd: resolved_cwd,
        kind: kind.to_string(),
        surface_params: params.clone(),
        name,
        subtitle,
        description,
    };

    let events = match core.apply(engine, intent) {
        Ok(events) => events,
        Err(e) => return JsonRpcResponse::internal_error(id, e.to_string()),
    };

    // events 안에 정확히 하나의 WorkspaceCreated 가 들어있다.
    let Some(crate::core::intent::CoreEvent::WorkspaceCreated {
        id: workspace_id,
        index,
        surface_id,
        renamed_name,
        renamed_subtitle,
        renamed_description,
    }) = events.into_iter().next()
    else {
        return JsonRpcResponse::internal_error(
            id,
            "Core::apply returned no WorkspaceCreated event",
        );
    };

    // cascade: host event 발화 (rename 필드 있을 때). IPC 는 Agent origin 이므로
    // active 전환은 하지 않는다 (`cascade_workspace_created` 가 origin 보고 분기).
    let agent_origin = crate::intent::IntentOrigin::Agent {
        source: crate::intent::AgentSource::Ipc,
    };
    crate::app::dispatch_domain::cascade_workspace_created(
        state,
        engine,
        &agent_origin,
        workspace_id,
        index,
        0,
        surface_id,
        renamed_name.clone(),
        renamed_subtitle.clone(),
        renamed_description.clone(),
    );

    let ws = &engine.workspaces[index];
    JsonRpcResponse::success(
        id,
        json!({
            "id": ws.id,
            "name": ws.name,
            "subtitle": ws.subtitle,
            "description": ws.description,
            "index": index,
            "surface_id": surface_id,
        }),
    )
}

pub fn handle_workspace_update(
    core: &mut crate::core::Core,
    state: &mut AppState,
    engine: &mut crate::engine_state::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    // workspace_id resolve — `id` 우선, 없으면 `index` 로 lookup.
    let workspace_id = if let Some(ws_id) = params.get("id").and_then(|v| v.as_u64()) {
        ws_id as u32
    } else if let Some(i) = params.get("index").and_then(|v| v.as_u64()) {
        let idx = i as usize;
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
        engine.workspaces[idx].id
    } else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'id' or 'index' parameter");
    };

    let name = params
        .get("name")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let subtitle = params
        .get("subtitle")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let description = params
        .get("description")
        .and_then(|v| v.as_str())
        .map(str::to_string);

    let intent = crate::core::intent::DomainIntent::UpdateWorkspaceMeta {
        workspace_id,
        name,
        subtitle,
        description,
    };

    let events = match core.apply(engine, intent) {
        Ok(events) => events,
        Err(e) => return JsonRpcResponse::invalid_params(id, e.to_string()),
    };

    let Some(crate::core::intent::CoreEvent::WorkspaceMetaUpdated {
        workspace_id,
        index,
        name,
        subtitle,
        description,
    }) = events.into_iter().next()
    else {
        return JsonRpcResponse::internal_error(
            id,
            "Core::apply returned no WorkspaceMetaUpdated event",
        );
    };

    crate::app::dispatch_domain::cascade_workspace_meta_updated(
        state,
        workspace_id,
        name,
        subtitle,
        description,
    );

    let ws = &engine.workspaces[index];
    JsonRpcResponse::success(
        id,
        json!({
            "id": ws.id,
            "name": ws.name,
            "subtitle": ws.subtitle,
            "description": ws.description,
            "index": index,
        }),
    )
}

pub fn handle_workspace_move(
    core: &mut crate::core::Core,
    state: &mut AppState,
    engine: &mut crate::engine_state::CoreState,
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

    let intent = crate::core::intent::DomainIntent::MoveWorkspace {
        from_index: from,
        to_index: to,
    };
    let events = match core.apply(engine, intent) {
        Ok(events) => events,
        Err(e) => return JsonRpcResponse::internal_error(id, e.to_string()),
    };

    let moved = matches!(
        events.iter().next(),
        Some(crate::core::intent::CoreEvent::WorkspaceMoved { moved: true, .. })
    );
    if moved {
        crate::app::dispatch_domain::cascade_workspace_moved(state, from, to);
    }
    JsonRpcResponse::success(id, json!({ "moved": moved }))
}

// workspace.select removed: focus is user-only (shortcuts/clicks).
