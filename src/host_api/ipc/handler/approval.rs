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
pub(super) fn requester_from_caller(caller: &CallerContext) -> Requester {
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
pub(super) fn responder_from_caller(caller: &CallerContext) -> Responder {
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

pub(super) fn parse_severity(s: &str) -> Option<Severity> {
    match s {
        "info" => Some(Severity::Info),
        "warn" => Some(Severity::Warn),
        "danger" => Some(Severity::Danger),
        _ => None,
    }
}

pub(super) fn map_error(id: Value, err: ApprovalError) -> JsonRpcResponse {
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
pub(super) fn scope_for(record: &ApprovalRecord) -> Scope {
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

pub(super) fn record_to_json(record: &ApprovalRecord) -> Value {
    serde_json::to_value(record).unwrap_or(Value::Null)
}

// ============================================================
// 핸들러
// ============================================================

/// `approval.request` — 새 요청 생성. 응답: `{ id, state, record }`.
pub(crate) fn publish_capability_elevation(
    state: &mut AppState,
    agent_id: &str,
    method: &str,
    permission: &str,
    reason: Option<&str>,
) -> Option<ApprovalRecord> {
    // 이미 같은 agent+permission 으로 Pending elevation 이 있으면 재사용.
    if let Some(existing) = state.engine.approval_store.list().into_iter().find(|r| {
        matches!(r.state, tasty_approval::ApprovalState::Pending)
            && r.request.metadata.get("kind").and_then(|v| v.as_str())
                == Some("capability_elevation")
            && r.request.metadata.get("agent_id").and_then(|v| v.as_str()) == Some(agent_id)
            && r.request
                .metadata
                .get("permission")
                .and_then(|v| v.as_str())
                == Some(permission)
    }) {
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
            crate::ui::popup::approval::enqueue_approval(state, &change.record);
            Some(change.record)
        }
        Err(e) => {
            tracing::warn!("capability elevation publish failed: {e}");
            None
        }
    }
}

/// `approval.respond` — 응답 제출. self-response 면 거부.
pub(crate) fn elevation_grant_decision(
    record: &ApprovalRecord,
    choice: &str,
) -> Option<(String, String, Option<u64>)> {
    if record.request.metadata.get("kind").and_then(|v| v.as_str()) != Some("capability_elevation")
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
pub(super) fn apply_elevation_grant_if_any(record: &ApprovalRecord, choice: &str) {
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

/// `approval.cancel` — 종료되지 않은 요청을 취소.
/// state 가 가진 timestamp(있다면) 를 추출.
pub(super) fn transition_at(state: &tasty_approval::ApprovalState) -> Option<u64> {
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

mod read;
mod request;
mod respond;
mod summary;

pub use read::{await_blocking, handle_cancel, handle_get, handle_history, handle_list};
pub use request::handle_request;
pub use respond::handle_respond;
pub use summary::{handle_summary_get, handle_summary_set};

#[cfg(test)]
#[path = "approval/tests.rs"]
mod tests;
