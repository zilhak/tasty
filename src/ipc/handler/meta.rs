use serde_json::json;

use crate::ipc::protocol::JsonRpcResponse;
use crate::state::AppState;

use super::require_surface_id;

pub fn handle_surface_meta_set(
    _state: &mut AppState,
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
    crate::surface_meta::SurfaceMetaStore::set(surface_id, key, value);
    JsonRpcResponse::success(id, json!({ "ok": true, "surface_id": surface_id }))
}

pub fn handle_surface_meta_get(
    _state: &mut AppState,
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
    let value = crate::surface_meta::SurfaceMetaStore::get(surface_id, key);
    JsonRpcResponse::success(id, json!({ "value": value, "surface_id": surface_id }))
}

pub fn handle_surface_meta_unset(
    _state: &mut AppState,
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
    crate::surface_meta::SurfaceMetaStore::unset(surface_id, key);
    JsonRpcResponse::success(id, json!({ "ok": true, "surface_id": surface_id }))
}

pub fn handle_surface_meta_list(
    _state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    let data = crate::surface_meta::SurfaceMetaStore::list(surface_id);
    JsonRpcResponse::success(id, json!({ "surface_id": surface_id, "data": data }))
}
