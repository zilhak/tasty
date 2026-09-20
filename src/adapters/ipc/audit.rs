//! IPC audit log 의 **dispatcher hook**.
//!
//! 레코드를 어디에 쌓는지는 [`crate::store::audit`] 가 안다. 이 모듈은 그 앞단만
//! 맡는다 — `CallerContext` 에서 호출자 종류를 읽고, allow 를 걸러내고, protocol
//! 타입으로 옮겨 [`tasty_ipc::IpcHostFacade::record_audit`] 를 부른다.
//!
//! **`Allow` 는 기록하지 않는다** — 그 정책의 근거와 이 선택이 무엇을 버리는지는
//! [ADR-0085](../../../docs/adr/0085-ipc-log-retention-bounded.md).

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

/// dispatcher hook — IPC call 한 건을 audit log 에 기록한다.
/// `record_ipc_call` (telemetry) 와 짝을 이루며 dispatcher 경로의 모든 진입점에서
/// 호출된다.
///
/// **`Allow` 는 기록하지 않고 즉시 반환한다** — 그 정책의 근거와 이 선택이 무엇을
/// 버리는지는 [ADR-0085](../../../docs/adr/0085-ipc-log-retention-bounded.md).
/// 게이트를 통과한 호출은 전부 여기로 오므로, 기록을 여기서 끊으면 dispatcher 의
/// 어느 진입점이 늘어나도 다시 새지 않는다.
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
    /// `record()` 의 정책 게이트. allow 는 store 근처에도 못 가고, deny 는
    /// **method 와 무관하게** 전부 간다 — 축소 대상은 allow 뿐이라는 것이 계약이다.
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
        // audit 행의 85% 를 만들던 폴링 4종 — allow 로는 한 건도 남지 않아야 한다.
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

        // 같은 폴링 method 라도 deny 면 기록된다 — 제외 목록이 아니라 decision 기준.
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
