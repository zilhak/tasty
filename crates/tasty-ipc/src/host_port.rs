//! IPC dispatcher 가 호스트(Core/AppState) 와 결합하지 않도록 좁힌 trait.
//!
//! `caller.rs` / `audit.rs` 는 `&dyn IpcHostFacade` 만 받아 호스트 측 Core 직접
//! 결합을 끊는다. 본 모듈은 그 trait + 보조 enum 의 단일 거주지 — 본 바이너리의
//! `crate::core::ipc_facade` 가 유일한 impl 제공자.

use std::time::Duration;

/// session token 해석 결과. Agent / Plugin / 미존재 케이스를 한 enum 으로 통일.
pub enum SessionResolution {
    /// 토큰이 unknown / expired / revoked.
    NotFound,
    /// 검증 OK — agent caller.
    Agent {
        agent_id: String,
        permissions: Vec<String>,
    },
    /// 검증 OK — plugin caller (다중 plugin 확장 대비).
    Plugin {
        plugin_id: String,
        permissions: Vec<String>,
    },
}

/// audit log 의 caller 표식. enum 으로 좁혀 trait 시그니처에 본 바이너리 타입이
/// 새지 않게 한다.
pub enum AuditCallerMarker {
    Local,
    Plugin(String),
    Agent(String),
}

pub enum AuditDecision {
    Allow,
    Deny,
}

/// IPC dispatcher 가 본 바이너리 Core 대신 의존하는 좁은 trait.
///
/// - `session_resolve` — 토큰 → caller 식별. Core 의 session store 우회 lookup.
/// - `record_audit` — 감사 로그 1 건 기록. 본 바이너리는 memory store 에 append.
pub trait IpcHostFacade: Send + Sync {
    fn session_resolve(&self, token: &str, now_ms: u64) -> SessionResolution;

    #[allow(clippy::too_many_arguments)]
    fn record_audit(
        &self,
        caller: AuditCallerMarker,
        method: &str,
        decision: AuditDecision,
        reason: Option<&str>,
        workspace_id: Option<u32>,
        seq: u64,
        ts_ms: u64,
    );

    /// session token 의 TTL — caller resolution / audit 양쪽 공통 사용.
    /// 일부 구현은 무시 가능. 기본 30 분.
    fn session_ttl(&self) -> Duration {
        Duration::from_secs(30 * 60)
    }
}
