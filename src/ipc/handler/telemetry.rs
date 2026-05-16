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
    CAP_KEY_PREFIX, CapAction, CapWindow, CostCap, EVENT_KEY_PREFIX, Op, TelemetryEvent, Window,
    aggregate_into_buckets, cap_key, event_key, summarize_events, top_n, validate_agent_id,
    validate_metric,
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

/// 호출 전 cap 차단 체크 (Phase 4.3c).
///
/// Plugin caller 의 agent 에 대해, `triggered` 가 있고 action 이 `Stop` 또는
/// `Pause` 인 cap 이 하나라도 있으면 차단 사유 문자열을 반환한다.
///
/// 모든 메서드를 차단한다 (telemetry.* 포함). `telemetry.cap.reset` 으로 해제는
/// **Local caller (CLI/사용자)** 만 가능 — Local 은 본 함수의 검사 대상이 아니므로
/// 차단되지 않는다.
///
/// 차단 메시지는 cap_id / metric / action 을 포함해 디버깅을 돕는다.
pub(crate) fn check_cap_block(caller: &CallerContext, _method: &str) -> Option<String> {
    if !caller.is_plugin() {
        return None;
    }
    let agent_id = caller.agent_id();
    let agent = agent_id.as_str();
    let caps = match load_all_caps() {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("cap block check: load failed: {e}");
            return None;
        }
    };
    for cap in &caps {
        if cap.agent != agent {
            continue;
        }
        if cap.triggered.is_none() {
            continue;
        }
        if !matches!(
            cap.action,
            CapAction::Stop | CapAction::Pause | CapAction::RequireApproval
        ) {
            continue;
        }
        return Some(format!(
            "cap_triggered: cap={} action={:?} metric={} agent={}",
            cap.id, cap.action, cap.metric, cap.agent,
        ));
    }
    None
}

/// dispatcher 미들웨어용 자동 카운트 (Phase 4.2).
///
/// caller 가 `_host` 이거나 method 가 `telemetry.` 로 시작하면 skip한다 —
/// 자기 자신을 측정하면 의미가 없고, telemetry 내부 호출은 재귀 폭주를 만든다.
/// 메트릭 이름은 `ipc_calls`, 메서드 식별자는 `method` 태그로 들어간다 (도메인
/// metric 검증이 `.` 을 허용하지 않으므로 `ipc_calls.<method>` 형태를 피한다).
///
/// 모든 실패는 best-effort. 호스트 stdout 의 IPC 정상 동작을 막지 않는다.
pub(crate) fn record_ipc_call(state: &mut AppState, caller: &CallerContext, method: &str) {
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
        return;
    }
    evaluate_caps_after_record(state, &ev);
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
    let response = match persist_event(state, &ev) {
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
    evaluate_caps_after_record(state, &ev);
    response
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
    for ev in &events {
        evaluate_caps_after_record(state, ev);
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

// ============================================================
// Cost cap — Phase 4.3a (CRUD + status/reset). eval/action wiring 은
// 후속 sub-phase 에서 dispatcher 미들웨어에 결합한다.
// ============================================================

/// 새 cap_id 발급. `cap_{ts_ms:013}{seq:04}` — TelemetrySeq 로 동일 ms 충돌 회피.
fn generate_cap_id(state: &AppState) -> String {
    let ts = now_ms();
    let seq = state.engine.telemetry_seq.next();
    format!("cap_{ts:013}{seq:04}", ts = ts, seq = seq % 10_000)
}

/// 모든 cap 을 memory 에서 읽어온다. cap 은 global scope 에만 저장.
fn load_all_caps() -> std::result::Result<Vec<CostCap>, String> {
    let list_opts = ListOpts {
        prefix: Some(CAP_KEY_PREFIX.to_string()),
        limit: None,
        since: None,
        until: None,
        offset: None,
    };
    let Some(list_result) = with_store(|s| s.list(&Scope::Global, &list_opts)) else {
        return Err("memory store unavailable".into());
    };
    let entries = list_result.map_err(|e| format!("memory list failed: {e}"))?;
    let mut out = Vec::new();
    for entry in entries {
        let MemoryValue::Json(v) = entry.value else {
            continue;
        };
        if let Ok(cap) = serde_json::from_value::<CostCap>(v) {
            out.push(cap);
        }
    }
    Ok(out)
}

fn save_cap(cap: &CostCap) -> std::result::Result<(), String> {
    let key = cap_key(&cap.id);
    let value = MemoryValue::Json(serde_json::to_value(cap).map_err(|e| e.to_string())?);
    let opts = PutOpts {
        expires_at: None,
        cas: None,
    };
    let result = with_store(|s| s.put(tasty_memory::HOST_OWNER, &Scope::Global, &key, &value, &opts));
    match result {
        Some(Ok(_)) => Ok(()),
        Some(Err(e)) => Err(format!("memory put failed: {e}")),
        None => Err("memory store unavailable".into()),
    }
}

fn cap_to_json(cap: &CostCap) -> Value {
    serde_json::to_value(cap).unwrap_or(Value::Null)
}

/// `telemetry.cap.set` — cap 등록.
pub fn handle_cap_set(
    state: &mut AppState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let agent = match params.get("agent").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return JsonRpcResponse::invalid_params(id, "Missing 'agent'"),
    };
    if let Err(e) = validate_agent_id(&agent) {
        return JsonRpcResponse::invalid_params(id, e.to_string());
    }
    let metric = match params.get("metric").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return JsonRpcResponse::invalid_params(id, "Missing 'metric'"),
    };
    if let Err(e) = validate_metric(&metric) {
        return JsonRpcResponse::invalid_params(id, e.to_string());
    }
    let threshold = match params.get("threshold").and_then(|v| v.as_f64()) {
        Some(t) if t > 0.0 => t,
        _ => {
            return JsonRpcResponse::invalid_params(
                id,
                "'threshold' must be a positive number",
            );
        }
    };
    let window_str = params
        .get("window")
        .and_then(|v| v.as_str())
        .unwrap_or("total");
    let window = match CapWindow::from_str(window_str) {
        Ok(w) => w,
        Err(_) => {
            return JsonRpcResponse::invalid_params(
                id,
                format!("invalid 'window' '{window_str}' (total|1h|1d)"),
            );
        }
    };
    let action_str = params
        .get("action")
        .and_then(|v| v.as_str())
        .unwrap_or("notify");
    let action = match CapAction::from_str(action_str) {
        Ok(a) => a,
        Err(_) => {
            return JsonRpcResponse::invalid_params(
                id,
                format!("invalid 'action' '{action_str}' (stop|pause|require_approval|notify)"),
            );
        }
    };

    let cap = CostCap {
        id: generate_cap_id(state),
        agent,
        metric,
        threshold,
        window,
        action,
        created_at: now_ms(),
        triggered: None,
    };
    if let Err(e) = save_cap(&cap) {
        return JsonRpcResponse::error(id, -32603, e);
    }
    JsonRpcResponse::success(id, cap_to_json(&cap))
}

