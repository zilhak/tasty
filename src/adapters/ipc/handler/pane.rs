use serde_json::json;

use crate::core::structural_exec::{self, Closed, SplitLevel, SplitOutcome, SplitRequest};
use crate::model::SplitDirection;
use crate::state::AppState;
use tasty_ipc::protocol::JsonRpcResponse;

use super::require_pane_id;

pub fn handle_pane_list(engine: &crate::core::CoreState, id: serde_json::Value) -> JsonRpcResponse {
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
    engine: &mut crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let pane_id = match require_pane_id(params, &id) {
        Ok(pid) => pid,
        Err(e) => return e,
    };

    if let Some(caller) = super::caller_surface_id(params)
        && super::surface_belongs_to_pane(engine, caller, pane_id)
    {
        return JsonRpcResponse::invalid_params(
            id,
            "Cannot close a pane that contains your own surface. Close all other surfaces in the pane first, then use 'tasty close self'.",
        );
    }

    match structural_exec::close_pane(core, state, engine, pane_id) {
        Ok(Closed {
            id: pane_id,
            closed: true,
        }) => JsonRpcResponse::success(id, json!({ "closed": true, "pane_id": pane_id })),
        Ok(Closed { id: pane_id, .. }) => JsonRpcResponse::success(
            id,
            json!({ "closed": false, "pane_id": pane_id, "reason": "cannot close the last pane" }),
        ),
        Err(f) => super::structural_failure_response(id, f),
    }
}

/// Resolve a surface target from params.
/// Supports numeric ID and nickname string.
///
/// `pub(super)`: hard-occupied dispatch 가드(`handler.rs`)가 `split` 의 대상
/// workspace 를 판별할 때 이 해석 로직을 그대로 재사용한다(nickname 해석 중복 방지).
pub(super) fn resolve_surface_target(
    core: &crate::core::Core,
    params: &serde_json::Value,
) -> Option<u32> {
    let val = params.get("target_surface");
    let val = val?;
    if val.is_null() {
        return None;
    }
    // 자르지 않는다 — 잘린 값은 실재하는 다른 surface 를 가리킨다. 이 함수는 `Option`
    // 을 반환해 "대상 미지정" 과 합쳐지지만, 자르기만은 여기서 막는다(호출부가 대상
    // 없음으로 이어서 거절한다).
    if val.is_number() {
        return crate::adapters::ipc::handler::params::read_int::<u32>(params, "target_surface")
            .ok()
            .flatten();
    }
    if let Some(s) = val.as_str() {
        if s.is_empty() {
            return None;
        }
        if let Ok(n) = s.parse::<u32>() {
            return Some(n);
        }
        // Try nickname lookup
        return core.with_memory(|m| {
            crate::surface_meta::SurfaceMetaStore::find_by_value(m, "nickname", s)
        });
    }
    None
}

pub fn handle_split(
    core: &mut crate::core::Core,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let level = match params.get("level").and_then(|v| v.as_str()) {
        Some("pane-group") | Some("pane") => SplitLevel::Pane,
        Some("surface") => SplitLevel::Surface,
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

    let target_surface = resolve_surface_target(core, params);
    let target_pane = match super::params::optional_u32(params, "target_pane", &id) {
        Ok(v) => v,
        Err(e) => return e,
    };

    let req = SplitRequest {
        level,
        direction,
        target_surface,
        target_pane,
        params,
    };
    match structural_exec::split(core, state, engine, req) {
        Ok(SplitOutcome::Pane {
            new_pane_id,
            new_surface_id,
        }) => JsonRpcResponse::success(
            id,
            json!({
                "new_pane_id": new_pane_id,
                "new_surface_id": new_surface_id,
            }),
        ),
        Ok(SplitOutcome::Surface { new_surface_id }) => JsonRpcResponse::success(
            id,
            json!({
                "new_surface_id": new_surface_id,
            }),
        ),
        Err(f) => super::structural_failure_response(id, f),
    }
}

// focus.direction removed: focus is user-only (shortcuts/clicks).
