//! Core의 공유 저장소로 세션을 조회·변경하고 결과를 즉시 반환한다.

use tasty_memory::HOST_OWNER;

use crate::core::Core;
use tasty_ipc::caller::SessionToken;
use tasty_ipc::session::{AgentSession, SessionError, SessionStore};
use tasty_plugin_manifest::Permission;

impl Core {
    /// parent가 None이면 Local 요청이다. 허용된 권한보다 더 부여하는지 검사는 호출자가 해야 한다.
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

    pub(crate) fn session_revoke(&self, token: &SessionToken) -> Result<bool, SessionError> {
        self.with_memory(|mem| {
            let mut store = SessionStore::new(mem, HOST_OWNER);
            store.revoke(token)
        })
    }

    /// 만료·폐기된 세션을 제외한 목록.
    pub(crate) fn session_list(&self, now_ms: u64) -> Result<Vec<AgentSession>, SessionError> {
        self.with_memory(|mem| {
            let mut store = SessionStore::new(mem, HOST_OWNER);
            store.list(now_ms)
        })
    }

    /// 만료·폐기된 token은 Ok(None)이다.
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

    /// 활성 세션이 없으면 None, 기본 권한에 이미 있으면 added=false다.
    /// 반환 만료 시각은 요청 TTL로 계산한 값이며 기존 grant와 합친 최종 만료값과 다를 수 있다.
    #[cfg(feature = "gui")]
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

    /// 해당 agent의 활성 세션이 없으면 Ok(false)다.
    #[cfg(feature = "gui")]
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
