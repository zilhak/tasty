//! `telemetry.record` / `telemetry.record_batch` 핸들러.

use serde_json::{Value, json};

use crate::app::services::AppServices;
use tasty_ipc::caller::CallerContext;
use tasty_ipc::protocol::JsonRpcResponse;

use super::{TriggeredCap, build_event, now_ms, put_event, reserve_event_key, trigger_caps_in};
use crate::adapters::ipc::handler::telemetry_follow_up::{FollowUp, RssSample};

/// Agent가 보고한 RSS 이벤트는 이상 탐지에 넘긴다. Plugin의 RSS는 호스트가 직접 측정한다.
fn rss_self_report(ev: &tasty_telemetry::TelemetryEvent) -> Option<RssSample> {
    (ev.metric == tasty_telemetry::RSS_METRIC_NAME).then(|| RssSample {
        agent: ev.agent.clone(),
        bytes: ev.value.max(0.0) as u64,
        ts: ev.ts,
    })
}

/// 이벤트를 저장하고 상한을 재평가한다. 저장소 오류가 아닌 기록 거절은 그 이벤트에서 멈추고, 앞서
/// 저장한 이벤트는 남는다(하나씩 저장하던 때와 같다). cap 액션과 RSS 탐지는 commit 뒤 메인 루프가
/// 실행한다.
fn store_events(
    core: &AppServices,
    id: Value,
    events: Vec<(tasty_telemetry::TelemetryEvent, String)>,
    respond: impl FnOnce(&AppServices, Value, Vec<String>) -> JsonRpcResponse + Send + 'static,
) -> JsonRpcResponse {
    let bytes = events
        .iter()
        .map(|(ev, key)| key.len() + ev.metric.len() + ev.agent.len())
        .sum();
    let rss: Vec<Option<RssSample>> = events.iter().map(|(ev, _)| rss_self_report(ev)).collect();
    core.reply_unit(
        id,
        bytes,
        move |s| {
            let mut keys = Vec::with_capacity(events.len());
            let mut triggered: Vec<Vec<TriggeredCap>> = Vec::with_capacity(events.len());
            let mut warnings = Vec::new();
            for (ev, key) in &events {
                if let Err(e) = put_event(s, ev, key) {
                    return Ok(Err(e));
                }
                keys.push(key.clone());
            }
            for (ev, _) in &events {
                triggered.push(trigger_caps_in(s, ev, &mut warnings));
            }
            Ok(Ok((keys, triggered, warnings)))
        },
        move |core, id, result| match result {
            Ok(Ok((keys, triggered, warnings))) => {
                for warning in warnings {
                    tracing::warn!("{warning}");
                }
                for (triggered, rss) in triggered.into_iter().zip(rss) {
                    core.telemetry_follow_ups()
                        .push(FollowUp { triggered, rss });
                }
                respond(core, id, keys)
            }
            Ok(Err(e)) => JsonRpcResponse::error(id, -32603, e),
            Err(e) => crate::adapters::ipc::handler::memory::map_error_ref(id, e),
        },
    )
}

pub fn handle_record(
    core: &mut AppServices,
    window: &mut dyn crate::ipc::window_port::IpcWindow,
    engine: &mut crate::runtime::engine_access::EngineMut<'_>,
    caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let default_agent = caller.agent_id();
    let default_ws = engine
        .workspace_at(window.active_workspace_index(engine))
        .map(|ws| ws.id);
    let ts = now_ms();
    let ev = match build_event(params, default_agent.as_str(), default_ws, ts) {
        Ok(ev) => ev,
        Err(e) => return JsonRpcResponse::invalid_params(id, e),
    };
    let key = reserve_event_key(core, &ev);
    let body = json!({
        "key": key,
        "ts": ev.ts,
        "agent": ev.agent,
        "metric": ev.metric,
    });
    store_events(core, id, vec![(ev, key)], move |core, id, _keys| {
        crate::adapters::ipc::handler::memory::written(core, id, body)
    })
}

/// `telemetry.record_batch` — 여러 이벤트를 한 번에 기록.
///
/// 입력: `{ events: [<event-params>, ...] }`. 각 항목은 record 와 동일한 스키마.
/// 모든 이벤트는 동일한 호출 ts 를 공유하며, seq 만 단조 증가하여 정렬을 보장한다.
pub fn handle_record_batch(
    core: &mut AppServices,
    window: &mut dyn crate::ipc::window_port::IpcWindow,
    engine: &mut crate::runtime::engine_access::EngineMut<'_>,
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
        .workspace_at(window.active_workspace_index(engine))
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
    let events = events
        .into_iter()
        .map(|ev| {
            let key = reserve_event_key(core, &ev);
            (ev, key)
        })
        .collect();
    store_events(core, id, events, |core, id, keys| {
        crate::adapters::ipc::handler::memory::written(
            core,
            id,
            json!({
                "recorded": keys.len(),
                "keys": keys,
            }),
        )
    })
}