/// `telemetry.cap.list` — 전체 cap. 필터: `agent`.
pub fn handle_cap_list(
    _state: &mut AppState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let agent_filter = params.get("agent").and_then(|v| v.as_str()).map(String::from);
    let mut caps = match load_all_caps() {
        Ok(c) => c,
        Err(e) => return JsonRpcResponse::error(id, -32603, e),
    };
    if let Some(ref a) = agent_filter {
        caps.retain(|c| &c.agent == a);
    }
    caps.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    let arr: Vec<Value> = caps.iter().map(cap_to_json).collect();
    JsonRpcResponse::success(id, json!({ "entries": arr, "count": arr.len() }))
}

/// `telemetry.cap.remove` — cap 삭제.
pub fn handle_cap_remove(
    _state: &mut AppState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let cap_id_str = match params.get("id").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return JsonRpcResponse::invalid_params(id, "Missing 'id'"),
    };
    let key = cap_key(&cap_id_str);
    let result = with_store(|s| s.delete(tasty_memory::HOST_OWNER, &Scope::Global, &key, None));
    match result {
        Some(Ok(())) => {
            JsonRpcResponse::success(id, json!({ "removed": true, "id": cap_id_str }))
        }
        Some(Err(tasty_memory::MemoryError::NotFound { .. })) => {
            JsonRpcResponse::error(id, -32004, format!("not_found: {cap_id_str}"))
        }
        Some(Err(e)) => JsonRpcResponse::error(id, -32603, format!("memory delete failed: {e}")),
        None => JsonRpcResponse::error(id, -32603, "memory store unavailable"),
    }
}

/// agent + metric + window 의 현재 누적값을 raw events 에서 즉시 집계.
///
/// `Op::Set` 은 sum 을 통째 교체. `Op::Inc/Dec` 는 누적. 4.1 의 `summarize_events`
/// 와 동일 정책.
fn compute_current_value(cap: &CostCap) -> std::result::Result<f64, String> {
    let now = now_ms();
    let (since, until) = match cap.window.span_ms() {
        Some(span) => (Some(now.saturating_sub(span)), Some(now)),
        None => (None, None),
    };
    let filter = QueryFilter {
        metric: Some(cap.metric.clone()),
        agent: Some(cap.agent.clone()),
        workspace_id: None,
        since,
        until,
    };
    let events = collect_events(&filter)?;
    if events.is_empty() {
        return Ok(0.0);
    }
    let summaries = summarize_events(events);
    // metric/agent 가 단일이면 결과는 단일 entry. workspace_id 분리는 무시 — cap 은 agent 전체.
    let sum: f64 = summaries.iter().map(|s| s.sum).sum();
    Ok(sum)
}

