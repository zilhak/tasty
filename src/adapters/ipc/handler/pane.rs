use serde_json::json;

use crate::core::structural_exec::{self, Closed, SplitLevel, SplitOutcome, SplitRequest};
use crate::model::SplitDirection;
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
    window: &mut dyn crate::ipc::window_port::IpcWindow,
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

    match structural_exec::close_pane(core, window, engine, pane_id) {
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

/// 숫자 ID나 별칭으로 surface를 찾는다. 점유 검사도 같은 해석을 사용한다.
pub(super) fn resolve_surface_target(
    core: &crate::core::Core,
    params: &serde_json::Value,
) -> Option<u32> {
    let val = params.get("target_surface");
    let val = val?;
    if val.is_null() {
        return None;
    }
    // 범위 초과 값을 자르면 다른 surface를 가리킬 수 있으므로 거절한다.
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
        return core.with_memory(|m| {
            crate::surface_meta::SurfaceMetaStore::find_by_value(m, "nickname", s)
        });
    }
    None
}

pub fn handle_split(
    core: &mut crate::core::Core,
    window: &mut dyn crate::ipc::window_port::IpcWindow,
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
    match structural_exec::split(core, window, engine, req) {
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
