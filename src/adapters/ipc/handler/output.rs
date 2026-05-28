//! `output.observe_*` IPC 핸들러. 옵저버 라우터 (`engine.observer_router`) 의
//! thin wrapper.

use std::path::PathBuf;

use serde_json::{Value, json};

use crate::ipc::protocol::JsonRpcResponse;
use crate::output_observer::{ObserverError, ObserverSpec, SinkSpec};
use crate::state::AppState;

pub fn handle_observe_start(
    _state: &mut AppState,
    engine: &mut crate::engine_state::CoreState,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let spec = match parse_spec(params) {
        Ok(s) => s,
        Err(e) => return JsonRpcResponse::invalid_params(id, e),
    };
    match engine.observer_router.register(spec) {
        Ok(observer_id) => {
            let info = engine
                .observer_router
                .info(observer_id)
                .expect("just-registered observer must exist");
            JsonRpcResponse::success(id, json!({ "observer_id": observer_id, "info": info }))
        }
        Err(e) => observer_error_to_response(id, e),
    }
}

pub fn handle_observe_stop(
    _state: &mut AppState,
    engine: &mut crate::engine_state::CoreState,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let observer_id = match params.get("observer_id").and_then(|v| v.as_u64()) {
        Some(v) => v,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'observer_id'"),
    };
    match engine.observer_router.unregister(observer_id) {
        Ok(()) => JsonRpcResponse::success(id, json!({ "observer_id": observer_id })),
        Err(e) => observer_error_to_response(id, e),
    }
}

pub fn handle_observe_list(
    _state: &AppState,
    engine: &crate::engine_state::CoreState,
    id: Value,
) -> JsonRpcResponse {
    let items = engine.observer_router.list();
    JsonRpcResponse::success(id, json!({ "observers": items }))
}

pub fn handle_observe_info(
    _state: &AppState,
    engine: &crate::engine_state::CoreState,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let observer_id = match params.get("observer_id").and_then(|v| v.as_u64()) {
        Some(v) => v,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'observer_id'"),
    };
    match engine.observer_router.info(observer_id) {
        Some(info) => JsonRpcResponse::success(id, json!(info)),
        None => observer_error_to_response(id, ObserverError::NotFound(observer_id)),
    }
}

fn parse_spec(params: &Value) -> Result<ObserverSpec, String> {
    let surface_id = params
        .get("surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);

    let parsers: Vec<String> = match params.get("parsers") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(arr)) => arr
            .iter()
            .map(|v| {
                v.as_str()
                    .map(|s| s.to_string())
                    .ok_or_else(|| "'parsers' entries must be strings".to_string())
            })
            .collect::<Result<_, _>>()?,
        Some(Value::String(s)) => s
            .split(',')
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .collect(),
        _ => {
            return Err(
                "'parsers' must be an array of strings or a comma-separated string".to_string(),
            );
        }
    };

    let kinds: Option<Vec<String>> = match params.get("kinds") {
        None | Some(Value::Null) => None,
        Some(Value::Array(arr)) => Some(
            arr.iter()
                .map(|v| {
                    v.as_str()
                        .map(|s| s.to_string())
                        .ok_or_else(|| "'kinds' entries must be strings".to_string())
                })
                .collect::<Result<_, _>>()?,
        ),
        Some(Value::String(s)) => Some(
            s.split(',')
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .collect(),
        ),
        _ => {
            return Err(
                "'kinds' must be an array of strings or comma-separated string".to_string(),
            );
        }
    };

    let sink_obj = params
        .get("sink")
        .ok_or_else(|| "Missing 'sink' object".to_string())?;
    let sink_type = sink_obj
        .get("type")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "'sink.type' must be 'memory' or 'file'".to_string())?;
    let sink = match sink_type {
        "memory" => {
            let max_records = sink_obj
                .get("max_records")
                .and_then(|v| v.as_u64())
                .map(|v| v as usize)
                .unwrap_or(10_000);
            SinkSpec::Memory { max_records }
        }
        "file" => {
            let path = sink_obj
                .get("path")
                .and_then(|v| v.as_str())
                .map(PathBuf::from);
            SinkSpec::File { path }
        }
        other => {
            return Err(format!(
                "unknown sink type '{other}' (expected 'memory' or 'file')"
            ));
        }
    };

    Ok(ObserverSpec {
        surface_id,
        parsers,
        kinds,
        sink,
    })
}

fn observer_error_to_response(id: Value, e: ObserverError) -> JsonRpcResponse {
    match &e {
        ObserverError::UnknownParser(_) | ObserverError::InvalidPath(_) => {
            JsonRpcResponse::invalid_params(id, e.to_string())
        }
        ObserverError::NotFound(_) => JsonRpcResponse::invalid_params(id, e.to_string()),
        ObserverError::FileOpen(_) => JsonRpcResponse::internal_error(id, e.to_string()),
    }
}
