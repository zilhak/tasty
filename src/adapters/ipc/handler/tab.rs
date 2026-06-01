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
    _state: &AppState,
    engine: &crate::engine_state::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let pane_id = match require_pane_id(params, &id) {
        Ok(pid) => pid,
        Err(e) => return e,
    };
    let tabs: Vec<_> = if let Some(pane) = engine.find_pane_by_id(pane_id) {
        pane.tabs
            .iter()
            .enumerate()
            .map(|(i, tab)| {
                let surface = tab.surface();
                let surface_type = surface.type_name();
                let surface_id = surface.surface_id();
                let sids = tab.all_surface_ids();
                let mut entry = json!({
                    "id": tab.id,
                    "name": tab.name,
                    "active": i == pane.active_tab,
                    "type": surface_type,
                    "busy_count": engine.busy_count(&sids),
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
    core: &mut crate::core::Core,
    state: &mut AppState,
    engine: &mut crate::engine_state::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let pane_id = match require_pane_id(params, &id) {
        Ok(pid) => pid,
        Err(e) => return e,
    };

    if engine.find_pane_by_id(pane_id).is_none() {
        return JsonRpcResponse::invalid_params(id, format!("Pane {} not found", pane_id));
    }

    let surface_type = params
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("terminal");

    // explorer는 path 미지정 시 home으로 보정 (Core 가 home dir 정책 모름 — handler 잔존).
    let mut params = params.clone();
    if surface_type == "explorer" && params.get("path").and_then(|v| v.as_str()).is_none() {
        let home = directories::BaseDirs::new()
            .map(|d| d.home_dir().to_string_lossy().to_string())
            .unwrap_or_else(|| ".".to_string());
        if let Some(obj) = params.as_object_mut() {
            obj.insert("path".into(), serde_json::Value::String(home));
        } else {
            params = serde_json::json!({ "path": home });
        }
    }

    // cwd resolve — terminal 만. explicit > pane active surface 의 inherit.
    let cwd = if surface_type == "terminal" {
        let explicit = params
            .get("cwd")
            .and_then(|v| v.as_str())
            .map(std::path::PathBuf::from);
        explicit.or_else(|| {
            let sid = engine
                .find_pane_by_id(pane_id)
                .and_then(|p| p.tabs.get(p.active_tab))
                .and_then(|t| t.focused_surface_id())?;
            state.resolve_inherit_cwd_from_surface(engine, sid)
        })
    } else {
        None
    };

    let intent = crate::core::intent::DomainIntent::CreateTab {
        pane_id,
        cwd,
        kind: surface_type.to_string(),
        surface_params: params,
    };
    let events = match core.apply(engine, intent) {
        Ok(events) => events,
        Err(e) => return JsonRpcResponse::internal_error(id, e.to_string()),
    };

    let Some(crate::core::intent::CoreEvent::TabCreated {
        pane_id,
        tab_count,
        active_tab,
        ..
    }) = events.into_iter().next()
    else {
        return JsonRpcResponse::internal_error(id, "Core::apply returned no TabCreated event");
    };

    JsonRpcResponse::success(
        id,
        json!({
            "pane_id": pane_id,
            "tab_count": tab_count,
            "active_tab": active_tab,
        }),
    )
}

pub fn handle_tab_close(
    core: &mut crate::core::Core,
    state: &mut AppState,
    engine: &mut crate::engine_state::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let tab_id = match require_tab_id(params, &id) {
        Ok(tid) => tid,
        Err(e) => return e,
    };

    // Prevent closing a tab that contains the caller — handler 잔존 (caller 정보 params).
    if let Some(caller) = super::caller_surface_id(params) {
        if let Some(pane_id) = engine.find_pane_for_tab(tab_id) {
            if super::surface_belongs_to_pane(engine, caller, pane_id) {
                return JsonRpcResponse::invalid_params(
                    id,
                    "Cannot close a tab that contains your own surface. Use 'tasty close self' instead.",
                );
            }
        }
    }

    let intent = crate::core::intent::DomainIntent::CloseTab { tab_id };
    let events = match core.apply(engine, intent) {
        Ok(events) => events,
        Err(e) => return JsonRpcResponse::internal_error(id, e.to_string()),
    };

    let Some(crate::core::intent::CoreEvent::TabClosed {
        tab_id,
        closed,
        cleanup_targets,
        ..
    }) = events.into_iter().next()
    else {
        return JsonRpcResponse::internal_error(id, "Core::apply returned no TabClosed event");
    };

    if closed {
        for (sid, pid) in cleanup_targets {
            state.cleanup_surface(engine, sid, pid);
        }
        JsonRpcResponse::success(id, json!({ "closed": true, "tab_id": tab_id }))
    } else {
        JsonRpcResponse::success(
            id,
            json!({
                "closed": false,
                "tab_id": tab_id,
                "reason": "tab not found or cannot close the last tab",
            }),
        )
    }
}

pub fn handle_tab_move(
    core: &mut crate::core::Core,
    _state: &mut AppState,
    engine: &mut crate::engine_state::CoreState,
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

    let intent = crate::core::intent::DomainIntent::MoveTab {
        pane_id,
        from_index: from,
        to_index: to,
    };
    let events = match core.apply(engine, intent) {
        Ok(events) => events,
        Err(e) => return JsonRpcResponse::internal_error(id, e.to_string()),
    };
    let moved = matches!(
        events.into_iter().next(),
        Some(crate::core::intent::CoreEvent::TabMoved { moved: true, .. })
    );
    JsonRpcResponse::success(id, json!({ "moved": moved, "pane_id": pane_id }))
}

// handle_open_markdown / handle_open_explorer removed: use handle_tab_create with type parameter
