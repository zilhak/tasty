//! 요청 게이트의 세션 판정 본문. 메인 루프는 writer에 넣고 결과를 회차마다 거둔다
//! (`adapters/ipc/handler/gate.rs`). 세션 IPC도 memory writer를 쓴다.

use tasty_ipc::SessionResolution;
use tasty_ipc::caller::SessionToken;
use tasty_ipc::session::{AgentSession, SessionError, SessionPeek, SessionStore, peek_session};
use tasty_memory::HOST_OWNER;

use crate::app::services::AppServices;

/// 판정과, 저장 실패가 아닌 조회 오류의 경고. 경고는 결과를 거둔 쪽이 한 번 남긴다.
pub(crate) struct SessionJudgement {
    pub(crate) resolution: SessionResolution,
    pub(crate) warning: Option<String>,
}

impl SessionJudgement {
    /// 경고를 남기고 판정을 돌려준다.
    pub(crate) fn into_resolution(self) -> SessionResolution {
        if let Some(warning) = self.warning {
            tracing::warn!("{warning}");
        }
        self.resolution
    }
}

/// 판정하지 못한 세션은 없는 세션으로 본다. 직접 읽기의 조회 실패와 같다.
pub(crate) fn lookup_failed(error: impl std::fmt::Display) -> SessionResolution {
    tracing::warn!("session_resolve lookup failed: {error}");
    SessionResolution::NotFound
}

fn resolution_of(session: AgentSession) -> SessionResolution {
    let mut permissions: Vec<String> = session.permissions.to_vec();
    for g in &session.temp_grants {
        permissions.push(g.permission.clone());
    }
    SessionResolution::Agent {
        agent_id: session.agent_id,
        permissions,
    }
}

/// 저장 실패는 writer 요청의 실패로 돌려 확정하지 않는다. 형식 오류처럼 다시 해도 같은 오류는
/// 판정으로 바꾼다(세션 없음과 경고).
fn judged_or_storage_failure(error: SessionError) -> tasty_memory::Result<SessionJudgement> {
    match error {
        SessionError::Memory(e) if e.storage_failure().is_some() => Err(e),
        other => Ok(SessionJudgement {
            resolution: SessionResolution::NotFound,
            warning: Some(format!("session_resolve lookup failed: {other}")),
        }),
    }
}

/// 만료 세션 삭제와 만료 grant 정리를 함께 하는 판정. 다시 실행해도 판정이 같다.
pub(crate) fn resolve_in(
    mem: &mut dyn tasty_memory::MemoryStorage,
    token: &SessionToken,
    now_ms: u64,
) -> tasty_memory::Result<SessionJudgement> {
    let mut store = SessionStore::new(mem, HOST_OWNER);
    match store.resolve(token, now_ms) {
        Ok(Some(session)) => Ok(SessionJudgement {
            resolution: resolution_of(session),
            warning: None,
        }),
        Ok(None) => Ok(SessionJudgement {
            resolution: SessionResolution::NotFound,
            warning: None,
        }),
        Err(e) => judged_or_storage_failure(e),
    }
}

/// 쓰기 없는 판정. 만료 grant를 지워야 판정이 끝나는 세션은 그 쓰기가 실패했을 때처럼 거절한다.
pub(crate) fn peek_in(
    mem: &dyn tasty_memory::MemoryStorage,
    token: &SessionToken,
    now_ms: u64,
) -> tasty_memory::Result<SessionJudgement> {
    match peek_session(mem, token, now_ms) {
        Ok(SessionPeek::Valid(session)) => Ok(SessionJudgement {
            resolution: resolution_of(session),
            warning: None,
        }),
        Ok(SessionPeek::Missing) => Ok(SessionJudgement {
            resolution: SessionResolution::NotFound,
            warning: None,
        }),
        Ok(SessionPeek::NeedsWrite) => Ok(SessionJudgement {
            resolution: SessionResolution::NotFound,
            warning: Some("session_resolve could not remove expired grants".to_string()),
        }),
        Err(e) => judged_or_storage_failure(e),
    }
}

impl AppServices {
    /// 결과를 기다리는 판정. 메인 루프는 쓰지 않는다(시험·facade).
    pub(crate) fn session_resolve_blocking(
        &self,
        token: SessionToken,
        now_ms: u64,
    ) -> SessionResolution {
        match self
            .memory_access()
            .run_blocking(0, move |mem| resolve_in(mem, &token, now_ms))
        {
            Ok(Ok(judgement)) => judgement.into_resolution(),
            Ok(Err(error)) => lookup_failed(error),
            Err(blocked) => lookup_failed(blocked.to_memory_error()),
        }
    }
}