/// `telemetry.cap.status` — agent 별 cap 들의 현재 값/임계/triggered 상태.
pub fn handle_cap_status(
    _state: &mut AppState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let agent_filter = params.get("agent").and_then(|v| v.as_str()).map(String::from);
    let caps = match load_all_caps() {
        Ok(c) => c,
        Err(e) => return JsonRpcResponse::error(id, -32603, e),
    };
    let mut out: Vec<Value> = Vec::new();
    for cap in &caps {
        if let Some(ref a) = agent_filter
            && &cap.agent != a
        {
            continue;
        }
        let current = match compute_current_value(cap) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("cap status: compute failed for {}: {e}", cap.id);
                continue;
            }
        };
        let ratio = if cap.threshold > 0.0 {
            current / cap.threshold
        } else {
            0.0
        };
        let mut entry = serde_json::to_value(cap).unwrap_or(Value::Null);
        if let Some(obj) = entry.as_object_mut() {
            obj.insert(
                "current_value".into(),
                Value::from(serde_json::Number::from_f64(current).unwrap_or_else(|| 0.into())),
            );
            obj.insert(
                "ratio".into(),
                Value::from(serde_json::Number::from_f64(ratio).unwrap_or_else(|| 0.into())),
            );
        }
        out.push(entry);
    }
    JsonRpcResponse::success(id, json!({ "entries": out, "count": out.len() }))
}

/// `telemetry.cap.reset` — `triggered` 상태 제거. `id` 또는 `agent` 둘 중 하나 필수.
pub fn handle_cap_reset(
    _state: &mut AppState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let by_id = params.get("id").and_then(|v| v.as_str()).map(String::from);
    let by_agent = params.get("agent").and_then(|v| v.as_str()).map(String::from);
    if by_id.is_none() && by_agent.is_none() {
        return JsonRpcResponse::invalid_params(id, "Provide 'id' or 'agent'");
    }
    let mut caps = match load_all_caps() {
        Ok(c) => c,
        Err(e) => return JsonRpcResponse::error(id, -32603, e),
    };
    let mut reset_ids: Vec<String> = Vec::new();
    for cap in caps.iter_mut() {
        let matches = match (&by_id, &by_agent) {
            (Some(i), _) => &cap.id == i,
            (None, Some(a)) => &cap.agent == a,
            _ => false,
        };
        if !matches || cap.triggered.is_none() {
            continue;
        }
        cap.triggered = None;
        if let Err(e) = save_cap(cap) {
            tracing::warn!("cap reset: save failed for {}: {e}", cap.id);
            continue;
        }
        reset_ids.push(cap.id.clone());
    }
    JsonRpcResponse::success(
        id,
        json!({ "reset_ids": reset_ids, "count": reset_ids.len() }),
    )
}

// ============================================================
// Cap 평가 / 액션 발화 — Phase 4.3b (Notify 만 우선)
// ============================================================

/// 매 record 후 호출되는 best-effort 후크. agent+metric 이 일치하고 아직
/// triggered 되지 않은 cap 들을 검사해 임계를 넘으면 `triggered` 마크 + 액션 발화.
///
/// 모든 실패는 warn 로그로만 — record 자체의 응답에는 영향이 없다.
///
/// Phase 4.3b: `Notify` 만 발화 (단순 알림 + 차단 없음). `Stop`/`Pause`/`RequireApproval`
/// 는 후속 sub-phase (호출 전 evaluator + dispatcher 거부) 에서 결합한다.
fn evaluate_caps_after_record(state: &mut AppState, ev: &TelemetryEvent) {
    let caps = match load_all_caps() {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("cap eval: load failed: {e}");
            return;
        }
    };
    for mut cap in caps {
        if cap.agent != ev.agent || cap.metric != ev.metric {
            continue;
        }
        if cap.triggered.is_some() {
            continue;
        }
        let current = match compute_current_value(&cap) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("cap eval: compute failed for {}: {e}", cap.id);
                continue;
            }
        };
        if current < cap.threshold {
            continue;
        }
        // 임계 초과 — triggered 마크 후 액션 발화.
        cap.triggered = Some(tasty_telemetry::CapTriggered {
            at: now_ms(),
            value: current,
        });
        if let Err(e) = save_cap(&cap) {
            tracing::warn!("cap eval: save failed for {}: {e}", cap.id);
            continue;
        }
        fire_cap_action(state, &cap, current);
    }
}

