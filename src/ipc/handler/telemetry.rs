//! `telemetry.*` IPC 핸들러 — 메트릭 기록·조회.
//!
//! `tasty-telemetry` 도메인의 얇은 어댑터. 이벤트마다 [`tasty_memory`] 에
//! `tasty.telemetry.event.{ts:013}.{seq:04}` 키로 영속 (workspace_id 가
//! 있으면 `workspace:<wid>` scope, 없으면 `global`). 조회 핸들러는 prefix
//! scan + 도메인 pure aggregation 으로 응답을 만든다.
//!
//! 단계 4.1 범위:
//! - `telemetry.record` / `telemetry.record_batch` — 단일/배치 기록
//! - `telemetry.summary` — 집계 요약
//! - `telemetry.timeseries` — 윈도우 버킷 시계열 (raw events 에서 즉시 집계)
//! - `telemetry.top` — agent/workspace top-N
//!
//! 단계 4.2+ 에서 dispatcher 미들웨어가 자동으로 ipc.<method> 카운트를
//! 기록하기 시작한다.

use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value, json};
use tasty_memory::{ListOpts, MemoryValue, PutOpts, Scope, with_store};
use tasty_telemetry::{
    EVENT_KEY_PREFIX, Op, TelemetryEvent, Window, aggregate_into_buckets, event_key,
    summarize_events, top_n, validate_agent_id, validate_metric,
};

use crate::ipc::caller::CallerContext;
use crate::ipc::protocol::JsonRpcResponse;
use crate::state::AppState;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// dispatcher 미들웨어용 자동 카운트 (Phase 4.2).
///
/// caller 가 `_host` 이거나 method 가 `telemetry.` 로 시작하면 skip한다 —
/// 자기 자신을 측정하면 의미가 없고, telemetry 내부 호출은 재귀 폭주를 만든다.
/// 메트릭 이름은 `ipc_calls`, 메서드 식별자는 `method` 태그로 들어간다 (도메인
/// metric 검증이 `.` 을 허용하지 않으므로 `ipc_calls.<method>` 형태를 피한다).
///
/// 모든 실패는 best-effort. 호스트 stdout 의 IPC 정상 동작을 막지 않는다.
pub(crate) fn record_ipc_call(state: &AppState, caller: &CallerContext, method: &str) {
    if method.starts_with("telemetry.") {
        return;
    }
    let agent = caller.agent_id();
    if agent.is_host() {
        return;
    }
    let ws = state
        .engine
        .workspaces
        .get(state.active_workspace)
        .map(|w| w.id);
    let ts = now_ms();
    let ev = match TelemetryEvent::new(agent.as_str(), "ipc_calls", 1.0, Op::Inc, ts) {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!("telemetry middleware: build event failed: {e}");
            return;
        }
    };
    let mut ev = ev.with_tag("method", method);
    if let Some(w) = ws {
        ev = ev.with_workspace(w);
    }
    if let Err(e) = persist_event(state, &ev) {
        tracing::warn!("telemetry middleware: record failed: {e}");
    }
}

fn scope_for(workspace_id: Option<u32>) -> Scope {
    match workspace_id {
        Some(w) => Scope::Workspace(w),
        None => Scope::Global,
    }
}

fn parse_tags(v: Option<&Value>) -> std::result::Result<Vec<(String, String)>, String> {
    let Some(v) = v else { return Ok(Vec::new()) };
    let Value::Object(map) = v else {
        return Err("'tags' must be an object of string→string".into());
    };
    let mut out = Vec::with_capacity(map.len());
    for (k, val) in map {
        let s = val
            .as_str()
            .ok_or_else(|| format!("tag '{k}' must be a string"))?;
        out.push((k.clone(), s.to_string()));
    }
    Ok(out)
}

/// 한 이벤트 입력 파라미터를 도메인 객체로 빌드. caller agent 가 디폴트.
fn build_event(
    params: &Value,
    default_agent: &str,
    default_workspace_id: Option<u32>,
    ts: u64,
) -> std::result::Result<TelemetryEvent, String> {
    let metric = params
        .get("metric")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing 'metric'".to_string())?;
    validate_metric(metric).map_err(|e| e.to_string())?;

    let value = params
        .get("value")
        .and_then(|v| v.as_f64())
        .ok_or_else(|| "Missing or non-numeric 'value'".to_string())?;

    let op_str = params.get("op").and_then(|v| v.as_str()).unwrap_or("inc");
    let op = Op::from_str(op_str).map_err(|e| e.to_string())?;

    let agent = params
        .get("agent")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| default_agent.to_string());
    validate_agent_id(&agent).map_err(|e| e.to_string())?;

    let workspace_id = params
        .get("workspace_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .or(default_workspace_id);

    let mut ev = TelemetryEvent::new(agent, metric, value, op, ts).map_err(|e| e.to_string())?;
    if let Some(w) = workspace_id {
        ev = ev.with_workspace(w);
    }
    for (k, v) in parse_tags(params.get("tags"))? {
        ev = ev.with_tag(k, v);
    }
    Ok(ev)
}

