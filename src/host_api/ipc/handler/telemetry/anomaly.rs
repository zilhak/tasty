//! `telemetry.anomaly.*` — anomaly 영속/조회/발동.

use serde_json::{Value, json};
use tasty_memory::{ListOpts, MemoryValue, PutOpts, Scope, with_store};
use tasty_telemetry::{ANOMALY_KEY_PREFIX, Anomaly, anomaly_key};

use crate::ipc::caller::CallerContext;
use crate::ipc::protocol::JsonRpcResponse;
use crate::state::AppState;

pub(super) fn persist_anomaly(anomaly: &Anomaly) -> std::result::Result<(), String> {
    let key = anomaly_key(anomaly.detected_at, &anomaly.id);
    let value = MemoryValue::Json(serde_json::to_value(anomaly).map_err(|e| e.to_string())?);
    let opts = PutOpts {
        expires_at: None,
        cas: None,
    };
    let result = with_store(|s| {
        s.put(
            tasty_memory::HOST_OWNER,
            &Scope::Global,
            &key,
            &value,
            &opts,
        )
    });
    match result {
        Some(Ok(_)) => Ok(()),
        Some(Err(e)) => Err(format!("memory put failed: {e}")),
        None => Err("memory store unavailable".into()),
    }
}

pub(super) fn fire_anomaly_notification(state: &mut AppState, anomaly: &Anomaly) {
    let Some(ws) = state.engine.workspaces.get(state.active_workspace) else {
        return;
    };
    let ws_id = ws.id;
    let count = anomaly
        .detail
        .get("count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let title = format!(
        "이상 탐지: {} ({})",
        anomaly.kind.as_token(),
        anomaly.subject
    );
    let body = format!(
        "agent={} subject={} count={} ({}s 윈도우, anomaly={})",
        anomaly.agent,
        anomaly.subject,
        count,
        tasty_telemetry::CALL_BURST_WINDOW_MS / 1000,
        anomaly.id,
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
            source: "telemetry.anomaly".to_string(),
        });
    }
}

/// `telemetry.anomaly.list` — 영속된 anomaly 레코드 조회. 필터: `agent`, `kind`,
/// `since`, `until` (unix ms). 응답은 `detected_at` 오름차순.
pub fn handle_anomaly_list(
    _state: &mut AppState,
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
    let since = params.get("since").and_then(|v| v.as_u64());
    let until = params.get("until").and_then(|v| v.as_u64());

    let list_opts = ListOpts {
        prefix: Some(ANOMALY_KEY_PREFIX.to_string()),
        limit: None,
        since: since.map(|v| v as i64),
        until: until.map(|v| v as i64),
        offset: None,
    };
    let Some(list_result) = with_store(|s| s.list(&Scope::Global, &list_opts)) else {
        return JsonRpcResponse::error(id, -32603, "memory store unavailable");
    };
    let entries = match list_result {
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
    out.sort_by(|a, b| a.detected_at.cmp(&b.detected_at));
    let arr: Vec<Value> = out
        .iter()
        .map(|a| serde_json::to_value(a).unwrap_or(Value::Null))
        .collect();
    JsonRpcResponse::success(id, json!({ "entries": arr, "count": arr.len() }))
}
