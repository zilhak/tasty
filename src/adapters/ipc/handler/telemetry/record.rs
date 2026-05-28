//! `telemetry.record` / `telemetry.record_batch` 핸들러.

use serde_json::{Value, json};

use crate::ipc::caller::CallerContext;
use crate::ipc::protocol::JsonRpcResponse;
use crate::state::AppState;

use super::{build_event, evaluate_caps_after_record, now_ms, persist_event};

pub fn handle_record(
    state: &mut AppState,
    engine: &mut crate::engine_state::EngineState,
    caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let default_agent = caller.agent_id();
    let default_ws = engine
        .workspaces
        .get(state.active_workspace)
        .map(|ws| ws.id);
    let ts = now_ms();
    let ev = match build_event(params, default_agent.as_str(), default_ws, ts) {
        Ok(ev) => ev,
        Err(e) => return JsonRpcResponse::invalid_params(id, e),
    };
    let response = match persist_event(engine, &ev) {
        Ok(key) => JsonRpcResponse::success(
            id,
            json!({
                "key": key,
                "ts": ev.ts,
                "agent": ev.agent,
                "metric": ev.metric,
            }),
        ),
        Err(e) => return JsonRpcResponse::error(id, -32603, e),
    };
    evaluate_caps_after_record(state, engine, &ev);
    response
}

/// `telemetry.record_batch` — 여러 이벤트를 한 번에 기록.
///
/// 입력: `{ events: [<event-params>, ...] }`. 각 항목은 record 와 동일한 스키마.
/// 모든 이벤트는 동일한 호출 ts 를 공유하며, seq 만 단조 증가하여 정렬을 보장한다.
pub fn handle_record_batch(
    state: &mut AppState,
    engine: &mut crate::engine_state::EngineState,
    caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let Some(Value::Array(arr)) = params.get("events") else {
        return JsonRpcResponse::invalid_params(id, "'events' must be an array");
    };
    if arr.is_empty() {
        return JsonRpcResponse::success(id, json!({ "recorded": 0, "keys": [] }));
    }
    let default_agent = caller.agent_id();
    let default_ws = engine
        .workspaces
        .get(state.active_workspace)
        .map(|ws| ws.id);
    let ts = now_ms();
    // pre-validate all → 부분 실패 방지.
    let mut events = Vec::with_capacity(arr.len());
    for (i, item) in arr.iter().enumerate() {
        match build_event(item, default_agent.as_str(), default_ws, ts) {
            Ok(ev) => events.push(ev),
            Err(e) => {
                return JsonRpcResponse::invalid_params(id, format!("events[{i}]: {e}"));
            }
        }
    }
    let mut keys = Vec::with_capacity(events.len());
    for ev in &events {
        match persist_event(engine, ev) {
            Ok(k) => keys.push(k),
            Err(e) => return JsonRpcResponse::error(id, -32603, e),
        }
    }
    for ev in &events {
        evaluate_caps_after_record(state, engine, ev);
    }
    JsonRpcResponse::success(
        id,
        json!({
            "recorded": keys.len(),
            "keys": keys,
        }),
    )
}

// ============================================================
// 조회 — memory prefix scan → pure aggregation
// ============================================================
