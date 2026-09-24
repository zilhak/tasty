//! 승인 요청의 호출자 변환과 상태 영속 저장.
//! workspace가 있으면 해당 scope에, 없으면 global에 저장한다.

use serde_json::{Value, json};
use tasty_approval::{
    ApprovalChoice, ApprovalError, ApprovalId, ApprovalRecord, ApprovalRequest, ApprovalStore,
    Requester, Responder, Severity, WaitOutcome,
};
use tasty_memory::{MemoryValue, PutOpts, Scope};

use crate::core::Core;
use tasty_ipc::caller::CallerContext;
use tasty_ipc::protocol::JsonRpcResponse;

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
        // 요청 형식 오류가 아닌 상태 변경 실패다. 원인은 store에서 기록한다.
        StorePoisoned => JsonRpcResponse::error(id, -32014, "store_poisoned"),
    }
}

const APPROVAL_KEY_PREFIX: &str = "tasty.approval.";

pub(super) fn scope_for(record: &ApprovalRecord) -> Scope {
    match record.request.workspace_id {
        Some(wid) => Scope::Workspace(wid),
        None => Scope::Global,
    }
}

/// Core에 접근할 수 없는 대기 워커가 memory port를 통해 상태를 저장한다.
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

pub(super) fn record_to_json(record: &ApprovalRecord) -> Value {
    serde_json::to_value(record).unwrap_or(Value::Null)
}

/// 권한 요청 레코드를 호출자가 조회할 수 있도록 공통 error.data를 만든다.
pub(crate) fn elevation_error_data(
    record: &ApprovalRecord,
    permission: &str,
    method: &str,
) -> Value {
    json!({
        "kind": "capability_elevation",
        "approval_id": record.request.id,
        "permission": permission,
        "method": method,
    })
}

/// 창 없이 권한 요청을 만든다. GUI 호출자는 publish_capability_elevation으로 팝업도 연다.
pub(crate) fn publish_capability_elevation_at(
    core: &mut crate::core::Core,
    engine: &mut crate::core::CoreState,
    workspace_id: Option<u32>,
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
            Some(change.record)
        }
        Err(e) => {
            tracing::warn!("capability elevation publish failed: {e}");
            None
        }
    }
}

/// 창을 가진 경계용 — [`publish_capability_elevation_at`] 에 팝업 enqueue 를 더한다.
pub(crate) fn publish_capability_elevation(
    core: &mut crate::core::Core,
    window: &mut dyn crate::ipc::window_port::IpcWindow,
    engine: &mut crate::core::CoreState,
    agent_id: &str,
    method: &str,
    permission: &str,
    reason: Option<&str>,
) -> Option<ApprovalRecord> {
    let workspace_id = engine
        .workspaces
        .get(window.active_workspace_index())
        .map(|ws| ws.id);
    let record = publish_capability_elevation_at(
        core,
        engine,
        workspace_id,
        agent_id,
        method,
        permission,
        reason,
    )?;
    #[cfg(feature = "gui")]
    window.enqueue_approval_popup(engine, &record);
    Some(record)
}

/// 승인 응답에서 agent·권한·유효 시간을 추출한다. 거절 응답에는 권한을 부여하지 않는다.
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

/// 완료 상태의 전이 시각을 반환한다. Pending에는 시각이 없다.
pub(super) fn transition_at(state: &tasty_approval::ApprovalState) -> Option<u64> {
    use tasty_approval::ApprovalState as S;
    match state {
        S::Pending => None,
        S::Responded { at, .. } | S::TimedOut { at, .. } | S::Cancelled { at, .. } => Some(*at),
    }
}

const SUMMARY_KEY: &str = "tasty.approval.summary";

mod read;
mod request;
mod respond;
mod summary;

pub(crate) use read::spawn_approval_await;
pub use read::{handle_cancel, handle_get, handle_history, handle_list};
pub use request::handle_request;
pub use respond::handle_respond;
pub use summary::{handle_summary_get, handle_summary_set};

#[cfg(test)]
#[path = "approval/tests.rs"]
mod tests;