/// 이벤트를 memory store 에 저장. seq 가 매 이벤트마다 새로 발급되어 동일
/// ms 안에서 key 가 충돌하지 않는다.
fn persist_event(state: &AppState, ev: &TelemetryEvent) -> std::result::Result<String, String> {
    let seq = state.engine.telemetry_seq.next();
    let key = event_key(ev.ts, seq);
    let scope = scope_for(ev.workspace_id);
    let value = MemoryValue::Json(serde_json::to_value(ev).map_err(|e| e.to_string())?);
    let opts = PutOpts {
        expires_at: None,
        cas: None,
    };
    let result = with_store(|s| s.put(tasty_memory::HOST_OWNER, &scope, &key, &value, &opts));
    match result {
        Some(Ok(_)) => Ok(key),
        Some(Err(e)) => Err(format!("memory put failed: {e}")),
        None => Err("memory store unavailable".into()),
    }
}

/// `telemetry.record` — 단일 메트릭 이벤트 기록.
pub fn handle_record(
    state: &mut AppState,
    caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let default_agent = caller.agent_id();
    let default_ws = state
        .engine
        .workspaces
        .get(state.active_workspace)
        .map(|ws| ws.id);
    let ts = now_ms();
    let ev = match build_event(params, default_agent.as_str(), default_ws, ts) {
        Ok(ev) => ev,
        Err(e) => return JsonRpcResponse::invalid_params(id, e),
    };
    match persist_event(state, &ev) {
        Ok(key) => JsonRpcResponse::success(
            id,
            json!({
                "key": key,
                "ts": ev.ts,
                "agent": ev.agent,
                "metric": ev.metric,
            }),
        ),
        Err(e) => JsonRpcResponse::error(id, -32603, e),
    }
}

/// `telemetry.record_batch` — 여러 이벤트를 한 번에 기록.
///
/// 입력: `{ events: [<event-params>, ...] }`. 각 항목은 record 와 동일한 스키마.
/// 모든 이벤트는 동일한 호출 ts 를 공유하며, seq 만 단조 증가하여 정렬을 보장한다.
pub fn handle_record_batch(
    state: &mut AppState,
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
    let default_ws = state
        .engine
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
        match persist_event(state, ev) {
            Ok(k) => keys.push(k),
            Err(e) => return JsonRpcResponse::error(id, -32603, e),
        }
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

/// 공통 필터 파라미터. 핸들러 진입에서 파싱 후 events 를 수집한다.
struct QueryFilter {
    metric: Option<String>,
    agent: Option<String>,
    workspace_id: Option<u32>,
    since: Option<u64>,
    until: Option<u64>,
}

impl QueryFilter {
    fn from_params(params: &Value) -> std::result::Result<Self, String> {
        let metric = params.get("metric").and_then(|v| v.as_str()).map(String::from);
        if let Some(ref m) = metric {
            validate_metric(m).map_err(|e| e.to_string())?;
        }
        let agent = params.get("agent").and_then(|v| v.as_str()).map(String::from);
        if let Some(ref a) = agent {
            validate_agent_id(a).map_err(|e| e.to_string())?;
        }
        let workspace_id = params
            .get("workspace_id")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);
        let since = params.get("since").and_then(|v| v.as_u64());
        let until = params.get("until").and_then(|v| v.as_u64());
        Ok(Self {
            metric,
            agent,
            workspace_id,
            since,
            until,
        })
    }

    fn matches(&self, ev: &TelemetryEvent) -> bool {
        if let Some(ref m) = self.metric
            && ev.metric != *m
        {
            return false;
        }
        if let Some(ref a) = self.agent
            && ev.agent != *a
        {
            return false;
        }
        if let Some(w) = self.workspace_id
            && ev.workspace_id != Some(w)
        {
            return false;
        }
        if let Some(s) = self.since
            && ev.ts < s
        {
            return false;
        }
        if let Some(u) = self.until
            && ev.ts >= u
        {
            return false;
        }
        true
    }
}