/// cap 액션을 실제 시스템으로 발화. Phase 4.3b 는 `Notify` 만 처리; 나머지 액션은
/// 미래 sub-phase 에서 결합되며 현재는 로그만 남긴다 (memory 상의 `triggered` 필드는
/// 이미 기록됐으므로 status 조회로 확인 가능).
fn fire_cap_action(state: &mut AppState, cap: &CostCap, current: f64) {
    match cap.action {
        CapAction::Notify => fire_notify(state, cap, current),
        CapAction::RequireApproval => fire_require_approval(state, cap, current),
        CapAction::Stop | CapAction::Pause => {
            // 차단은 dispatcher 의 check_cap_block 이 담당. 여기서는 사용자에게
            // 사실을 알리는 알림만 함께 띄운다 — 차단된 plugin 이 침묵 속에 멈춰
            // 보이지 않도록.
            fire_notify(state, cap, current);
            tracing::info!(
                "cap triggered (action {:?}): cap={} agent={} metric={} value={} threshold={}",
                cap.action,
                cap.id,
                cap.agent,
                cap.metric,
                current,
                cap.threshold,
            );
        }
    }
}

/// `RequireApproval` 액션 (Phase 4.3d): cap 이 처음 triggered 되는 시점에
/// host 가 approval.request 를 자동 발행한다. 이후 plugin 의 모든 IPC 는
/// `check_cap_block` 이 거부 — 사용자는 popup 에서 승인 후 `cap.reset` 으로
/// triggered 를 풀어야 plugin 이 재개된다 (또는 거부 후 그대로 둠).
///
/// 매 호출마다 새 approval 을 발행하지 않고, **cap-당 1회**만 발행 (triggered 가
/// 비어 있을 때만 fire_cap_action 가 호출되므로 자연스럽게 단발). 추가 발행이
/// 필요하면 `cap.reset` 후 다음 record 가 임계를 다시 넘을 때 fire 된다.
fn fire_require_approval(state: &mut AppState, cap: &CostCap, current: f64) {
    let ws_id = state
        .engine
        .workspaces
        .get(state.active_workspace)
        .map(|w| w.id);
    let title = format!("Cap '{}' — 승인 필요", cap.metric);
    let body = format!(
        "agent={} metric={} value={} ≥ threshold={} (window={:?}, cap={}). \
         승인하면 `tasty telemetry cap reset --id {}` 으로 해제하세요.",
        cap.agent, cap.metric, current, cap.threshold, cap.window, cap.id, cap.id,
    );
    let req = tasty_approval::ApprovalRequest {
        id: tasty_approval::ApprovalId::generate(),
        requester: tasty_approval::Requester::Agent {
            id: tasty_memory::HOST_OWNER.to_string(),
        },
        workspace_id: ws_id,
        surface_id: None,
        title,
        body: Some(body),
        choices: Vec::new(),
        default_choice: None,
        timeout_ms: None,
        severity: tasty_approval::Severity::Warn,
        created_at: 0,
        metadata: serde_json::json!({
            "source": "telemetry.cap",
            "cap_id": cap.id,
            "agent": cap.agent,
            "metric": cap.metric,
            "value": current,
            "threshold": cap.threshold,
        }),
    };
    match state.engine.approval_store.request(req) {
        Ok(change) => {
            crate::ipc::handler::approval::persist_record(&change.record);
            crate::ui::approval_popup::enqueue_approval(state, &change.record);
            tracing::info!(
                "cap require_approval: issued approval id={} for cap={}",
                change.record.request.id.as_str(),
                cap.id,
            );
        }
        Err(e) => {
            tracing::warn!("cap require_approval: approval.request failed for cap={}: {e}", cap.id);
        }
    }
}

/// `Notify` 액션: 활성 워크스페이스에 notification 추가 + host event enqueue.
/// notification.create 핸들러의 단순 경로와 동등하나 IPC 를 거치지 않는다.
fn fire_notify(state: &mut AppState, cap: &CostCap, current: f64) {
    let Some(ws) = state.engine.workspaces.get(state.active_workspace) else {
        tracing::warn!("cap notify: no active workspace, skipping cap {}", cap.id);
        return;
    };
    let ws_id = ws.id;
    let title = format!("Cap '{}' 임계 도달", cap.metric);
    let body = format!(
        "agent={} metric={} value={} ≥ threshold={} (window={:?}, cap={})",
        cap.agent, cap.metric, current, cap.threshold, cap.window, cap.id,
    );
    let created = state
        .engine
        .notifications
        .add(ws_id, 0, title.clone(), body.clone());
    if let Some(nid) = created {
        state.enqueue_host_event(crate::state::PendingHostEvent::NotificationCreated {
            id: nid,
            title,
            body,
            source: "telemetry.cap".to_string(),
        });
    }
}
