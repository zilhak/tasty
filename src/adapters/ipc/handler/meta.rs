use serde_json::json;

use crate::ipc::protocol::JsonRpcResponse;
use crate::state::AppState;

use super::require_surface_id;

pub fn handle_surface_meta_set(
    state: &mut AppState,
    _engine: &mut crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    let key = match params.get("key").and_then(|v| v.as_str()) {
        Some(k) => k,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'key' parameter"),
    };
    let value = match params.get("value").and_then(|v| v.as_str()) {
        Some(v) => v,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'value' parameter"),
    };
    let result = state
        .with_memory(|m| crate::surface_meta::SurfaceMetaStore::set(m, surface_id, key, value));
    if let Err(e) = result {
        return JsonRpcResponse::internal_error(id, format!("surface meta set failed: {e}"));
    }
    JsonRpcResponse::success(id, json!({ "ok": true, "surface_id": surface_id }))
}

pub fn handle_surface_meta_get(
    state: &mut AppState,
    _engine: &mut crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    let key = match params.get("key").and_then(|v| v.as_str()) {
        Some(k) => k,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'key' parameter"),
    };
    let value =
        state.with_memory(|m| crate::surface_meta::SurfaceMetaStore::get(m, surface_id, key));
    JsonRpcResponse::success(id, json!({ "value": value, "surface_id": surface_id }))
}

pub fn handle_surface_meta_unset(
    state: &mut AppState,
    _engine: &mut crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    let key = match params.get("key").and_then(|v| v.as_str()) {
        Some(k) => k,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'key' parameter"),
    };
    let result =
        state.with_memory(|m| crate::surface_meta::SurfaceMetaStore::unset(m, surface_id, key));
    if let Err(e) = result {
        return JsonRpcResponse::internal_error(id, format!("surface meta unset failed: {e}"));
    }
    JsonRpcResponse::success(id, json!({ "ok": true, "surface_id": surface_id }))
}

pub fn handle_surface_meta_list(
    state: &mut AppState,
    _engine: &mut crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    let data = state.with_memory(|m| crate::surface_meta::SurfaceMetaStore::list(m, surface_id));
    JsonRpcResponse::success(id, json!({ "surface_id": surface_id, "data": data }))
}
