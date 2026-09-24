//! IPC 크레이트가 Core 타입에 의존하지 않도록 세션 조회와 감사 기록 인터페이스를 구현한다.

use tasty_ipc::{
    AuditCallerMarker, AuditDecision as ProtoDecision, IpcHostFacade, SessionResolution,
};

use crate::core::Core;
use crate::store::audit::{AuditCallerKind, AuditDecision, AuditRecord, AuditStore};
use tasty_ipc::caller::SessionToken;

impl IpcHostFacade for Core {
    fn session_resolve(&self, token: &str, now_ms: u64) -> SessionResolution {
        let Ok(parsed) = token.parse::<SessionToken>() else {
            // 형식 오류와 미등록 token을 여기서는 구별하지 않는다. caller의 형식 검사는 별도다.
            return SessionResolution::NotFound;
        };
        let resolved = match Core::session_resolve(self, &parsed, now_ms) {
            Ok(Some(s)) => s,
            Ok(None) => return SessionResolution::NotFound,
            Err(e) => {
                tracing::warn!("session_resolve lookup failed: {e}");
                return SessionResolution::NotFound;
            }
        };
        let mut perms: Vec<String> = resolved.permissions.to_vec();
        for g in &resolved.temp_grants {
            perms.push(g.permission.clone());
        }
        SessionResolution::Agent {
            agent_id: resolved.agent_id,
            permissions: perms,
        }
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
            AuditCallerMarker::Local => (AuditCallerKind::Local, String::new()),
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
        let result = self.with_memory(|mem| {
            // 조회가 없어도 보관 기간 정리가 실행되도록 기록을 추가할 때도 호출한다.
            crate::store::log_retention::maybe_prune(mem, ts_ms);
            let mut store = AuditStore::new(mem, tasty_memory::HOST_OWNER);
            store.append(&record)?;
            Ok::<(), crate::store::audit::AuditError>(())
        });
        if let Err(e) = result {
            tracing::warn!("audit: append failed: {e}");
        }
    }
}
