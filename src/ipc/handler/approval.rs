//! `approval.*` IPC 핸들러 — 휴먼 핸드오프 결정 게이트.
//!
//! `tasty-approval` 도메인 store 의 얇은 어댑터. [`CallerContext`] 를
//! `Requester`/`Responder` 로 변환하고, 상태 전이마다 `tasty-memory` 의
//! `tasty.approval.<id>` 키로 영속한다 (workspace 가 주어지면 `workspace:<wid>`,
//! 그 외엔 `global`).

use serde_json::{Value, json};
use tasty_approval::{
    ApprovalChoice, ApprovalError, ApprovalId, ApprovalRecord, ApprovalRequest, ApprovalStore,
    Requester, Responder, Severity, WaitOutcome,
};
use tasty_memory::{MemoryValue, PutOpts, Scope, with_store};

use crate::ipc::caller::CallerContext;
use crate::ipc::protocol::JsonRpcResponse;
use crate::state::AppState;

// ============================================================
// 변환 헬퍼
// ============================================================

/// CallerContext → Requester.
fn requester_from_caller(caller: &CallerContext) -> Requester {
    match caller {
        CallerContext::Local => Requester::User,
        CallerContext::Plugin { plugin_id, .. } => Requester::Plugin {
            id: plugin_id.clone(),
        },
    }
}

/// CallerContext → Responder.
fn responder_from_caller(caller: &CallerContext) -> Responder {
    match caller {
        CallerContext::Local => Responder::User,
        CallerContext::Plugin { plugin_id, .. } => Responder::Agent {
            id: plugin_id.clone(),
        },
    }
}

fn parse_severity(s: &str) -> Option<Severity> {
    match s {
        "info" => Some(Severity::Info),
        "warn" => Some(Severity::Warn),
        "danger" => Some(Severity::Danger),
        _ => None,
    }
}

fn map_error(id: Value, err: ApprovalError) -> JsonRpcResponse {
    use ApprovalError::*;
    match err {
        NotFound(s) => JsonRpcResponse::error(id, -32004, format!("not_found: {s}")),
        AlreadyResponded(aid) => {
            JsonRpcResponse::error(id, -32010, format!("already_responded: {aid}"))
        }
        SelfResponse => JsonRpcResponse::error(id, -32011, "self_response_forbidden"),
        InvalidChoice(c) => JsonRpcResponse::invalid_params(id, format!("invalid_choice: {c}")),
        InvalidRequest(m) => JsonRpcResponse::invalid_params(id, format!("invalid_request: {m}")),
        TimedOut => JsonRpcResponse::error(id, -32012, "timed_out"),
        Cancelled => JsonRpcResponse::error(id, -32013, "cancelled"),
    }
}

// ============================================================
// 영속 — tasty-memory 에 record 를 JSON 으로 보관
// ============================================================

const APPROVAL_KEY_PREFIX: &str = "tasty.approval.";

/// record 의 workspace_id 에 따라 scope 결정. 없으면 global.
fn scope_for(record: &ApprovalRecord) -> Scope {
    match record.request.workspace_id {
        Some(wid) => Scope::Workspace(wid),
        None => Scope::Global,
    }
}

/// 상태 전이마다 호출. memory store 가 초기화되지 않은 환경(테스트 등)에서는
/// silent 통과 — 도메인 상태는 in-memory 에 이미 있다.
fn persist_record(record: &ApprovalRecord) {
    let scope = scope_for(record);
    let key = format!("{}{}", APPROVAL_KEY_PREFIX, record.request.id);
    let value = match serde_json::to_value(record) {
        Ok(v) => MemoryValue::Json(v),
        Err(e) => {
            tracing::warn!("approval: serialize failed for {}: {e}", record.request.id);
            return;
        }
    };
    let opts = PutOpts {
        expires_at: None,
        cas: None,
    };
    let result = with_store(|s| s.put(tasty_memory::HOST_OWNER, &scope, &key, &value, &opts));
    match result {
        Some(Ok(_)) => {}
        Some(Err(e)) => {
            tracing::warn!("approval: memory put failed for {}: {e}", record.request.id);
        }
        None => {
            // store 미초기화 — 테스트 환경.
        }
    }
}

// ============================================================
// JSON 직렬화 헬퍼
// ============================================================

fn record_to_json(record: &ApprovalRecord) -> Value {
    serde_json::to_value(record).unwrap_or(Value::Null)
}

// ============================================================
// 핸들러
// ============================================================

/// `approval.request` — 새 요청 생성. 응답: `{ id, state, record }`.
pub fn handle_request(
    state: &mut AppState,
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
            state
                .engine
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

    match state.engine.approval_store.request(req) {
        Ok(change) => {
            persist_record(&change.record);
            crate::ui::approval_popup::enqueue_approval(state, &change.record);
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

/// `approval.respond` — 응답 제출. self-response 면 거부.
pub fn handle_respond(
    state: &mut AppState,
    caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let req_id = match params.get("id").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => ApprovalId(s.to_string()),
        _ => return JsonRpcResponse::invalid_params(id, "Missing 'id'"),
    };
    let choice = match params.get("choice").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return JsonRpcResponse::invalid_params(id, "Missing 'choice'"),
    };
    let comment = params
        .get("comment")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let by = responder_from_caller(caller);

    match state.engine.approval_store.respond(&req_id, choice, by, comment) {
        Ok(change) => {
            persist_record(&change.record);
            JsonRpcResponse::success(id, record_to_json(&change.record))
        }
        Err(e) => map_error(id, e),
    }
}

/// `approval.cancel` — 종료되지 않은 요청을 취소.
pub fn handle_cancel(
    state: &mut AppState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let req_id = match params.get("id").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => ApprovalId(s.to_string()),
        _ => return JsonRpcResponse::invalid_params(id, "Missing 'id'"),
    };
    match state.engine.approval_store.cancel(&req_id) {
        Ok(change) => {
            persist_record(&change.record);
            JsonRpcResponse::success(id, record_to_json(&change.record))
        }
        Err(e) => map_error(id, e),
    }
}

