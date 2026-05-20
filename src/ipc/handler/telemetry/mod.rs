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

use serde_json::Value;
use tasty_memory::{MemoryValue, PutOpts, Scope, with_store};
use tasty_telemetry::{
    CapAction, Op, TelemetryEvent, event_key, validate_agent_id, validate_metric,
};

use crate::ipc::caller::CallerContext;
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
    detect_anomalies_after_ipc(state, agent.as_str(), method, ts);
}

/// IPC 호출 후 anomaly 검출 (Phase 4.4). `AnomalyDetector::record_call` 이 burst
/// 임계를 넘는다고 보고하면 host 가 영속 + notification 으로 알린다.
fn detect_anomalies_after_ipc(state: &mut AppState, agent: &str, method: &str, ts: u64) {
    let seq = state.engine.telemetry_seq.next();
    let detector = state.engine.anomaly_detector.clone();
    let Some(anomaly) = detector.record_call(agent, method, ts, seq) else {
        return;
    };
    if let Err(e) = persist_anomaly(&anomaly) {
        tracing::warn!("anomaly persist failed: {e}");
    }
    fire_anomaly_notification(state, &anomaly);
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

pub(self) use anomaly::{fire_anomaly_notification, persist_anomaly};
pub(self) use cap::{evaluate_caps_after_record, load_all_caps};
