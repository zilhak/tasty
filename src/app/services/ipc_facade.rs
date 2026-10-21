//! IPC 크레이트가 AppServices 타입에 의존하지 않도록 세션 조회와 감사 기록 인터페이스를 구현한다.

use tasty_ipc::{
    AuditCallerMarker, AuditDecision as ProtoDecision, IpcHostFacade, SessionResolution,
};

use crate::app::services::AppServices;
use crate::store::audit::{AuditCallerKind, AuditDecision, AuditRecord, AuditStore};
use tasty_ipc::caller::SessionToken;

impl IpcHostFacade for AppServices {
    /// 결과를 기다리는 판정이다. 요청 게이트는 이 경로를 쓰지 않고 writer 결과를 거둔다.
    fn session_resolve(&self, token: &str, now_ms: u64) -> SessionResolution {
        let Ok(parsed) = token.parse::<SessionToken>() else {
            // 형식 오류와 미등록 token을 여기서는 구별하지 않는다. caller의 형식 검사는 별도다.
            return SessionResolution::NotFound;
        };
        self.session_resolve_blocking(parsed, now_ms)
    }

    fn record_audit(
        &self,
        caller: AuditCallerMarker,
        method: &str,
        decision: ProtoDecision,
        reason: Option<&str>,
        workspace_id: Option<u32>,
        seq: u64,
        ts_ms: u64,
    ) {
        let (caller_kind, caller_id) = match caller {
            AuditCallerMarker::Local(claimed) => {
                (AuditCallerKind::Local, claimed.unwrap_or_default())
            }
            AuditCallerMarker::Plugin(id) => (AuditCallerKind::Plugin, id),
            AuditCallerMarker::Agent(id) => (AuditCallerKind::Agent, id),
        };
        let decision = match decision {
            ProtoDecision::Allow => AuditDecision::Allow,
            ProtoDecision::Deny => AuditDecision::Deny,
        };
        let record = AuditRecord {
            ts_ms,
            seq,
            caller_kind,
            caller_id,
            method: method.to_string(),
            decision,
            reason: reason.map(|s| s.to_string()),
            workspace_id,
        };
        // 감사 기록은 응답을 막지 않는다. 기다리지 않는 writer 요청이며 실패는 경고만 남긴다.
        let bytes = record.method.len() + record.reason.as_ref().map_or(0, String::len);
        self.memory_access().submit_detached(
            tasty_memory::writer::MemoryRequest::unit(bytes, move |mem| {
                let mut store = AuditStore::new(mem, tasty_memory::HOST_OWNER);
                Ok(store.append(&record).map_err(|e| e.to_string()))
            }),
            "audit append",
            Some(Box::new(|output| {
                if let Some(Err(e)) = output.into_value::<Result<(), String>>() {
                    tracing::warn!("audit: append failed: {e}");
                }
            })),
        );
        self.submit_log_prune_if_due(ts_ms);
    }
}
