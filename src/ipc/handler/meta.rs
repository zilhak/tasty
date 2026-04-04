use serde_json::json;

use crate::ipc::protocol::JsonRpcResponse;
use crate::state::AppState;

pub fn handle_surface_meta_set(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = params
        .get("surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .or_else(|| state.focused_surface_id())
        .unwrap_or(0);
    let key = match params.get("key").and_then(|v| v.as_str()) {
        Some(k) => k,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'key' parameter"),
    };
    let value = match params.get("value").and_then(|v| v.as_str()) {
        Some(v) => v,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'value' parameter"),
    };
    crate::surface_meta::SurfaceMetaStore::set(surface_id, key, value);
    JsonRpcResponse::success(id, json!({ "ok": true }))
}

pub fn handle_surface_meta_get(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = params
        .get("surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .or_else(|| state.focused_surface_id())
        .unwrap_or(0);
    let key = match params.get("key").and_then(|v| v.as_str()) {
        Some(k) => k,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'key' parameter"),
    };
    let value = crate::surface_meta::SurfaceMetaStore::get(surface_id, key);
    JsonRpcResponse::success(id, json!({ "value": value }))
}

pub fn handle_surface_meta_unset(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = params
        .get("surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .or_else(|| state.focused_surface_id())
        .unwrap_or(0);
    let key = match params.get("key").and_then(|v| v.as_str()) {
        Some(k) => k,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'key' parameter"),
    };
    crate::surface_meta::SurfaceMetaStore::unset(surface_id, key);
    JsonRpcResponse::success(id, json!({ "ok": true }))
}

pub fn handle_surface_meta_list(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = params
        .get("surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .or_else(|| state.focused_surface_id())
        .unwrap_or(0);
    let data = crate::surface_meta::SurfaceMetaStore::list(surface_id);
    JsonRpcResponse::success(id, json!(data))
}
