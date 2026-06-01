use serde_json::json;

use crate::ipc::protocol::JsonRpcResponse;
use crate::model::SplitDirection;
use crate::state::AppState;

use super::{apply_meta, require_pane_id};

pub fn handle_pane_list(
    _state: &AppState,
    engine: &crate::engine_state::CoreState,
    id: serde_json::Value,
) -> JsonRpcResponse {
    let mut panes = Vec::new();
    for ws in &engine.workspaces {
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

pub fn handle_pane_close(
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

    if let Some(caller) = super::caller_surface_id(params) {
        if super::surface_belongs_to_pane(engine, caller, pane_id) {
            return JsonRpcResponse::invalid_params(
                id,
                "Cannot close a pane that contains your own surface. Close all other surfaces in the pane first, then use 'tasty close self'.",
            );
        }
    }

    if engine.find_pane_by_id(pane_id).is_none() {
        return JsonRpcResponse::invalid_params(id, format!("Pane {} not found", pane_id));
    }

    let intent = crate::core::intent::DomainIntent::ClosePane { pane_id };
    let events = match core.apply(engine, intent) {
        Ok(events) => events,
        Err(e) => return JsonRpcResponse::internal_error(id, e.to_string()),
    };
    let Some(crate::core::intent::CoreEvent::PaneClosed {
        pane_id,
        closed,
        cleanup_targets,
    }) = events.into_iter().next()
    else {
        return JsonRpcResponse::internal_error(id, "Core::apply returned no PaneClosed event");
    };

    if closed {
        for (sid, pid) in cleanup_targets {
            state.cleanup_surface(engine, sid, pid);
        }
        JsonRpcResponse::success(id, json!({ "closed": true, "pane_id": pane_id }))
    } else {
        JsonRpcResponse::success(
            id,
            json!({ "closed": false, "pane_id": pane_id, "reason": "cannot close the last pane" }),
        )
    }
}

/// Resolve a surface target from params.
/// Supports numeric ID and nickname string.
fn resolve_surface_target(state: &AppState, params: &serde_json::Value) -> Option<u32> {
    let val = params.get("target_surface");
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
        return crate::surface_meta::SurfaceMetaStore::find_by_value(&state.memory, "nickname", s);
    }
    None
}

pub fn handle_split(
    core: &mut crate::core::Core,
    state: &mut AppState,
    engine: &mut crate::engine_state::CoreState,
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
            );
        }
        None => return JsonRpcResponse::invalid_params(id, "Missing 'level' parameter"),
    };

    let direction = match params.get("direction").and_then(|v| v.as_str()) {
        Some("horizontal") | Some("h") => SplitDirection::Horizontal,
        _ => SplitDirection::Vertical,
    };

    let target_surface_id = resolve_surface_target(state, params);
    let target_pane_id = params
        .get("target_pane")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);

    // Validate: at least one target must be specified
    if target_surface_id.is_none() && target_pane_id.is_none() {
        return JsonRpcResponse::invalid_params(
            id,
            "Missing target. Use 'target_surface' (surface ID or nickname) and/or 'target_pane' (pane ID)",
        );
    }
    // Validate: can't specify both
    if target_surface_id.is_some() && target_pane_id.is_some() {
        return JsonRpcResponse::invalid_params(
            id,
            "Cannot specify both 'target_surface' and 'target_pane'. Use one.",
        );
    }

    let meta = params.get("meta").and_then(|v| v.as_object());
    let cwd = params
        .get("cwd")
        .and_then(|v| v.as_str())
        .map(std::path::PathBuf::from);
    let kind = params
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("terminal");

    // 필수 파라미터 선검증
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

    match level {
        "pane" => {
            // pane-level split: target_pane 또는 target_surface 로 pane_id 결정 (IPC 명시).
            let resolved_pane_id = if let Some(pid) = target_pane_id {
                pid
            } else if let Some(sid) = target_surface_id {
                match engine.find_pane_for_surface(sid) {
                    Some(pid) => pid,
                    None => {
                        return JsonRpcResponse::invalid_params(
                            id,
                            format!("Surface {sid} not found"),
                        );
                    }
                }
            } else {
                return JsonRpcResponse::invalid_params(
                    id,
                    "pane-level split requires 'target_pane' or 'target_surface'",
                );
            };

            // terminal 의 cwd inherit — 호출자가 미리 결정 (Core 는 focus state 모름).
            let resolved_cwd = if kind == "terminal" {
                cwd.or_else(|| {
                    let sid = engine
                        .find_pane_by_id(resolved_pane_id)
                        .and_then(|p| p.tabs.get(p.active_tab))
                        .and_then(|t| t.focused_surface_id())?;
                    state.resolve_inherit_cwd_from_surface(engine, sid)
                })
            } else {
                None
            };

            let intent = crate::core::intent::DomainIntent::SplitPane {
                target_pane_id: resolved_pane_id,
                direction,
                cwd: resolved_cwd,
                kind: kind.to_string(),
                surface_params: params.clone(),
            };
            let events = match core.apply(engine, intent) {
                Ok(events) => events,
                Err(e) => return JsonRpcResponse::internal_error(id, e.to_string()),
            };
            let Some(crate::core::intent::CoreEvent::PaneSplit {
                workspace_index,
                original_pane_id,
                new_pane_id,
                new_surface_id,
                direction,
            }) = events.into_iter().next()
            else {
                return JsonRpcResponse::internal_error(
                    id,
                    "Core::apply returned no PaneSplit event",
                );
            };

            // IPC = Agent origin — focus 이동 안 함.
            let agent_origin = crate::intent::IntentOrigin::Agent {
                source: crate::intent::AgentSource::Ipc,
            };
            crate::app::dispatch_domain::cascade_pane_split(
                state,
                engine,
                &agent_origin,
                workspace_index,
                original_pane_id,
                new_pane_id,
                new_surface_id,
                direction,
            );

            apply_meta(state, new_surface_id, meta);
            JsonRpcResponse::success(
                id,
                json!({
                    "new_pane_id": new_pane_id,
                    "new_surface_id": new_surface_id,
                }),
            )
        }
        "surface" => {
            let sid = match target_surface_id {
                Some(sid) => sid,
                None => {
                    return JsonRpcResponse::invalid_params(
                        id,
                        "Surface-level split requires 'target_surface', not 'target_pane'",
                    );
                }
            };

            // terminal cwd inherit — 호출자가 미리 결정.
            let resolved_cwd = if kind == "terminal" {
                cwd.or_else(|| state.resolve_inherit_cwd_from_surface(engine, sid))
            } else {
                None
            };

            let intent = crate::core::intent::DomainIntent::SplitSurface {
                target_surface_id: sid,
                direction,
                cwd: resolved_cwd,
                kind: kind.to_string(),
                surface_params: params.clone(),
            };
            let events = match core.apply(engine, intent) {
                Ok(events) => events,
                Err(e) => return JsonRpcResponse::internal_error(id, e.to_string()),
            };
            let Some(crate::core::intent::CoreEvent::SurfaceSplit {
                workspace_index,
                pane_id,
                target_surface_id: _,
                new_surface_id,
            }) = events.into_iter().next()
            else {
                return JsonRpcResponse::internal_error(
                    id,
                    "Core::apply returned no SurfaceSplit event",
                );
            };

            // IPC = Agent — focus 이동 안 함.
            let agent_origin = crate::intent::IntentOrigin::Agent {
                source: crate::intent::AgentSource::Ipc,
            };
            crate::app::dispatch_domain::cascade_surface_split(
                state,
                engine,
                &agent_origin,
                workspace_index,
                pane_id,
                new_surface_id,
            );

            apply_meta(state, new_surface_id, meta);
            JsonRpcResponse::success(
                id,
                json!({
                    "new_surface_id": new_surface_id,
                }),
            )
        }
        _ => unreachable!(),
    }
}

// focus.direction removed: focus is user-only (shortcuts/clicks).
