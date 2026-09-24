//! `telemetry.record` / `telemetry.record_batch` 핸들러.

use serde_json::{Value, json};

use crate::core::Core;
use tasty_ipc::caller::CallerContext;
use tasty_ipc::protocol::JsonRpcResponse;

use super::{build_event, evaluate_caps_after_record, now_ms, persist_event, record_rss_sample};

/// Agent가 보고한 RSS 이벤트를 이상 탐지에 전달한다. Plugin의 RSS는 호스트가 직접 측정한다.
fn detect_rss_self_report(
    core: &Core,
    window: &mut dyn crate::ipc::window_port::IpcWindow,
    out: &mut crate::ipc::window_port::IntentOutbox,
    engine: &mut crate::core::CoreState,
    ev: &tasty_telemetry::TelemetryEvent,
) {
    if ev.metric != tasty_telemetry::RSS_METRIC_NAME {
        return;
    }
    record_rss_sample(
        core,
        window,
        out,
        engine,
        &ev.agent,
        ev.value.max(0.0) as u64,
        ev.ts,
    );
}

pub fn handle_record(
    core: &mut Core,
    window: &mut dyn crate::ipc::window_port::IpcWindow,
    out: &mut crate::ipc::window_port::IntentOutbox,
    engine: &mut crate::core::CoreState,
    caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let default_agent = caller.agent_id();
    let default_ws = engine
        .workspaces
        .get(window.active_workspace_index())
        .map(|ws| ws.id);
    let ts = now_ms();
    let ev = match build_event(params, default_agent.as_str(), default_ws, ts) {
        Ok(ev) => ev,
        Err(e) => return JsonRpcResponse::invalid_params(id, e),
    };
    let response = match persist_event(core, engine, &ev) {
        Ok(key) => crate::adapters::ipc::handler::memory::written(
            core,
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
    evaluate_caps_after_record(core, window, out, engine, &ev);
    detect_rss_self_report(core, window, out, engine, &ev);
    response
}

/// `telemetry.record_batch` — 여러 이벤트를 한 번에 기록.
///
/// 입력: `{ events: [<event-params>, ...] }`. 각 항목은 record 와 동일한 스키마.
/// 모든 이벤트는 동일한 호출 ts 를 공유하며, seq 만 단조 증가하여 정렬을 보장한다.
pub fn handle_record_batch(
    core: &mut Core,
    window: &mut dyn crate::ipc::window_port::IpcWindow,
    out: &mut crate::ipc::window_port::IntentOutbox,
    engine: &mut crate::core::CoreState,
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
        .get(window.active_workspace_index())
        .map(|ws| ws.id);
    let ts = now_ms();
    // 잘못된 입력으로 일부만 기록되지 않도록 전체 입력을 먼저 검사한다.
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
        match persist_event(core, engine, ev) {
            Ok(k) => keys.push(k),
            Err(e) => return JsonRpcResponse::error(id, -32603, e),
        }
    }
    for ev in &events {
        evaluate_caps_after_record(core, window, out, engine, ev);
        detect_rss_self_report(core, window, out, engine, ev);
    }
    crate::adapters::ipc::handler::memory::written(
        core,
        id,
        json!({
            "recorded": keys.len(),
            "keys": keys,
        }),
    )
}
