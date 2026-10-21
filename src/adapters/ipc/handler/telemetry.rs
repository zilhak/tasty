//! 메트릭을 메모리 저장소에 기록하고 조회·집계한다.
//! workspace_id가 있으면 해당 workspace scope, 없으면 global에 저장한다.

use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;
use tasty_memory::{MemoryValue, PutOpts, Scope};
use tasty_telemetry::{
    CapAction, Op, TelemetryEvent, event_key, validate_agent_id, validate_metric,
};

use crate::app::services::AppServices;
use tasty_ipc::caller::CallerContext;

pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Pause/RequireApproval cap이 발동한 플러그인의 모든 IPC를 차단한다.
/// Local은 이 검사 대상이 아니므로 cap.reset으로 해제할 수 있다. `caps`는 요청 게이트가 writer에서
/// 읽은 cap 목록이며, 읽지 못했으면 None이라 차단하지 않는다(fail-open).
pub(crate) fn check_cap_block(
    caller: &CallerContext,
    caps: Option<&[tasty_telemetry::CostCap]>,
) -> Option<String> {
    if !caller.is_plugin() {
        return None;
    }
    let agent_id = caller.agent_id();
    let agent = agent_id.as_str();
    for cap in caps? {
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

/// 요청 게이트가 기록할 ipc_calls 이벤트. 메서드는 태그에 담는다.
/// _host와 telemetry.*는 자기 집계를 피하려고 기록하지 않는다.
pub(crate) fn ipc_call_event(
    caller: &CallerContext,
    method: &str,
    workspace_id: Option<u32>,
    ts: u64,
) -> Option<TelemetryEvent> {
    if method.starts_with("telemetry.") {
        return None;
    }
    let agent = caller.agent_id();
    if agent.is_host() {
        return None;
    }
    let ev = match TelemetryEvent::new(agent.as_str(), "ipc_calls", 1.0, Op::Inc, ts) {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!("telemetry middleware: build event failed: {e}");
            return None;
        }
    };
    let ev = ev.with_tag("method", method);
    Some(match workspace_id {
        Some(w) => ev.with_workspace(w),
        None => ev,
    })
}

/// 기록 후 상한에 닿은 cap. triggered는 이미 저장됐다.
pub(crate) struct TriggeredCap {
    pub(crate) cap: tasty_telemetry::CostCap,
    pub(crate) current: f64,
}

/// 저장된 이벤트 뒤 상한을 검사해 triggered를 저장한다. writer 요청 본문 안에서 부르며 액션은
/// 실행하지 않는다. 저장 실패가 아닌 오류는 `warnings`에 남기고 그 cap만 건너뛴다.
pub(crate) fn trigger_caps_in(
    s: &mut dyn tasty_memory::MemoryStorage,
    ev: &TelemetryEvent,
    warnings: &mut Vec<String>,
) -> Vec<TriggeredCap> {
    let caps = match load_all_caps(s) {
        Ok(c) => c,
        Err(e) => {
            warnings.push(format!("cap eval: load failed: {e}"));
            return Vec::new();
        }
    };
    let mut triggered = Vec::new();
    for mut cap in caps {
        if !cap_matches_untriggered(&cap, ev) {
            continue;
        }
        let current = match compute_current_value(s, &cap) {
            Ok(v) => v,
            Err(e) => {
                warnings.push(format!("cap eval: compute failed for {}: {e}", cap.id));
                continue;
            }
        };
        if current < cap.threshold {
            continue;
        }
        cap.triggered = Some(tasty_telemetry::CapTriggered {
            at: now_ms(),
            value: current,
        });
        if let Err(e) = save_cap(s, &cap) {
            warnings.push(format!("cap eval: save failed for {}: {e}", cap.id));
            continue;
        }
        triggered.push(TriggeredCap { cap, current });
    }
    triggered
}

/// 확정된 triggered cap의 액션을 실행한다. Notify는 알림, RequireApproval은 승인 요청, Pause는 알림이다.
pub(crate) fn fire_triggered_caps(
    core: &mut AppServices,
    window: &mut dyn crate::ipc::window_port::IpcWindow,
    out: &mut crate::ipc::window_port::IntentOutbox,
    engine: &mut crate::runtime::engine_access::EngineMut<'_>,
    triggered: &[TriggeredCap],
) {
    for t in triggered {
        fire_cap_action(core, window, out, engine, &t.cap, t.current);
    }
}

/// 게이트가 기록한 ipc_calls 뒤의 이상 탐지와 알림. 탐지기는 메모리 상태라 writer 결과를 거둔 뒤
/// 한 번만 부른다.
pub(crate) fn observe_ipc_call(
    core: &AppServices,
    window: &mut dyn crate::ipc::window_port::IpcWindow,
    out: &mut crate::ipc::window_port::IntentOutbox,
    engine: &mut crate::runtime::engine_access::EngineMut<'_>,
    ev: &TelemetryEvent,
    method: &str,
    params: &Value,
) {
    for anomaly in detect_anomalies_after_ipc(core, &ev.agent, method, params, ev.ts) {
        fire_anomaly_notification(window, out, engine, &anomaly);
    }
}

/// 호출 후 CallBurst/SlowLoop를 검사해 저장한다. 알림은 호출자가 담당한다.
fn detect_anomalies_after_ipc(
    core: &AppServices,
    agent: &str,
    method: &str,
    params: &Value,
    ts: u64,
) -> Vec<tasty_telemetry::Anomaly> {
    let seq = core.telemetry_seq.next();
    let detector = core.anomaly_detector.clone();
    let anomalies = detector.record_call(agent, method, params, ts, seq);
    for anomaly in &anomalies {
        persist_anomaly(core, anomaly);
    }
    anomalies
}

/// RSS 증가를 검사한다. Agent는 record/record_batch의 자기 보고를,
/// Plugin은 호스트가 sysinfo로 수집한 샘플을 사용한다.
pub(crate) fn record_rss_sample(
    core: &AppServices,
    window: &mut dyn crate::ipc::window_port::IpcWindow,
    out: &mut crate::ipc::window_port::IntentOutbox,
    engine: &mut crate::runtime::engine_access::EngineMut<'_>,
    agent: &str,
    rss_bytes: u64,
    ts: u64,
) {
    let Some(anomaly) = core.record_rss_sample(agent, rss_bytes, ts) else {
        return;
    };
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

/// 이벤트 키를 접수 전에 정한다. 같은 밀리초 이벤트는 seq로 구별하고, 본문이 다시 실행돼도 같은 키다.
pub(crate) fn reserve_event_key(core: &AppServices, ev: &TelemetryEvent) -> String {
    event_key(ev.ts, core.telemetry_seq.next())
}

/// writer 요청 본문 안에서 이벤트 하나를 저장한다. 로그 보존 정리는 결과를 거둔 쪽이 따로 넣는다.
pub(crate) fn put_event(
    s: &mut dyn tasty_memory::MemoryStorage,
    ev: &TelemetryEvent,
    key: &str,
) -> std::result::Result<(), String> {
    let scope = scope_for(ev.workspace_id);
    let value = MemoryValue::Json(serde_json::to_value(ev).map_err(|e| e.to_string())?);
    let opts = PutOpts {
        expires_at: None,
        cas: None,
    };
    s.put(tasty_memory::HOST_OWNER, &scope, key, &value, &opts)
        .map(|_| ())
        .map_err(|e| format!("memory put failed: {e}"))
}

pub mod anomaly;
#[cfg(test)]
mod caller_agent_id_tests;
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

pub(super) use anomaly::fire_anomaly_notification;
use anomaly::persist_anomaly;
pub(crate) use cap::load_all_caps;
use cap::{cap_matches_untriggered, compute_current_value, fire_cap_action, save_cap};
