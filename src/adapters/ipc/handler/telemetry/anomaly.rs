//! `telemetry.anomaly.*` — anomaly 영속/조회/발동.
//!
//! 보존은 관측 로그 3종 공통 정책(`adapters::ipc::log_retention`)을 따른다. 과거
//! 이 로그만 TTL 도 부팅 정리 목록도 없어 **재시작해도 영원히 남는** 유일한 유입원
//! 이었다(18시간 실행 21,102건). 근거는
//! [ADR-0085](../../../../../docs/adr/0085-ipc-log-retention-bounded.md).

use serde_json::{Value, json};
use tasty_memory::{ListOpts, MemoryValue, PutOpts, Scope};
use tasty_telemetry::{ANOMALY_KEY_PREFIX, Anomaly, AnomalyKind, anomaly_key};

use crate::core::Core;
use crate::state::AppState;
use tasty_ipc::caller::CallerContext;
use tasty_ipc::protocol::JsonRpcResponse;

/// anomaly 1건 영속. **정리 경로를 두 겹으로 건다** — TTL 하나만으로는 부족하다.
///
/// `expires_at` 은 만료된 행을 조회에서 빼줄 뿐, 디스크에서 지우지는 않는다
/// (`purge_expired` 는 `memory.gc` IPC 에서만 돌고 자동 호출자가 없다). 즉 TTL 만
/// 걸면 "조회에는 안 보이는데 파일은 계속 커지는" 상태가 되어, 이 로그가 원래 앓던
/// 문제가 그대로 남는다. 물리 삭제는 [`log_retention`](crate::adapters::ipc::log_retention)
/// 의 상한이 부팅·런타임 양쪽에서 수행한다.
pub(super) fn persist_anomaly(core: &Core, anomaly: &Anomaly) -> std::result::Result<(), String> {
    let key = anomaly_key(anomaly.detected_at, &anomaly.id);
    let value = MemoryValue::Json(serde_json::to_value(anomaly).map_err(|e| e.to_string())?);
    let opts = PutOpts {
        expires_at: Some(
            (anomaly.detected_at + crate::adapters::ipc::log_retention::LOG_TTL_MS) as i64,
        ),
        cas: None,
    };
    core.with_memory(|s| {
        // 다른 두 로그와 같은 게이트 — anomaly 만 유입되는 인스턴스에서도 정리가 돈다.
        crate::adapters::ipc::log_retention::maybe_prune_on_append(s, anomaly.detected_at);
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
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    anomaly: &Anomaly,
) {
    let Some(ws) = engine.workspaces.get(state.active_workspace) else {
        return;
    };
    let ws_id = ws.id;
    let title = format!(
        "이상 탐지: {} ({})",
        anomaly.kind.as_token(),
        anomaly.subject
    );
    // kind 마다 detail 의 필드 구성이 달라(CallBurst/SlowLoop 는 window_ms+count,
    // RssSurge 는 min_samples+latest_rss_bytes) 본문도 분기한다 — 과거엔
    // `CALL_BURST_WINDOW_MS` 를 kind 무관하게 하드코딩해 SlowLoop/RssSurge 에
    // 잘못된 윈도우 값을 표시했었다.
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
    state.dispatch_intent(
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
    _state: &mut AppState,
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
    let since = params.get("since").and_then(|v| v.as_u64());
    let until = params.get("until").and_then(|v| v.as_u64());

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
