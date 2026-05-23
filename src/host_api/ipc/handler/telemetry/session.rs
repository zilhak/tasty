//! `telemetry.session_summary` — 세션 단위 집계 요약 + markdown 렌더.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use tasty_memory::{ListOpts, MemoryValue, Scope, with_store};

use super::query::{QueryFilter, collect_events};
use tasty_telemetry::{ANOMALY_KEY_PREFIX, Anomaly};

use crate::ipc::caller::CallerContext;
use crate::ipc::protocol::JsonRpcResponse;
use crate::state::AppState;

pub fn handle_session_summary(
    _state: &mut AppState,
    _engine: &mut crate::engine_state::EngineState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    // workspace_id 가 없으면 모든 workspace 를 합산한다 — 포커스 독립 원칙.
    let workspace_id = params
        .get("workspace_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);
    let since = params.get("since").and_then(|v| v.as_u64());
    let until = params.get("until").and_then(|v| v.as_u64());
    let format = params
        .get("format")
        .and_then(|v| v.as_str())
        .unwrap_or("markdown")
        .to_string();
    if format != "markdown" && format != "json" {
        return JsonRpcResponse::invalid_params(
            id,
            format!("invalid 'format' '{format}' (markdown|json)"),
        );
    }
    let top_n_size = params
        .get("top_n")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(10);

    let summary = match build_session_summary(workspace_id, since, until, top_n_size) {
        Ok(s) => s,
        Err(e) => return JsonRpcResponse::error(id, -32603, e),
    };

    if format == "json" {
        return JsonRpcResponse::success(id, json!({ "format": "json", "summary": summary }));
    }
    let md = render_summary_markdown(&summary);
    JsonRpcResponse::success(id, json!({ "format": "markdown", "summary": md }))
}

/// 집계 결과. workspace_id / since / until 은 입력 그대로 echo.
#[derive(serde::Serialize)]
pub(super) struct SessionSummary {
    workspace_id: Option<u32>,
    since: Option<u64>,
    until: Option<u64>,
    /// metric 별 sum (`ipc_calls` 는 제외 — `ipc_calls.total` 로 분리).
    tokens: serde_json::Map<String, Value>,
    ipc_calls_total: u64,
    ipc_calls_top: Vec<(String, u64)>, // (method, count)
    approvals: ApprovalCounts,
    anomalies: Vec<Anomaly>,
}

#[derive(Default, serde::Serialize)]
pub(super) struct ApprovalCounts {
    total: u64,
    pending: u64,
    responded: u64,
    timed_out: u64,
    cancelled: u64,
    /// 선택된 choice key 별 count (responded 만).
    by_choice: serde_json::Map<String, Value>,
}

pub(super) fn build_session_summary(
    workspace_id: Option<u32>,
    since: Option<u64>,
    until: Option<u64>,
    top_n_size: usize,
) -> std::result::Result<SessionSummary, String> {
    let filter = QueryFilter {
        metric: None,
        agent: None,
        workspace_id,
        since,
        until,
    };
    let events = collect_events(&filter)?;

    // Metric 별 sum. ipc_calls 는 분리.
    let mut metric_sum: BTreeMap<String, f64> = BTreeMap::new();
    let mut ipc_method_count: BTreeMap<String, u64> = BTreeMap::new();
    let mut ipc_total: u64 = 0;
    for ev in &events {
        if ev.metric == "ipc_calls" {
            ipc_total += 1;
            let method = ev
                .tags
                .get("method")
                .cloned()
                .unwrap_or_else(|| "<unknown>".to_string());
            *ipc_method_count.entry(method).or_insert(0) += 1;
        } else {
            *metric_sum.entry(ev.metric.clone()).or_insert(0.0) += ev.value;
        }
    }
    let mut tokens: serde_json::Map<String, Value> = serde_json::Map::new();
    for (k, v) in metric_sum {
        let n = serde_json::Number::from_f64(v).unwrap_or_else(|| 0.into());
        tokens.insert(k, Value::Number(n));
    }
    let mut top: Vec<(String, u64)> = ipc_method_count.into_iter().collect();
    top.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    top.truncate(top_n_size);

    // Approval 집계 — approval.history 의 prefix scan 재사용 패턴.
    let approvals = collect_approvals(workspace_id, since, until)?;

    // Anomaly 집계 — Global scope prefix scan, 윈도우 적용.
    let anomalies = collect_anomalies(since, until)?;

    Ok(SessionSummary {
        workspace_id,
        since,
        until,
        tokens,
        ipc_calls_total: ipc_total,
        ipc_calls_top: top,
        approvals,
        anomalies,
    })
}

pub(super) fn collect_approvals(
    workspace_filter: Option<u32>,
    since: Option<u64>,
    until: Option<u64>,
) -> std::result::Result<ApprovalCounts, String> {
    use tasty_approval::{ApprovalRecord, ApprovalState};
    let scopes = with_store(|s| s.scopes())
        .ok_or_else(|| "memory store unavailable".to_string())?
        .map_err(|e| format!("memory scopes failed: {e}"))?;
    let mut counts = ApprovalCounts::default();
    let mut by_choice: BTreeMap<String, u64> = BTreeMap::new();
    for scope_str in scopes {
        let Ok(scope) = Scope::parse(&scope_str) else {
            continue;
        };
        if let Some(wid) = workspace_filter
            && !matches!(scope, Scope::Workspace(s) if s == wid)
        {
            continue;
        }
        let list_opts = ListOpts {
            prefix: Some("tasty.approval.".to_string()),
            limit: None,
            since: since.map(|v| v as i64),
            until: until.map(|v| v as i64),
            offset: None,
        };
        let Some(Ok(entries)) = with_store(|s| s.list(&scope, &list_opts)) else {
            continue;
        };
        for entry in entries {
            if entry.key == "tasty.approval.summary" {
                continue;
            }
            let MemoryValue::Json(v) = entry.value else {
                continue;
            };
            let Ok(rec) = serde_json::from_value::<ApprovalRecord>(v) else {
                continue;
            };
            counts.total += 1;
            match &rec.state {
                ApprovalState::Pending => counts.pending += 1,
                ApprovalState::Responded { choice, .. } => {
                    counts.responded += 1;
                    *by_choice.entry(choice.clone()).or_insert(0) += 1;
                }
                ApprovalState::TimedOut { .. } => counts.timed_out += 1,
                ApprovalState::Cancelled { .. } => counts.cancelled += 1,
            }
        }
    }
    for (k, v) in by_choice {
        counts.by_choice.insert(k, Value::from(v));
    }
    Ok(counts)
}

pub(super) fn collect_anomalies(
    since: Option<u64>,
    until: Option<u64>,
) -> std::result::Result<Vec<Anomaly>, String> {
    let list_opts = ListOpts {
        prefix: Some(ANOMALY_KEY_PREFIX.to_string()),
        limit: None,
        since: since.map(|v| v as i64),
        until: until.map(|v| v as i64),
        offset: None,
    };
    let Some(list_result) = with_store(|s| s.list(&Scope::Global, &list_opts)) else {
        return Ok(Vec::new());
    };
    let entries = list_result.map_err(|e| format!("memory list failed: {e}"))?;
    let mut out: Vec<Anomaly> = Vec::new();
    for entry in entries {
        let MemoryValue::Json(v) = entry.value else {
            continue;
        };
        let Ok(a) = serde_json::from_value::<Anomaly>(v) else {
            continue;
        };
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
    Ok(out)
}

pub(super) fn render_summary_markdown(s: &SessionSummary) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let _ = writeln!(out, "# 세션 요약");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "- workspace_id: {}",
        s.workspace_id
            .map(|w| w.to_string())
            .unwrap_or_else(|| "(all)".into())
    );
    let _ = writeln!(
        out,
        "- 기간: {} ~ {}",
        s.since
            .map(|v| v.to_string())
            .unwrap_or_else(|| "(시작 없음)".into()),
        s.until
            .map(|v| v.to_string())
            .unwrap_or_else(|| "(끝 없음)".into()),
    );
    let _ = writeln!(out);

    let _ = writeln!(out, "## 메트릭 합계");
    if s.tokens.is_empty() {
        let _ = writeln!(out, "_없음_");
    } else {
        let _ = writeln!(out, "| metric | sum |");
        let _ = writeln!(out, "|---|---|");
        for (k, v) in s.tokens.iter() {
            let _ = writeln!(out, "| `{k}` | {v} |");
        }
    }
    let _ = writeln!(out);

    let _ = writeln!(out, "## IPC 호출");
    let _ = writeln!(out, "- 총 {} 회", s.ipc_calls_total);
    if !s.ipc_calls_top.is_empty() {
        let _ = writeln!(out);
        let _ = writeln!(out, "| method | count |");
        let _ = writeln!(out, "|---|---|");
        for (m, c) in &s.ipc_calls_top {
            let _ = writeln!(out, "| `{m}` | {c} |");
        }
    }
    let _ = writeln!(out);

    let _ = writeln!(out, "## 승인");
    let _ = writeln!(
        out,
        "- total {}, responded {} (pending {}, timed_out {}, cancelled {})",
        s.approvals.total,
        s.approvals.responded,
        s.approvals.pending,
        s.approvals.timed_out,
        s.approvals.cancelled,
    );
    if !s.approvals.by_choice.is_empty() {
        let _ = writeln!(out);
        let _ = writeln!(out, "| choice | count |");
        let _ = writeln!(out, "|---|---|");
        for (k, v) in s.approvals.by_choice.iter() {
            let _ = writeln!(out, "| `{k}` | {v} |");
        }
    }
    let _ = writeln!(out);

    let _ = writeln!(out, "## 이상 신호 (anomalies)");
    if s.anomalies.is_empty() {
        let _ = writeln!(out, "_없음_");
    } else {
        let _ = writeln!(out, "| detected_at | kind | agent | subject | count |");
        let _ = writeln!(out, "|---|---|---|---|---|");
        for a in &s.anomalies {
            let cnt = a.detail.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
            let _ = writeln!(
                out,
                "| {} | `{}` | `{}` | `{}` | {} |",
                a.detected_at,
                a.kind.as_token(),
                a.agent,
                a.subject,
                cnt,
            );
        }
    }
    out
}
