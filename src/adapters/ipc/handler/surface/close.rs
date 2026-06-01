use serde_json::json;

use crate::ipc::protocol::JsonRpcResponse;
use crate::state::AppState;

use super::require_surface_id;

/// 공용 close 본문 — IPC handle_surface_close / handle_surface_close_self 가
/// 공유한다. save_snapshot=false (Agent), auto-recreate empty workspace.
fn close_surface_via_intent(
    core: &mut crate::core::Core,
    state: &mut AppState,
    engine: &mut crate::engine_state::CoreState,
    id: serde_json::Value,
    surface_id: u32,
) -> JsonRpcResponse {
    let kind = state.surface_kind(engine, surface_id);
    let intent = crate::core::intent::DomainIntent::CloseSurface {
        surface_id,
        save_snapshot: false,
    };
    let events = match core.apply(engine, intent) {
        Ok(events) => events,
        Err(e) => return JsonRpcResponse::internal_error(id, e.to_string()),
    };
    let Some(crate::core::intent::CoreEvent::SurfaceClosed {
        surface_id,
        closed,
        cascade_level,
        cleanup_targets,
        closed_tab_ids,
        closed_pane_ids,
        workspace_id_purged,
        workspaces_now_empty,
    }) = events.into_iter().next()
    else {
        return JsonRpcResponse::internal_error(id, "Core::apply returned no SurfaceClosed event");
    };

    if !closed {
        return JsonRpcResponse::success(
            id,
            json!({ "closed": false, "surface_id": surface_id, "reason": "surface not found" }),
        );
    }

    crate::app::dispatch_domain::cascade_surface_closed(
        core,
        state,
        engine,
        cascade_level,
        cleanup_targets,
        closed_tab_ids,
        closed_pane_ids,
        workspace_id_purged,
        workspaces_now_empty,
    );

    if let Some(k) = kind {
        state.enqueue_surface_closed(surface_id, k, false);
    }

    JsonRpcResponse::success(id, json!({ "closed": true, "surface_id": surface_id }))
}

pub(crate) fn handle_surface_close(
    core: &mut crate::core::Core,
    state: &mut AppState,
    engine: &mut crate::engine_state::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    // Prevent closing the caller's own surface — use 'close self' instead.
    if let Some(caller) = super::caller_surface_id(params) {
        if caller == surface_id {
            return JsonRpcResponse::invalid_params(
                id,
                "Cannot close your own surface with 'close surface'. Use 'tasty close self' instead.",
            );
        }
    }
    close_surface_via_intent(core, state, engine, id, surface_id)
}

/// Close the calling surface itself. Only way for a surface to close itself.
pub(crate) fn handle_surface_close_self(
    core: &mut crate::core::Core,
    state: &mut AppState,
    engine: &mut crate::engine_state::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    close_surface_via_intent(core, state, engine, id, surface_id)
}
