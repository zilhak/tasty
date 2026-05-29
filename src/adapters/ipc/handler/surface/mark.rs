use serde_json::json;

use crate::ipc::protocol::JsonRpcResponse;
use crate::state::AppState;

use super::require_surface_id;

pub(crate) fn handle_set_mark(
    state: &mut AppState,
    engine: &mut crate::engine_state::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    let _ = engine; // handler 는 enqueue 만. cascade 가 적용.
    state.enqueue_core_intent(crate::core::intent::CoreIntent::SetTerminalMark { surface_id });
    JsonRpcResponse::success(id, json!({ "ok": true, "surface_id": surface_id }))
}

pub(crate) fn handle_read_since_mark(
    state: &mut AppState,
    engine: &mut crate::engine_state::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };

    let strip_ansi = params
        .get("strip_ansi")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let text = state.read_since_mark(engine, Some(surface_id), strip_ansi);
    JsonRpcResponse::success(id, json!({ "text": text, "surface_id": surface_id }))
}

/// `surface.parse_since_mark` — read_since_mark 결과를 `tasty-output` 빌트인
/// 파서들로 분해. `parsers` 가 생략되면 `DEFAULT_PARSER_IDS` 사용. `prompt_boundary`
/// /`exit_code` 같이 ANSI escape 자체가 의미인 파서를 쓸 수 있도록 raw 텍스트
/// (strip_ansi=false) 를 항상 입력으로 한다.
pub(crate) fn handle_parse_since_mark(
    state: &mut AppState,
    engine: &mut crate::engine_state::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };

    let parser_ids: Vec<String> = match params.get("parsers") {
        Some(serde_json::Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
        Some(serde_json::Value::String(s)) => s
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        _ => tasty_output::DEFAULT_PARSER_IDS
            .iter()
            .map(|s| s.to_string())
            .collect(),
    };

    let text = state.read_since_mark(engine, Some(surface_id), false);
    let items = match tasty_output::parse_buffer(&text, parser_ids.iter().map(String::as_str)) {
        Ok(v) => v,
        Err(unknown) => {
            return JsonRpcResponse::invalid_params(id, format!("unknown parser: '{unknown}'"));
        }
    };

    JsonRpcResponse::success(
        id,
        json!({
            "surface_id": surface_id,
            "parsers": parser_ids,
            "items": items,
        }),
    )
}
