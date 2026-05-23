use serde_json::json;

use crate::ipc::protocol::JsonRpcResponse;
use crate::state::AppState;

use super::require_surface_id;

pub(crate) fn handle_surface_close(
    state: &mut AppState,
    engine: &mut crate::engine_state::EngineState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    // Prevent closing the caller's own surface — use 'close self' instead
    if let Some(caller) = super::caller_surface_id(params) {
        if caller == surface_id {
            return JsonRpcResponse::invalid_params(
                id,
                "Cannot close your own surface with 'close surface'. Use 'tasty close self' instead.",
            );
        }
    }
    let kind = state.surface_kind(engine, engine, engine, surface_id);
    if state.close_surface_by_id_no_snapshot(engine, engine, engine, surface_id) {
        if let Some(k) = kind {
            state.enqueue_surface_closed(surface_id, k, false);
        }
        JsonRpcResponse::success(id, json!({ "closed": true, "surface_id": surface_id }))
    } else {
        JsonRpcResponse::success(
            id,
            json!({ "closed": false, "surface_id": surface_id, "reason": "surface not found" }),
        )
    }
}

/// Close the calling surface itself. Only way for a surface to close itself.
pub(crate) fn handle_surface_close_self(
    state: &mut AppState,
    engine: &mut crate::engine_state::EngineState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    let kind = state.surface_kind(engine, engine, engine, surface_id);
    if state.close_surface_by_id_no_snapshot(engine, engine, engine, surface_id) {
        if let Some(k) = kind {
            state.enqueue_surface_closed(surface_id, k, false);
        }
        JsonRpcResponse::success(id, json!({ "closed": true, "surface_id": surface_id }))
    } else {
        JsonRpcResponse::success(
            id,
            json!({ "closed": false, "surface_id": surface_id, "reason": "surface not found" }),
        )
    }
}
