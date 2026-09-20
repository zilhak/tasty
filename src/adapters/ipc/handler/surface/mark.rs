use serde_json::json;

use crate::state::AppState;
use tasty_ipc::protocol::JsonRpcResponse;

use super::require_surface_id;

pub(crate) fn handle_set_mark(
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    let _ = engine; // handler 는 enqueue 만. cascade 가 적용.
    state.dispatch_intent(
        crate::core::intent::DomainIntent::SetTerminalMark { surface_id }.from_agent_ipc(),
    );
    JsonRpcResponse::success(id, json!({ "ok": true, "surface_id": surface_id }))
}

pub(crate) fn handle_read_since_mark(
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
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

/// `surface.read_since_scan_mark` — 출력 스캐너 전용 커서. 지난 호출 이후 새로 온
/// 출력만 돌려주고 커서를 그만큼 전진시킨다.
///
/// `surface.read_since_mark` 과 **다른 커서**를 쓴다. 같은 커서를 쓰면 에이전트의
/// `surface.set_mark` 이 스캐너의 관측 창을 밀고, 아무도 mark 를 안 세운 surface 에서는
/// 폴링마다 버퍼 전체가 다시 실린다
/// (`docs/adr/0307-the-output-scanner-reads-its-own-cursor.md`).
///
/// 읽기가 커서를 전진시키므로 **같은 구간을 두 번 받을 수 없다.** 그래서 이 이름의
/// 소비자는 하나라는 전제 위에 있다 — 둘이 부르면 서로의 바이트를 먹고 그 손실은
/// 조용하다. 그 전제가 위 ADR 의 재검토 조건이다.
pub(crate) fn handle_read_since_scan_mark(
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
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

    let text = state.take_since_output_scan_mark(engine, surface_id, strip_ansi);
    JsonRpcResponse::success(id, json!({ "text": text, "surface_id": surface_id }))
}

/// `surface.parse_since_mark` — read_since_mark 결과를 `tasty-output` 빌트인
/// 파서들로 분해. `parsers` 가 생략되면 `DEFAULT_PARSER_IDS` 사용. `prompt_boundary`
/// /`exit_code` 같이 ANSI escape 자체가 의미인 파서를 쓸 수 있도록 raw 텍스트
/// (strip_ansi=false) 를 항상 입력으로 한다.
pub(crate) fn handle_parse_since_mark(
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
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
