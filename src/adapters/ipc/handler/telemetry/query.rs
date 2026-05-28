//! `telemetry.summary` / `telemetry.timeseries` / `telemetry.top` 핸들러.

use serde_json::{Map, Value, json};
use tasty_memory::{ListOpts, MemoryValue, Scope, with_store};
use tasty_telemetry::{
    EVENT_KEY_PREFIX, TelemetryEvent, Window, aggregate_into_buckets, summarize_events, top_n,
    validate_agent_id, validate_metric,
};

use crate::ipc::caller::CallerContext;
use crate::ipc::protocol::JsonRpcResponse;
use crate::state::AppState;

/// 공통 필터 파라미터. 핸들러 진입에서 파싱 후 events 를 수집한다.
pub(super) struct QueryFilter {
    pub(super) metric: Option<String>,
    pub(super) agent: Option<String>,
    pub(super) workspace_id: Option<u32>,
    pub(super) since: Option<u64>,
    pub(super) until: Option<u64>,
}

impl QueryFilter {
    fn from_params(params: &Value) -> std::result::Result<Self, String> {
        let metric = params
            .get("metric")
            .and_then(|v| v.as_str())
            .map(String::from);
        if let Some(ref m) = metric {
            validate_metric(m).map_err(|e| e.to_string())?;
        }
        let agent = params
            .get("agent")
            .and_then(|v| v.as_str())
            .map(String::from);
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
pub(super) fn collect_events(
    filter: &QueryFilter,
) -> std::result::Result<Vec<TelemetryEvent>, String> {
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
    _engine: &mut crate::engine_state::EngineState,
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
    _engine: &mut crate::engine_state::EngineState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let window_str = params
        .get("window")
        .and_then(|v| v.as_str())
        .unwrap_or("1m");
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
    _engine: &mut crate::engine_state::EngineState,
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
