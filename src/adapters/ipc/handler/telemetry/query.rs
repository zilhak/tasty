//! `telemetry.summary` / `telemetry.timeseries` / `telemetry.top` 핸들러.

use crate::adapters::ipc::handler::params::{self, p_try};
use serde_json::{Map, Value, json};
use tasty_memory::{ListOpts, MemoryValue, Scope};
use tasty_telemetry::{
    EVENT_KEY_PREFIX, TelemetryEvent, Window, aggregate_into_buckets, summarize_events, top_n,
    validate_agent_id, validate_metric,
};

use crate::core::Core;
use tasty_ipc::caller::CallerContext;
use tasty_ipc::protocol::JsonRpcResponse;

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
        let workspace_id = params::read_u32(params, "workspace_id")?;
        let since = params::read_int::<u64>(params, "since")?;
        let until = params::read_int::<u64>(params, "until")?;
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

pub(super) fn collect_events(
    core: &Core,
    filter: &QueryFilter,
) -> std::result::Result<Vec<TelemetryEvent>, String> {
    let scopes: Vec<Scope> = if let Some(w) = filter.workspace_id {
        vec![Scope::Workspace(w)]
    } else {
        let scope_strs = core
            .with_memory(|s| s.scopes())
            .map_err(|e| format!("memory scopes failed: {e}"))?;
        scope_strs
            .into_iter()
            .filter_map(|s| Scope::parse(&s).ok())
            .collect()
    };

    let list_opts = ListOpts {
        prefix: Some(EVENT_KEY_PREFIX.to_string()),
        limit: None,
        // 저장 시각과 이벤트 시각은 다를 수 있어 QueryFilter에서 ev.ts로 다시 거른다.
        since: filter.since.map(|v| v as i64),
        until: filter.until.map(|v| v as i64),
        offset: None,
    };
    let mut out = Vec::new();
    for scope in scopes {
        let entries = match core.with_memory(|s| s.list(&scope, &list_opts)) {
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
    core: &Core,
    _engine: &mut crate::core::CoreState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let filter = match QueryFilter::from_params(params) {
        Ok(f) => f,
        Err(e) => return JsonRpcResponse::invalid_params(id, e),
    };
    let events = match collect_events(core, &filter) {
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
    core: &Core,
    _engine: &mut crate::core::CoreState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let window_str = params
        .get("window")
        .and_then(|v| v.as_str())
        .unwrap_or("1m");
    let window = match window_str.parse::<Window>() {
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
    if filter.metric.is_none() {
        return JsonRpcResponse::invalid_params(id, "'metric' is required for timeseries");
    }
    let _ = &mut filter; // 재할당 안 함 — reborrow 로 mut 바인딩 의도 표시(값 drop, Result 아님).
    let events = match collect_events(core, &filter) {
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
    core: &Core,
    _engine: &mut crate::core::CoreState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let by = params.get("by").and_then(|v| v.as_str()).unwrap_or("agent");
    if by != "agent" && by != "workspace" {
        return JsonRpcResponse::invalid_params(id, "'by' must be 'agent' or 'workspace'");
    }
    let limit = p_try!(params::opt_int::<usize>(params, "limit", &id)).unwrap_or(10);
    let filter = match QueryFilter::from_params(params) {
        Ok(f) => f,
        Err(e) => return JsonRpcResponse::invalid_params(id, e),
    };
    let events = match collect_events(core, &filter) {
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