/// 모든 (또는 지정된) scope 에서 telemetry 이벤트를 수집해 필터링한다.
fn collect_events(filter: &QueryFilter) -> std::result::Result<Vec<TelemetryEvent>, String> {
    // workspace_id 가 명시되면 해당 scope 만, 아니면 모든 scope 순회.
    let scopes: Vec<Scope> = if let Some(w) = filter.workspace_id {
        vec![Scope::Workspace(w)]
    } else {
        let scope_strs = with_store(|s| s.scopes())
            .ok_or_else(|| "memory store unavailable".to_string())?
            .map_err(|e| format!("memory scopes failed: {e}"))?;
        scope_strs
            .into_iter()
            .filter_map(|s| Scope::parse(&s).ok())
            .collect()
    };

    let list_opts = ListOpts {
        prefix: Some(EVENT_KEY_PREFIX.to_string()),
        limit: None,
        // updated_at 은 도메인 ts 와 거의 일치하지만 정확하지 않으므로 server
        // 측에서 한 번 더 ev.ts 로 필터한다 (QueryFilter::matches). 여기서는
        // 미리 좁힐 수 있을 때만 좁힌다.
        since: filter.since.map(|v| v as i64),
        until: filter.until.map(|v| v as i64),
        offset: None,
    };
    let mut out = Vec::new();
    for scope in scopes {
        let Some(list_result) = with_store(|s| s.list(&scope, &list_opts)) else {
            continue;
        };
        let entries = match list_result {
            Ok(es) => es,
            Err(e) => {
                tracing::warn!("telemetry: list failed in scope {}: {e}", scope.as_token());
                continue;
            }
        };
        for entry in entries {
            let MemoryValue::Json(v) = entry.value else {
                continue;
            };
            let Ok(ev) = serde_json::from_value::<TelemetryEvent>(v) else {
                continue;
            };
            if filter.matches(&ev) {
                out.push(ev);
            }
        }
    }
    Ok(out)
}

/// `telemetry.summary` — (metric, agent) 별 합/카운트/min/max/last.
pub fn handle_summary(
    _state: &mut AppState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let filter = match QueryFilter::from_params(params) {
        Ok(f) => f,
        Err(e) => return JsonRpcResponse::invalid_params(id, e),
    };
    let events = match collect_events(&filter) {
        Ok(e) => e,
        Err(e) => return JsonRpcResponse::error(id, -32603, e),
    };
    let total_events = events.len();
    let summaries = summarize_events(events);
    let arr: Vec<Value> = summaries
        .iter()
        .map(|s| serde_json::to_value(s).unwrap_or(Value::Null))
        .collect();
    JsonRpcResponse::success(
        id,
        json!({
            "entries": arr,
            "count": arr.len(),
            "total_events": total_events,
        }),
    )
}

/// `telemetry.timeseries` — 윈도우 단위 버킷 시계열.
///
/// 입력: `metric` (필수), `agent` (선택), `workspace_id` (선택), `window` (1m|1h|1d),
/// `since` / `until` (선택, unix ms).
pub fn handle_timeseries(
    _state: &mut AppState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let window_str = params.get("window").and_then(|v| v.as_str()).unwrap_or("1m");
    let window = match Window::from_str(window_str) {
        Ok(w) => w,
        Err(_) => {
            return JsonRpcResponse::invalid_params(
                id,
                format!("invalid 'window' '{window_str}' (1m|1h|1d)"),
            );
        }
    };
    let mut filter = match QueryFilter::from_params(params) {
        Ok(f) => f,
        Err(e) => return JsonRpcResponse::invalid_params(id, e),
    };
    // metric 은 시계열 응답에서 의미 있게 단일 metric 으로 제한.
    if filter.metric.is_none() {
        return JsonRpcResponse::invalid_params(id, "'metric' is required for timeseries");
    }
    // since/until 을 window 경계로 정렬 보존하지는 않는다 — 도메인 함수가 align 처리.
    // (재할당하지 않더라도 의미 없음 — 필터링 후 aggregate.)
    let _ = &mut filter;
    let events = match collect_events(&filter) {
        Ok(e) => e,
        Err(e) => return JsonRpcResponse::error(id, -32603, e),
    };
    let buckets = aggregate_into_buckets(events, window);
    let arr: Vec<Value> = buckets
        .iter()
        .map(|b| serde_json::to_value(b).unwrap_or(Value::Null))
        .collect();
    JsonRpcResponse::success(
        id,
        json!({
            "window": window.as_str(),
            "buckets": arr,
            "count": arr.len(),
        }),
    )
}

/// `telemetry.top` — agent 또는 workspace 기준 sum 내림차순.
pub fn handle_top(
    _state: &mut AppState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let by = params.get("by").and_then(|v| v.as_str()).unwrap_or("agent");
    if by != "agent" && by != "workspace" {
        return JsonRpcResponse::invalid_params(id, "'by' must be 'agent' or 'workspace'");
    }
    let limit = params
        .get("limit")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(10);
    let filter = match QueryFilter::from_params(params) {
        Ok(f) => f,
        Err(e) => return JsonRpcResponse::invalid_params(id, e),
    };
    let events = match collect_events(&filter) {
        Ok(e) => e,
        Err(e) => return JsonRpcResponse::error(id, -32603, e),
    };
    let entries = top_n(events, by, limit);
    let arr: Vec<Value> = entries
        .iter()
        .map(|t| {
            let mut obj = Map::new();
            obj.insert("key".into(), Value::String(t.key.clone()));
            obj.insert(
                "sum".into(),
                Value::from(serde_json::Number::from_f64(t.sum).unwrap_or_else(|| 0.into())),
            );
            obj.insert("count".into(), Value::from(t.count));
            Value::Object(obj)
        })
        .collect();
    JsonRpcResponse::success(
        id,
        json!({
            "by": by,
            "entries": arr,
            "count": arr.len(),
        }),
    )
}