/// `approval.await` 의 실제 본문 — 메인 스레드를 막지 않도록 워커 스레드에서
/// 호출한다. main.rs::process_ipc 가 Arc<ApprovalStore> 를 클론해 thread::spawn
/// 안에서 이 함수를 호출하고, 응답을 `response_tx` 로 보낸다.
///
/// `timeout_ms` 가 0 또는 null 이면 record 의 `timeout_ms` 사용, 그것도 없으면 무한 대기.
pub fn await_blocking(
    store: &ApprovalStore,
    rpc_id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let req_id = match params.get("id").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => ApprovalId(s.to_string()),
        _ => return JsonRpcResponse::invalid_params(rpc_id, "Missing 'id'"),
    };
    let timeout_ms = match params.get("timeout_ms").and_then(|v| v.as_u64()) {
        Some(0) => None,
        Some(v) => Some(v),
        None => store.get(&req_id).and_then(|r| r.request.timeout_ms),
    };
    let outcome = store.await_response(&req_id, timeout_ms);
    // 상태 전이(timeout 자동 전이 포함) 가 있었으면 영속.
    if let Some(record) = store.get(&req_id) {
        persist_record(&record);
    }
    match outcome {
        Ok(WaitOutcome::Responded { choice, by, comment }) => JsonRpcResponse::success(
            rpc_id,
            json!({
                "outcome": "responded",
                "choice": choice,
                "by": by,
                "comment": comment,
            }),
        ),
        Ok(WaitOutcome::TimedOut { default_choice }) => JsonRpcResponse::success(
            rpc_id,
            json!({
                "outcome": "timed_out",
                "default_choice": default_choice,
            }),
        ),
        Ok(WaitOutcome::Cancelled) => {
            JsonRpcResponse::success(rpc_id, json!({ "outcome": "cancelled" }))
        }
        Err(e) => map_error(rpc_id, e),
    }
}

/// `approval.get` — 단일 record 조회.
pub fn handle_get(
    state: &mut AppState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let req_id = match params.get("id").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => ApprovalId(s.to_string()),
        _ => return JsonRpcResponse::invalid_params(id, "Missing 'id'"),
    };
    match state.engine.approval_store.get(&req_id) {
        Some(rec) => JsonRpcResponse::success(id, record_to_json(&rec)),
        None => JsonRpcResponse::success(id, Value::Null),
    }
}

/// `approval.list` — 전체 record. 필터: `state` (pending|responded|timed_out|cancelled|terminal),
/// `workspace_id`.
pub fn handle_list(
    state: &mut AppState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let state_filter = params.get("state").and_then(|v| v.as_str());
    let workspace_filter = params
        .get("workspace_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);

    let mut records = state.engine.approval_store.list();
    if let Some(f) = state_filter {
        records.retain(|r| {
            use tasty_approval::ApprovalState as S;
            match (f, &r.state) {
                ("pending", S::Pending) => true,
                ("responded", S::Responded { .. }) => true,
                ("timed_out", S::TimedOut { .. }) => true,
                ("cancelled", S::Cancelled { .. }) => true,
                ("terminal", s) => s.is_terminal(),
                _ => false,
            }
        });
    }
    if let Some(wid) = workspace_filter {
        records.retain(|r| r.request.workspace_id == Some(wid));
    }
    records.sort_by_key(|r| r.request.created_at);
    let arr: Vec<Value> = records.iter().map(record_to_json).collect();
    JsonRpcResponse::success(id, json!({ "entries": arr, "count": arr.len() }))
}

// ============================================================
// 부팅 시 memory 에서 rehydrate
// ============================================================

/// 호스트 부팅 시 호출. memory 에 저장된 `tasty.approval.*` 키 전체를 읽어 store
/// 에 다시 주입. 종료된(terminal) 상태도 함께 복원해 history 조회 가능.
#[allow(dead_code)]
pub fn rehydrate(store: &ApprovalStore) {
    // global + 각 workspace scope 를 모두 훑는다. memory 에는 scope 가 입력 시점에
    // 결정되었으니, 가용 scope 를 모두 순회해야 한다.
    let scopes_result = with_store(|s| s.scopes());
    let Some(Ok(scopes)) = scopes_result else {
        return;
    };
    for scope_str in scopes {
        let Ok(scope) = Scope::parse(&scope_str) else {
            continue;
        };
        let opts = tasty_memory::ListOpts {
            prefix: Some(APPROVAL_KEY_PREFIX.to_string()),
            limit: None,
            since: None,
            until: None,
            offset: None,
        };
        let Some(Ok(entries)) = with_store(|s| s.list(&scope, &opts)) else {
            continue;
        };
        for entry in entries {
            if let MemoryValue::Json(v) = entry.value
                && let Ok(record) = serde_json::from_value::<ApprovalRecord>(v)
            {
                store.insert(record);
            }
        }
    }
}
