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
use tasty_memory::{MemoryValue, PutOpts, Scope};

use crate::core::Core;
use crate::state::AppState;
use tasty_ipc::caller::CallerContext;
use tasty_ipc::protocol::JsonRpcResponse;

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
        // 상태 변경을 거절한 것이지 요청이 잘못된 것이 아니다 — invalid_params 가 아닌
        // 서버측 에러 코드로 낸다. 원인 로그는 store 가 남긴다.
        StorePoisoned => JsonRpcResponse::error(id, -32014, "store_poisoned"),
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

/// Worker thread 용 — `core` 가 도달하지 못하는 thread 에서, memory port 의
/// Arc clone 으로 직접 영속한다. `await_blocking` 전용. 메인 스레드는
/// `persist_record(core, ...)` 사용.
pub(super) fn persist_record_via_arc(
    memory: &std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>>,
    record: &ApprovalRecord,
) {
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
    let mut guard = crate::poison::recover_mutex(
        memory.lock(),
        crate::core::MEMORY_WHAT,
        &crate::core::MEMORY_POISONED,
    );
    if let Err(e) = guard.put(tasty_memory::HOST_OWNER, &scope, &key, &value, &opts) {
        tracing::warn!("approval: memory put failed for {}: {e}", record.request.id);
    }
}

pub(crate) fn persist_record(core: &Core, record: &ApprovalRecord) {
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
    if let Err(e) =
        core.with_memory(|s| s.put(tasty_memory::HOST_OWNER, &scope, &key, &value, &opts))
    {
        tracing::warn!("approval: memory put failed for {}: {e}", record.request.id);
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
    core: &mut crate::core::Core,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    agent_id: &str,
    method: &str,
    permission: &str,
    reason: Option<&str>,
) -> Option<ApprovalRecord> {
    // 이미 같은 agent+permission 으로 Pending elevation 이 있으면 재사용.
    if let Some(existing) = engine.approval_store.list().into_iter().find(|r| {
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

    let workspace_id = engine
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

    match core.request_approval(engine, req) {
        Ok(change) => {
            persist_record(core, &change.record);
            #[cfg(feature = "gui")]
            crate::adapters::ui::popup::approval::enqueue_approval(state, engine, &change.record);
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
pub(super) fn apply_elevation_grant_if_any(core: &Core, record: &ApprovalRecord, choice: &str) {
    let Some((agent_id, permission, ttl_ms)) = elevation_grant_decision(record, choice) else {
        return;
    };
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let result = core.with_memory(|mem| {
        let mut store = crate::ipc::session::SessionStore::new(mem, tasty_memory::HOST_OWNER);
        let token = match store.find_by_agent_id(&agent_id, now_ms)? {
            Some((t, _)) => t,
            None => {
                return Err(tasty_ipc::session::SessionError::InvalidArgument(format!(
                    "no active session for agent_id '{agent_id}'"
                )));
            }
        };
        store.grant_permission(&token, &permission, ttl_ms, now_ms)
    });
    match result {
        Ok(added) => {
            tracing::info!(
                agent_id,
                permission,
                ttl_ms,
                added,
                "capability_elevation grant applied"
            );
        }
        Err(e) => {
            tracing::warn!(
                agent_id,
                permission,
                "capability_elevation grant failed: {e}"
            );
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
