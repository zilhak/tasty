//! `output.observe_*` IPC 핸들러. 모든 mutate / read 는 `Core` wrapper 를 거친다
//! (`core.observer_*`) — handler 는 *engine 직접 mutate 금지* (Phase D 원칙).

use std::path::PathBuf;

use serde_json::{Value, json};

use crate::core::Core;
use crate::output_observer::{ObserverError, ObserverSpec, SinkSpec};
use crate::state::AppState;
use tasty_ipc::protocol::JsonRpcResponse;

pub fn handle_observe_start(
    core: &mut Core,
    _state: &mut AppState,
    engine: &mut crate::core::CoreState,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let spec = match parse_spec(params) {
        Ok(s) => s,
        Err(e) => return JsonRpcResponse::invalid_params(id, e),
    };
    match core.observer_register(engine, spec) {
        Ok(observer_id) => {
            let info = core
                .observer_info(engine, observer_id)
                .expect("just-registered observer must exist");
            JsonRpcResponse::success(id, json!({ "observer_id": observer_id, "info": info }))
        }
        Err(e) => observer_error_to_response(id, e),
    }
}

pub fn handle_observe_stop(
    core: &mut Core,
    _state: &mut AppState,
    engine: &mut crate::core::CoreState,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let observer_id = match params.get("observer_id").and_then(|v| v.as_u64()) {
        Some(v) => v,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'observer_id'"),
    };
    match core.observer_unregister(engine, observer_id) {
        Ok(()) => JsonRpcResponse::success(id, json!({ "observer_id": observer_id })),
        Err(e) => observer_error_to_response(id, e),
    }
}

pub fn handle_observe_list(
    core: &Core,
    _state: &AppState,
    engine: &crate::core::CoreState,
    id: Value,
) -> JsonRpcResponse {
    let items = core.observer_list(engine);
    JsonRpcResponse::success(id, json!({ "observers": items }))
}

pub fn handle_observe_info(
    core: &Core,
    _state: &AppState,
    engine: &crate::core::CoreState,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let observer_id = match params.get("observer_id").and_then(|v| v.as_u64()) {
        Some(v) => v,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'observer_id'"),
    };
    match core.observer_info(engine, observer_id) {
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
        ObserverError::FileOpen(_) | ObserverError::ThreadSpawn(_) => {
            JsonRpcResponse::internal_error(id, e.to_string())
        }
    }
}

/// `observe.start` 의 실패가 **에이전트에게 도달**하는지 고정한다.
///
/// sink 스레드 spawn 실패는 한때 `.expect` 로 호스트 전체를 죽였다. 이제는
/// `ObserverError::ThreadSpawn` 으로 올라오며, 그 값이 응답으로 매핑되지 않으면
/// 에이전트는 실패를 영영 모른다
/// (`docs/adr/0117-window-and-modal-creation-failure-policy.md`).
#[cfg(test)]
mod observer_error_mapping_tests {
    use super::*;

    fn code_of(e: ObserverError) -> i32 {
        observer_error_to_response(Value::from(1), e)
            .error
            .expect("an ObserverError must map to a JSON-RPC error")
            .code
    }

    #[test]
    fn a_sink_thread_spawn_failure_maps_to_internal_error() {
        assert_eq!(code_of(ObserverError::ThreadSpawn("EAGAIN".into())), -32603);
    }

    #[test]
    fn thread_spawn_and_file_open_are_both_server_side_failures() {
        // 같은 함수 안에서 갈리던 비대칭(파일 열기는 에러 반환, spawn 은 패닉)이
        // 해소됐다 — 둘 다 같은 등급으로 보고된다.
        assert_eq!(
            code_of(ObserverError::ThreadSpawn("EAGAIN".into())),
            code_of(ObserverError::FileOpen("EACCES".into())),
        );
    }

    #[test]
    fn caller_side_mistakes_stay_invalid_params() {
        assert_eq!(code_of(ObserverError::InvalidPath("/x".into())), -32602);
    }

    #[test]
    fn the_spawn_failure_message_names_the_cause() {
        let msg = ObserverError::ThreadSpawn("EAGAIN".into()).to_string();
        assert!(msg.contains("EAGAIN"), "got: {msg}");
    }
}
