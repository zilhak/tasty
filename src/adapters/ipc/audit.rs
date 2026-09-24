//! IPC 호출자를 감사 레코드로 변환해 store에 전달한다.
//! Allow는 저장하지 않는다. 보존 정책은
//! [ADR-0009](../../../docs/adr/0009-state-storage-and-retention.md)를 따른다.

use crate::ipc::caller::CallerContext;
use crate::store::audit::{AuditCallerKind, AuditDecision};

impl AuditCallerKind {
    pub fn from_caller(caller: &CallerContext) -> Self {
        match caller {
            CallerContext::Local => Self::Local,
            CallerContext::Plugin { .. } => Self::Plugin,
            CallerContext::Agent { .. } => Self::Agent,
        }
    }
}

/// 거절된 호출만 감사 로그에 기록한다. 허용된 호출의 사용량 집계는 별도다.
pub fn record(
    host: &dyn tasty_ipc::IpcHostFacade,
    caller: &CallerContext,
    method: &str,
    decision: AuditDecision,
    reason: Option<&str>,
    workspace_id: Option<u32>,
    seq: u64,
) {
    use tasty_ipc::{AuditCallerMarker, AuditDecision as ProtoDecision};

    if decision == AuditDecision::Allow {
        return;
    }

    let ts_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let kind = AuditCallerKind::from_caller(caller);
    let caller_id = caller.agent_id().as_str().to_string();
    let marker = match kind {
        AuditCallerKind::Local => AuditCallerMarker::Local,
        AuditCallerKind::Agent => AuditCallerMarker::Agent(caller_id),
        AuditCallerKind::Plugin => AuditCallerMarker::Plugin(caller_id),
    };
    let proto_decision = match decision {
        AuditDecision::Allow => ProtoDecision::Allow,
        AuditDecision::Deny => ProtoDecision::Deny,
    };
    host.record_audit(
        marker,
        method,
        proto_decision,
        reason,
        workspace_id,
        seq,
        ts_ms,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    /// 메서드와 관계없이 Deny만 기록한다.
    #[test]
    fn allow_is_dropped_before_the_store_and_deny_never_is() {
        use std::sync::Mutex;
        use tasty_ipc::{IpcHostFacade, SessionResolution};

        #[derive(Default)]
        struct CountingFacade {
            recorded: Mutex<Vec<String>>,
        }
        impl IpcHostFacade for CountingFacade {
            fn session_resolve(&self, _token: &str, _now_ms: u64) -> SessionResolution {
                SessionResolution::NotFound
            }
            fn record_audit(
                &self,
                _caller: tasty_ipc::AuditCallerMarker,
                method: &str,
                _decision: tasty_ipc::AuditDecision,
                _reason: Option<&str>,
                _workspace_id: Option<u32>,
                _seq: u64,
                _ts_ms: u64,
            ) {
                self.recorded.lock().unwrap().push(method.to_string());
            }
        }

        let host = CountingFacade::default();
        let caller = CallerContext::Local;
        for method in [
            "terminal.parent",
            "surface.read_since_mark",
            "terminal.state",
            "surface.foreground_process",
        ] {
            record(&host, &caller, method, AuditDecision::Allow, None, None, 0);
        }
        assert!(
            host.recorded.lock().unwrap().is_empty(),
            "allow 는 기록되지 않는다"
        );

        // 폴링 메서드도 거절되면 기록한다.
        record(
            &host,
            &caller,
            "terminal.state",
            AuditDecision::Deny,
            Some("permission_denied"),
            None,
            1,
        );
        assert_eq!(
            *host.recorded.lock().unwrap(),
            vec!["terminal.state".to_string()]
        );
    }
}
