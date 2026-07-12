//! `IpcHostFacade` trait 의 본 바이너리 impl.
//!
//! Phase F.B.0e-2: IPC dispatcher (caller.rs / audit.rs) 가 본 바이너리 Core 직접
//! 결합을 끊고 `&dyn IpcHostFacade` 만 받도록 한다. 본 모듈이 그 trait 의 단일 impl.

use tasty_ipc::{
    AuditCallerMarker, AuditDecision as ProtoDecision, IpcHostFacade, SessionResolution,
};

use crate::adapters::ipc::audit::{AuditCallerKind, AuditDecision, AuditRecord, AuditStore};
use crate::adapters::ipc::caller::SessionToken;
use crate::core::Core;

impl IpcHostFacade for Core {
    fn session_resolve(&self, token: &str, now_ms: u64) -> SessionResolution {
        let Ok(parsed) = token.parse::<SessionToken>() else {
            // 형식 위반은 NotFound 와 동일 처리 — caller.rs 가 별도 invalid_format
            // 에러를 띄우려면 token 검증을 자체적으로 한 번 더 한다 (현재 그렇게 됨).
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
            let mut store = AuditStore::new(mem, tasty_memory::HOST_OWNER);
            store.append(&record)?;
            // append 경로 retention 집행 (최대 1시간 1회) — query 전용 lazy 만으론
            // 조회가 없는 일반 사용에서 디스크가 무한 축적된다.
            crate::adapters::ipc::audit::maybe_evict_on_append(&mut store, ts_ms);
            Ok::<(), crate::adapters::ipc::audit::AuditError>(())
        });
        if let Err(e) = result {
            tracing::warn!("audit: append failed: {e}");
        }
    }
}
