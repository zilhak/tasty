//! `approval` IPC: request 도메인.

use super::*;

pub fn handle_request(
    state: &mut AppState,
    engine: &mut crate::engine_state::EngineState,
    caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let title = match params.get("title").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return JsonRpcResponse::invalid_params(id, "Missing or empty 'title'"),
    };
    let body = params
        .get("body")
        .and_then(|v| v.as_str())
        .map(str::to_string);

    let choices: Vec<ApprovalChoice> = match params.get("choices") {
        None => Vec::new(),
        Some(Value::Array(arr)) => {
            let mut out = Vec::with_capacity(arr.len());
            for v in arr {
                let key = match v.get("key").and_then(|x| x.as_str()) {
                    Some(k) if !k.is_empty() => k.to_string(),
                    _ => {
                        return JsonRpcResponse::invalid_params(
                            id,
                            "each choice requires 'key' string",
                        );
                    }
                };
                let label = v
                    .get("label")
                    .and_then(|x| x.as_str())
                    .map(str::to_string)
                    .unwrap_or_else(|| key.clone());
                let destructive = v
                    .get("destructive")
                    .and_then(|x| x.as_bool())
                    .unwrap_or(false);
                out.push(ApprovalChoice {
                    key,
                    label,
                    destructive,
                });
            }
            out
        }
        Some(_) => return JsonRpcResponse::invalid_params(id, "'choices' must be an array"),
    };

    let default_choice = params
        .get("default_choice")
        .and_then(|v| v.as_str())
        .map(str::to_string);

    let timeout_ms = params.get("timeout_ms").and_then(|v| v.as_u64());

    let severity = match params.get("severity").and_then(|v| v.as_str()) {
        None => Severity::Info,
        Some(s) => match parse_severity(s) {
            Some(sev) => sev,
            None => {
                return JsonRpcResponse::invalid_params(
                    id,
                    format!("invalid severity '{s}' (info|warn|danger)"),
                );
            }
        },
    };

    let workspace_id = params
        .get("workspace_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .or_else(|| {
            // 미지정이면 활성 워크스페이스로 fallback (편의).
            engine
        .workspaces
                .get(state.active_workspace)
                .map(|ws| ws.id)
        });

    let surface_id = params
        .get("surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);

    let metadata = params.get("metadata").cloned().unwrap_or(Value::Null);

    let req = ApprovalRequest {
        id: ApprovalId::generate(),
        requester: requester_from_caller(caller),
        workspace_id,
        surface_id,
        title,
        body,
        choices,
        default_choice,
        timeout_ms,
        severity,
        created_at: 0,
        metadata,
    };

    match engine.approval_store.request(req) {
        Ok(change) => {
            persist_record(&change.record);
            crate::ui::popup::approval::enqueue_approval(state, engine, &change.record);
            JsonRpcResponse::success(
                id,
                json!({
                    "id": change.record.request.id,
                    "record": record_to_json(&change.record),
                }),
            )
        }
        Err(e) => map_error(id, e),
    }
}
