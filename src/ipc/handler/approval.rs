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
        CallerContext::Agent { agent_id, .. } => Requester::Plugin {
            id: agent_id.clone(),
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
        CallerContext::Agent { agent_id, .. } => Responder::Agent {
            id: agent_id.clone(),
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
pub(crate) fn persist_record(record: &ApprovalRecord) {
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

/// Phase 6.4a — capability_elevation 발행.
///
/// Agent caller 가 권한 부족으로 IPC 호출이 거부될 때 dispatcher 가 호출한다.
/// `tasty.approval.<id>` 영속 + popup enqueue 까지 수행하고, 발행된 record 를
/// 돌려준다. 호출자는 `record.request.id` 를 error.data 에 실어 agent 에게
/// 전달하면 된다.
///
/// 같은 (agent_id, permission) 에 대한 미응답 elevation 이 이미 있으면 그것을
/// 재사용 — 동일 거부가 반복돼도 알림 폭주를 막는다.
pub(crate) fn publish_capability_elevation(
    state: &mut AppState,
    agent_id: &str,
    method: &str,
    permission: &str,
    reason: Option<&str>,
) -> Option<ApprovalRecord> {
    // 이미 같은 agent+permission 으로 Pending elevation 이 있으면 재사용.
    if let Some(existing) = state
        .engine
        .approval_store
        .list()
        .into_iter()
        .find(|r| {
            matches!(r.state, tasty_approval::ApprovalState::Pending)
                && r.request.metadata.get("kind").and_then(|v| v.as_str())
                    == Some("capability_elevation")
                && r.request.metadata.get("agent_id").and_then(|v| v.as_str()) == Some(agent_id)
                && r.request.metadata.get("permission").and_then(|v| v.as_str())
                    == Some(permission)
        })
    {
        return Some(existing);
    }

    let workspace_id = state
        .engine
        .workspaces
        .get(state.active_workspace)
        .map(|ws| ws.id);

    // approve → 기본 TTL (1시간), approve_permanently → 무기한.
    // grant_ttl_secs metadata 는 respond 핸들러가 grant_permission ttl 로 사용.
    let metadata = json!({
        "kind": "capability_elevation",
        "agent_id": agent_id,
        "method": method,
        "permission": permission,
        "reason": reason,
        "grant_ttl_secs": 3600u64,
    });

    let title = format!("Capability request: {permission}");
    let body = Some(format!(
        "Agent '{agent_id}' requires '{permission}' to call '{method}'.{}",
        match reason {
            Some(r) => format!(" Reason: {r}"),
            None => String::new(),
        }
    ));

    let req = ApprovalRequest {
        id: ApprovalId::generate(),
        requester: Requester::Plugin {
            id: agent_id.to_string(),
        },
        workspace_id,
        surface_id: None,
        title,
        body,
        choices: vec![
            ApprovalChoice::approve(),
            ApprovalChoice {
                key: "approve_permanently".to_string(),
                label: "Approve permanently".to_string(),
                destructive: false,
            },
            ApprovalChoice::deny(),
        ],
        default_choice: Some("deny".to_string()),
        timeout_ms: None,
        severity: Severity::Warn,
        created_at: 0,
        metadata,
    };

    match state.engine.approval_store.request(req) {
        Ok(change) => {
            persist_record(&change.record);
            crate::ui::approval_popup::enqueue_approval(state, &change.record);
            Some(change.record)
        }
        Err(e) => {
            tracing::warn!("capability elevation publish failed: {e}");
            None
        }
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

    match state
        .engine
        .approval_store
        .respond(&req_id, choice.clone(), by, comment)
    {
        Ok(change) => {
            persist_record(&change.record);
            // Phase 6.4b — capability_elevation 이 approve* 로 응답되면 대상
            // agent 에 임시 grant 를 적용한다. 실패해도 응답 자체는 유지
            // (grant 가 실패해도 agent 는 retry 시 다시 elevation 을 받게 됨).
            apply_elevation_grant_if_any(&change.record, &choice);
            JsonRpcResponse::success(id, record_to_json(&change.record))
        }
        Err(e) => map_error(id, e),
    }
}

/// Phase 6.4b — elevation record 와 응답 choice 로부터 grant 매개변수를 결정.
///
/// `None` 반환 = grant 안 함 (다른 종류 요청이거나 deny). `Some` 의 ttl_ms 는
/// `None` 이면 무기한 grant, `Some(n)` 이면 n ms 후 만료.
pub(crate) fn elevation_grant_decision(
    record: &ApprovalRecord,
    choice: &str,
) -> Option<(String, String, Option<u64>)> {
    if record.request.metadata.get("kind").and_then(|v| v.as_str())
        != Some("capability_elevation")
    {
        return None;
    }
    let agent_id = record
        .request
        .metadata
        .get("agent_id")
        .and_then(|v| v.as_str())?
        .to_string();
    let permission = record
        .request
        .metadata
        .get("permission")
        .and_then(|v| v.as_str())?
        .to_string();
    let ttl_secs = match choice {
        "approve" => record
            .request
            .metadata
            .get("grant_ttl_secs")
            .and_then(|v| v.as_u64()),
        "approve_permanently" => None,
        _ => return None, // deny / 그 외는 grant 없음.
    };
    let ttl_ms = ttl_secs.map(|s| s.saturating_mul(1000));
    Some((agent_id, permission, ttl_ms))
}

/// I/O wrapper — `elevation_grant_decision` 결과를 SessionStore 에 적용.
fn apply_elevation_grant_if_any(record: &ApprovalRecord, choice: &str) {
    let Some((agent_id, permission, ttl_ms)) = elevation_grant_decision(record, choice) else {
        return;
    };
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let result = with_store(|mem| {
        let mut store = crate::ipc::session::SessionStore::new(mem, tasty_memory::HOST_OWNER);
        let token = match store.find_by_agent_id(&agent_id, now_ms)? {
            Some((t, _)) => t,
            None => {
                return Err(crate::ipc::session::SessionError::InvalidArgument(format!(
                    "no active session for agent_id '{agent_id}'"
                )));
            }
        };
        store.grant_permission(&token, &permission, ttl_ms, now_ms)
    });
    match result {
        Some(Ok(added)) => {
            tracing::info!(
                agent_id,
                permission,
                ttl_ms,
                added,
                "capability_elevation grant applied"
            );
        }
        Some(Err(e)) => {
            tracing::warn!(
                agent_id,
                permission,
                "capability_elevation grant failed: {e}"
            );
        }
        None => {
            tracing::warn!("capability_elevation grant skipped: memory store not initialized");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tasty_approval::ApprovalState;

    fn elevation_record(extra_metadata: Value) -> ApprovalRecord {
        let mut md = json!({
            "kind": "capability_elevation",
            "agent_id": "child:1",
            "permission": "fs.write",
            "grant_ttl_secs": 3600u64,
        });
        if let (Value::Object(base), Value::Object(extra)) = (&mut md, extra_metadata) {
            for (k, v) in extra {
                base.insert(k, v);
            }
        }
        ApprovalRecord {
            request: ApprovalRequest {
                id: ApprovalId::generate(),
                requester: Requester::Plugin {
                    id: "child:1".into(),
                },
                workspace_id: None,
                surface_id: None,
                title: "t".into(),
                body: None,
                choices: vec![],
                default_choice: None,
                timeout_ms: None,
                severity: Severity::Warn,
                created_at: 0,
                metadata: md,
            },
            state: ApprovalState::Pending,
            history: vec![],
        }
    }

    #[test]
    fn approve_yields_finite_ttl_from_metadata() {
        let rec = elevation_record(json!({}));
        let (aid, perm, ttl) = elevation_grant_decision(&rec, "approve").expect("decision");
        assert_eq!(aid, "child:1");
        assert_eq!(perm, "fs.write");
        assert_eq!(ttl, Some(3_600_000));
    }

    #[test]
    fn approve_permanently_yields_no_ttl() {
        let rec = elevation_record(json!({}));
        let (_, _, ttl) =
            elevation_grant_decision(&rec, "approve_permanently").expect("decision");
        assert_eq!(ttl, None);
    }

    #[test]
    fn deny_yields_no_grant() {
        let rec = elevation_record(json!({}));
        assert!(elevation_grant_decision(&rec, "deny").is_none());
    }

    #[test]
    fn non_elevation_record_skipped() {
        let mut rec = elevation_record(json!({}));
        rec.request.metadata = json!({"kind": "other"});
        assert!(elevation_grant_decision(&rec, "approve").is_none());
    }

    #[test]
    fn missing_required_metadata_skipped() {
        let mut rec = elevation_record(json!({}));
        rec.request.metadata = json!({"kind": "capability_elevation"});
        assert!(elevation_grant_decision(&rec, "approve").is_none());
    }

    #[test]
    fn approve_without_grant_ttl_secs_is_indefinite_in_metadata() {
        // grant_ttl_secs 누락 시 approve 는 None (무기한) 으로 fallback.
        let mut rec = elevation_record(json!({}));
        if let Value::Object(m) = &mut rec.request.metadata {
            m.remove("grant_ttl_secs");
        }
        let (_, _, ttl) = elevation_grant_decision(&rec, "approve").expect("decision");
        assert_eq!(ttl, None);
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
// History — memory 의 `tasty.approval.<id>` 키 전체를 시간 기준으로 조회
// ============================================================

/// `approval.history` — 영속 기록 조회. memory 에서 모든 scope 의 approval 항목을
/// 읽어 필터링한다. 필터: `since`/`until` (unix ms, memory updated_at 기준),
/// `workspace_id`, `requester_id`, `decision`, `state`, `limit`.
///
/// 응답: `{ entries: [...], count, returned }`.
pub fn handle_history(
    _state: &mut AppState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let since = params.get("since").and_then(|v| v.as_i64());
    let until = params.get("until").and_then(|v| v.as_i64());
    let workspace_filter = params
        .get("workspace_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);
    let requester_filter = params
        .get("requester_id")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let decision_filter = params
        .get("decision")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let state_filter = params.get("state").and_then(|v| v.as_str()).map(str::to_string);
    let limit = params
        .get("limit")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize);

    let scopes_result = with_store(|s| s.scopes());
    let scopes: Vec<String> = match scopes_result {
        Some(Ok(s)) => s,
        Some(Err(e)) => {
            return JsonRpcResponse::error(id, -32603, format!("memory scopes failed: {e}"));
        }
        None => return JsonRpcResponse::success(id, json!({ "entries": [], "count": 0, "returned": 0 })),
    };

    let mut collected: Vec<ApprovalRecord> = Vec::new();
    for scope_str in scopes {
        let Ok(scope) = Scope::parse(&scope_str) else {
            continue;
        };
        if let Some(wid) = workspace_filter
            && !matches!(scope, Scope::Workspace(s) if s == wid)
        {
            continue;
        }
        let list_opts = tasty_memory::ListOpts {
            prefix: Some(APPROVAL_KEY_PREFIX.to_string()),
            limit: None,
            since,
            until,
            offset: None,
        };
        let Some(Ok(entries)) = with_store(|s| s.list(&scope, &list_opts)) else {
            continue;
        };
        for entry in entries {
            // summary 키는 history 결과에서 제외 (3.5 에서 사용).
            if entry.key == "tasty.approval.summary" {
                continue;
            }
            let MemoryValue::Json(v) = entry.value else { continue };
            let Ok(record) = serde_json::from_value::<ApprovalRecord>(v) else { continue };
            collected.push(record);
        }
    }

    if let Some(ref rid) = requester_filter {
        collected.retain(|r| match &r.request.requester {
            tasty_approval::Requester::User => rid == "user",
            tasty_approval::Requester::Plugin { id } => id == rid,
            tasty_approval::Requester::Agent { id } => id == rid,
        });
    }
    if let Some(ref decision) = decision_filter {
        collected.retain(|r| match &r.state {
            tasty_approval::ApprovalState::Responded { choice, .. } => choice == decision,
            _ => false,
        });
    }
    if let Some(ref sf) = state_filter {
        use tasty_approval::ApprovalState as S;
        collected.retain(|r| {
            matches!(
                (sf.as_str(), &r.state),
                ("pending", S::Pending)
                    | ("responded", S::Responded { .. })
                    | ("timed_out", S::TimedOut { .. })
                    | ("cancelled", S::Cancelled { .. })
            ) || (sf == "terminal" && r.state.is_terminal())
        });
    }

    // 시간 역순 — 최신 응답이 위로.
    collected.sort_by(|a, b| {
        let ta = transition_at(&a.state).unwrap_or(a.request.created_at);
        let tb = transition_at(&b.state).unwrap_or(b.request.created_at);
        tb.cmp(&ta)
    });

    let total = collected.len();
    if let Some(n) = limit {
        collected.truncate(n);
    }
    let returned = collected.len();
    let arr: Vec<Value> = collected.iter().map(record_to_json).collect();
    JsonRpcResponse::success(
        id,
        json!({ "entries": arr, "count": total, "returned": returned }),
    )
}

/// state 가 가진 timestamp(있다면) 를 추출.
fn transition_at(state: &tasty_approval::ApprovalState) -> Option<u64> {
    use tasty_approval::ApprovalState as S;
    match state {
        S::Pending => None,
        S::Responded { at, .. } | S::TimedOut { at, .. } | S::Cancelled { at, .. } => Some(*at),
    }
}

// ============================================================
// 세션 요약 — workspace 별 1개 markdown 텍스트. memory key `tasty.approval.summary`.
// ============================================================

const SUMMARY_KEY: &str = "tasty.approval.summary";

/// `approval.summary.set` — workspace 의 markdown 요약을 저장 (overwrite).
pub fn handle_summary_set(
    _state: &mut AppState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let workspace_id = match params.get("workspace_id").and_then(|v| v.as_u64()) {
        Some(v) => v as u32,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'workspace_id'"),
    };
    let content = match params.get("content").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return JsonRpcResponse::invalid_params(id, "Missing 'content' (string)"),
    };
    let scope = Scope::Workspace(workspace_id);
    let value = MemoryValue::Text(content);
    let opts = PutOpts {
        expires_at: None,
        cas: None,
    };
    match with_store(|s| s.put(tasty_memory::HOST_OWNER, &scope, SUMMARY_KEY, &value, &opts)) {
        Some(Ok(_)) => JsonRpcResponse::success(id, json!({ "workspace_id": workspace_id })),
        Some(Err(e)) => JsonRpcResponse::error(id, -32603, format!("summary set failed: {e}")),
        None => JsonRpcResponse::error(id, -32603, "memory store unavailable"),
    }
}

/// `approval.summary.get` — workspace 의 요약을 반환. 없으면 `content: null`.
pub fn handle_summary_get(
    _state: &mut AppState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let workspace_id = match params.get("workspace_id").and_then(|v| v.as_u64()) {
        Some(v) => v as u32,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'workspace_id'"),
    };
    let scope = Scope::Workspace(workspace_id);
    let entry = with_store(|s| s.get(&scope, SUMMARY_KEY));
    match entry {
        Some(Ok(Some(e))) => {
            let content = match e.value {
                MemoryValue::Text(t) => Some(t),
                MemoryValue::Json(v) => v.as_str().map(str::to_string),
                _ => None,
            };
            JsonRpcResponse::success(
                id,
                json!({
                    "workspace_id": workspace_id,
                    "content": content,
                    "updated_at": e.updated_at,
                }),
            )
        }
        Some(Ok(None)) => JsonRpcResponse::success(
            id,
            json!({ "workspace_id": workspace_id, "content": null }),
        ),
        Some(Err(e)) => JsonRpcResponse::error(id, -32603, format!("summary get failed: {e}")),
        None => JsonRpcResponse::error(id, -32603, "memory store unavailable"),
    }
}

