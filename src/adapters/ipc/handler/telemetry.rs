//! 메트릭을 메모리 저장소에 기록하고 조회·집계한다.
//! workspace_id가 있으면 해당 workspace scope, 없으면 global에 저장한다.

use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;
use tasty_memory::{MemoryValue, PutOpts, Scope};
use tasty_telemetry::{
    CapAction, Op, TelemetryEvent, event_key, validate_agent_id, validate_metric,
};

use crate::core::Core;
use tasty_ipc::caller::CallerContext;

pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Pause/RequireApproval cap이 발동한 플러그인의 모든 IPC를 차단한다.
/// Local은 이 검사 대상이 아니므로 cap.reset으로 해제할 수 있다.
pub(crate) fn check_cap_block(
    core: &Core,
    caller: &CallerContext,
    _method: &str,
) -> Option<String> {
    if !caller.is_plugin() {
        return None;
    }
    let agent_id = caller.agent_id();
    let agent = agent_id.as_str();
    let caps = match load_all_caps(core) {
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
        if !matches!(cap.action, CapAction::Pause | CapAction::RequireApproval) {
            continue;
        }
        return Some(format!(
            "cap_triggered: cap={} action={:?} metric={} agent={}",
            cap.id, cap.action, cap.metric, cap.agent,
        ));
    }
    None
}

/// IPC 호출을 ipc_calls로 기록하고 메서드는 태그에 담는다.
/// _host와 telemetry.*는 자기 집계를 피하려고 제외하며 기록 실패가 원래 호출을 막지는 않는다.
pub(crate) fn record_ipc_call(
    core: &mut Core,
    window: &mut dyn crate::ipc::window_port::IpcWindow,
    out: &mut crate::ipc::window_port::IntentOutbox,
    engine: &mut crate::core::CoreState,
    caller: &CallerContext,
    method: &str,
    params: &Value,
) {
    if method.starts_with("telemetry.") {
        return;
    }
    let agent = caller.agent_id();
    if agent.is_host() {
        return;
    }
    let ws = engine.workspaces.first().map(|w| w.id);
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
    if let Err(e) = persist_event(core, engine, &ev) {
        tracing::warn!("telemetry middleware: record failed: {e}");
        return;
    }
    evaluate_caps_after_record(core, window, out, engine, &ev);
    for anomaly in detect_anomalies_after_ipc(core, engine, agent.as_str(), method, params, ts) {
        fire_anomaly_notification(window, out, engine, &anomaly);
    }
}

/// 호출 후 CallBurst/SlowLoop를 검사해 저장한다. 알림은 호출자가 담당한다.
fn detect_anomalies_after_ipc(
    core: &Core,
    engine: &mut crate::core::CoreState,
    agent: &str,
    method: &str,
    params: &Value,
    ts: u64,
) -> Vec<tasty_telemetry::Anomaly> {
    let seq = engine.telemetry_seq.next();
    let detector = engine.anomaly_detector.clone();
    let anomalies = detector.record_call(agent, method, params, ts, seq);
    for anomaly in &anomalies {
        if let Err(e) = persist_anomaly(core, anomaly) {
            tracing::warn!("anomaly persist failed: {e}");
        }
    }
    anomalies
}

/// RSS 증가를 검사한다. Agent는 record/record_batch의 자기 보고를,
/// Plugin은 호스트가 sysinfo로 수집한 샘플을 사용한다.
pub(crate) fn record_rss_sample(
    core: &Core,
    window: &mut dyn crate::ipc::window_port::IpcWindow,
    out: &mut crate::ipc::window_port::IntentOutbox,
    engine: &mut crate::core::CoreState,
    agent: &str,
    rss_bytes: u64,
    ts: u64,
) {
    let seq = engine.telemetry_seq.next();
    let detector = engine.anomaly_detector.clone();
    let Some(anomaly) = detector.record_rss_sample(agent, rss_bytes, ts, seq) else {
        return;
    };
    if let Err(e) = persist_anomaly(core, &anomaly) {
        tracing::warn!("anomaly persist failed: {e}");
    }
    fire_anomaly_notification(window, out, engine, &anomaly);
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

    let value =
        super::params::read_f64(params, "value")?.ok_or_else(|| "Missing 'value'".to_string())?;

    let op_str = params.get("op").and_then(|v| v.as_str()).unwrap_or("inc");
    let op = op_str.parse::<Op>().map_err(|e| e.to_string())?;

    let agent = params
        .get("agent")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| default_agent.to_string());
    validate_agent_id(&agent).map_err(|e| e.to_string())?;

    let workspace_id = super::params::read_u32(params, "workspace_id")?.or(default_workspace_id);

    let mut ev = TelemetryEvent::new(agent, metric, value, op, ts).map_err(|e| e.to_string())?;
    if let Some(w) = workspace_id {
        ev = ev.with_workspace(w);
    }
    for (k, v) in parse_tags(params.get("tags"))? {
        ev = ev.with_tag(k, v);
    }
    Ok(ev)
}

/// 같은 밀리초에 들어온 이벤트는 새 seq로 키 충돌을 피한다.
fn persist_event(
    core: &Core,
    engine: &mut crate::core::CoreState,
    ev: &TelemetryEvent,
) -> std::result::Result<String, String> {
    let seq = engine.telemetry_seq.next();
    let key = event_key(ev.ts, seq);
    let scope = scope_for(ev.workspace_id);
    let value = MemoryValue::Json(serde_json::to_value(ev).map_err(|e| e.to_string())?);
    let opts = PutOpts {
        expires_at: None,
        cas: None,
    };
    core.with_memory(|s| {
        // 기록 유입 시 관측 로그를 정리한다. 공용 게이트로 최대 시간당 한 번 수행한다.
        crate::store::log_retention::maybe_prune(s, ev.ts);
        s.put(tasty_memory::HOST_OWNER, &scope, &key, &value, &opts)
    })
    .map_err(|e| format!("memory put failed: {e}"))?;
    Ok(key)
}

pub mod anomaly;
pub mod cap;
pub mod query;
pub mod record;
pub mod session;

pub use anomaly::handle_anomaly_list;
pub use cap::{
    handle_cap_list, handle_cap_remove, handle_cap_reset, handle_cap_set, handle_cap_status,
};
pub use query::{handle_summary, handle_timeseries, handle_top};
pub use record::{handle_record, handle_record_batch};
pub use session::handle_session_summary;

use anomaly::{fire_anomaly_notification, persist_anomaly};
use cap::{evaluate_caps_after_record, load_all_caps};
