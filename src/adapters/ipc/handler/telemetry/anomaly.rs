//! 이상 탐지 기록·조회·알림. 보존은 store::log_retention의 공통 정책을 따른다(ADR-0009).

use crate::adapters::ipc::handler::params::{self, p_try};
use serde_json::{Value, json};
use tasty_memory::{ListOpts, MemoryValue, PutOpts, Scope};
use tasty_telemetry::{ANOMALY_KEY_PREFIX, Anomaly, AnomalyKind, anomaly_key};

use crate::core::Core;
use tasty_ipc::caller::CallerContext;
use tasty_ipc::protocol::JsonRpcResponse;

/// expires_at은 조회에서 제외할 뿐 디스크에서 지우지 않는다.
/// 물리 삭제는 부팅·실행 중 log_retention의 상한으로 별도 수행한다.
pub(super) fn persist_anomaly(core: &Core, anomaly: &Anomaly) -> std::result::Result<(), String> {
    let key = anomaly_key(anomaly.detected_at, &anomaly.id);
    let value = MemoryValue::Json(serde_json::to_value(anomaly).map_err(|e| e.to_string())?);
    let opts = PutOpts {
        expires_at: Some((anomaly.detected_at + crate::store::log_retention::LOG_TTL_MS) as i64),
        cas: None,
    };
    core.with_memory(|s| {
        crate::store::log_retention::maybe_prune(s, anomaly.detected_at);
        s.put(
            tasty_memory::HOST_OWNER,
            &Scope::Global,
            &key,
            &value,
            &opts,
        )
    })
    .map(|_| ())
    .map_err(|e| format!("memory put failed: {e}"))
}

pub(super) fn fire_anomaly_notification(
    window: &mut dyn crate::ipc::window_port::IpcWindow,
    out: &mut crate::ipc::window_port::IntentOutbox,
    engine: &mut crate::core::CoreState,
    anomaly: &Anomaly,
) {
    let Some(ws) = engine.workspaces.get(window.active_workspace_index()) else {
        return;
    };
    let ws_id = ws.id;
    let title = format!(
        "이상 탐지: {} ({})",
        anomaly.kind.as_token(),
        anomaly.subject
    );
    // CallBurst/SlowLoop는 시간·건수, RssSurge는 샘플 수·RSS로 설명한다.
    let body = match anomaly.kind {
        AnomalyKind::CallBurst | AnomalyKind::SlowLoop => {
            let count = anomaly
                .detail
                .get("count")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let window_ms = anomaly
                .detail
                .get("window_ms")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            format!(
                "agent={} subject={} count={} ({}s 윈도우, anomaly={})",
                anomaly.agent,
                anomaly.subject,
                count,
                window_ms / 1000,
                anomaly.id,
            )
        }
        AnomalyKind::RssSurge => {
            let latest = anomaly
                .detail
                .get("latest_rss_bytes")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let min_samples = anomaly
                .detail
                .get("min_samples")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            format!(
                "agent={} subject={} latest_rss_bytes={} ({}개 샘플 연속 증가, anomaly={})",
                anomaly.agent, anomaly.subject, latest, min_samples, anomaly.id,
            )
        }
    };
    let _ = engine; // 옛 직접 add 경로 제거 — cascade 가 라우팅 + add + host event 일괄.
    out.push(
        crate::core::intent::DomainIntent::PushNotification {
            ws_id,
            surface_id: 0,
            title,
            body,
            source: "telemetry.anomaly".to_string(),
        }
        .from_system(),
    );
}

/// `telemetry.anomaly.list` — 영속된 anomaly 레코드 조회. 필터: `agent`, `kind`,
/// `since`, `until` (unix ms). 응답은 `detected_at` 오름차순.
pub fn handle_anomaly_list(
    core: &Core,
    _engine: &mut crate::core::CoreState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let agent_filter = params
        .get("agent")
        .and_then(|v| v.as_str())
        .map(String::from);
    let kind_filter = params
        .get("kind")
        .and_then(|v| v.as_str())
        .map(String::from);
    let since = p_try!(params::opt_int::<u64>(params, "since", &id));
    let until = p_try!(params::opt_int::<u64>(params, "until", &id));

    let list_opts = ListOpts {
        prefix: Some(ANOMALY_KEY_PREFIX.to_string()),
        limit: None,
        since: since.map(|v| v as i64),
        until: until.map(|v| v as i64),
        offset: None,
    };
    let entries = match core.with_memory(|s| s.list(&Scope::Global, &list_opts)) {
        Ok(e) => e,
        Err(e) => return JsonRpcResponse::error(id, -32603, format!("memory list failed: {e}")),
    };
    let mut out: Vec<Anomaly> = Vec::with_capacity(entries.len());
    for entry in entries {
        let MemoryValue::Json(v) = entry.value else {
            continue;
        };
        let Ok(a) = serde_json::from_value::<Anomaly>(v) else {
            continue;
        };
        if let Some(ref agent) = agent_filter
            && &a.agent != agent
        {
            continue;
        }
        if let Some(ref k) = kind_filter
            && a.kind.as_token() != k
        {
            continue;
        }
        if let Some(s) = since
            && a.detected_at < s
        {
            continue;
        }
        if let Some(u) = until
            && a.detected_at >= u
        {
            continue;
        }
        out.push(a);
    }
    out.sort_by_key(|a| a.detected_at);
    let arr: Vec<Value> = out
        .iter()
        .map(|a| serde_json::to_value(a).unwrap_or(Value::Null))
        .collect();
    JsonRpcResponse::success(id, json!({ "entries": arr, "count": arr.len() }))
}
