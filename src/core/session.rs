//! Session token store wrapper. handler 의 `core.with_memory + SessionStore::new`
//! 조립을 본 모듈로 흡수.
//!
//! 모든 메서드가 **Method call 패턴** (응답 데이터 반환). Intent 가 아님 — 호출자가
//! 즉시 응답을 받아야 한다. agent/ratelimit, agent/semaphore 와 같은 패턴.

use tasty_memory::HOST_OWNER;

use crate::core::Core;
use crate::ipc::caller::SessionToken;
use crate::ipc::session::{AgentSession, SessionError, SessionStore};
use crate::plugin::manifest::Permission;

impl Core {
    /// 새 SessionToken 발급. `parent` 는 부모 caller (Plugin/Agent id 또는
    /// `None`=Local). escalation 검사는 *호출자 책임* — 본 wrapper 는 store 만 만짐.
    pub(crate) fn session_issue(
        &self,
        agent_id: String,
        parent: Option<String>,
        permissions: Vec<Permission>,
        ttl_ms: Option<u64>,
        now_ms: u64,
    ) -> Result<(SessionToken, AgentSession), SessionError> {
        self.with_memory(|mem| {
            let mut store = SessionStore::new(mem, HOST_OWNER);
            store.issue(agent_id, parent, permissions, ttl_ms, now_ms)
        })
    }

    /// 토큰 무효화. 반환: 실제 revoke 여부.
    pub(crate) fn session_revoke(&self, token: &SessionToken) -> Result<bool, SessionError> {
        self.with_memory(|mem| {
            let mut store = SessionStore::new(mem, HOST_OWNER);
            store.revoke(token)
        })
    }

    /// 활성 세션 목록 (만료/revoked 제외).
    pub(crate) fn session_list(&self, now_ms: u64) -> Result<Vec<AgentSession>, SessionError> {
        self.with_memory(|mem| {
            let mut store = SessionStore::new(mem, HOST_OWNER);
            store.list(now_ms)
        })
    }

    /// 들어오는 요청의 token resolve — caller_gate 가 호출. 만료/revoked 면 `Ok(None)`.
    pub(crate) fn session_resolve(
        &self,
        token: &SessionToken,
        now_ms: u64,
    ) -> Result<Option<AgentSession>, SessionError> {
        self.with_memory(|mem| {
            let mut store = SessionStore::new(mem, HOST_OWNER);
            store.resolve(token, now_ms)
        })
    }

    /// agent_id 기반 임시 권한 grant. 반환: `None` 이면 agent_id 의 활성 세션이
    /// 없음. `Some((added, expires_at_ms))` — added=false 이면 base 에 이미 있음.
    pub(crate) fn session_grant_permission_for_agent(
        &self,
        agent_id: &str,
        permission: &str,
        ttl_ms: Option<u64>,
        now_ms: u64,
    ) -> Result<Option<(bool, Option<u64>)>, SessionError> {
        self.with_memory(|mem| {
            let mut store = SessionStore::new(mem, HOST_OWNER);
            let Some((token, _)) = store.find_by_agent_id(agent_id, now_ms)? else {
                return Ok(None);
            };
            let added = store.grant_permission(&token, permission, ttl_ms, now_ms)?;
            let expires_at = ttl_ms.map(|t| now_ms.saturating_add(t));
            Ok(Some((added, expires_at)))
        })
    }

    /// agent_id 기반 임시 권한 revoke. agent 없으면 `Ok(false)`.
    pub(crate) fn session_revoke_permission_for_agent(
        &self,
        agent_id: &str,
        permission: &str,
        now_ms: u64,
    ) -> Result<bool, SessionError> {
        self.with_memory(|mem| {
            let mut store = SessionStore::new(mem, HOST_OWNER);
            let Some((token, _)) = store.find_by_agent_id(agent_id, now_ms)? else {
                return Ok(false);
            };
            store.revoke_permission(&token, permission, now_ms)
        })
    }
}
