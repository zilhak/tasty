//! Phase 6.2b — Agent session token 영속 + 검증.
//!
//! `claude.spawn` 등으로 호스트가 띄운 자식 프로세스에 1:1 로 발급된
//! [`SessionToken`] 의 라이프사이클을 관리한다. 자식이 IPC envelope 에 토큰을
//! 함께 보내면 [`SessionStore::resolve`] 가 신원을 검증해 [`AgentSession`] 을
//! 돌려준다 — 호스트는 이 정보로 [`CallerContext::Agent`] 를 만들어 권한 게이트를
//! 적용한다.
//!
//! 영속 키:
//! - `tasty.session.<token>` (scope=Global) — token 자체를 key suffix 로 사용.
//!   token 은 64-char lowercase hex 라 memory key 허용 문자(`[a-z0-9._-]`) 안에
//!   들어맞는다. memory.db 디스크 보호는 별도 phase 의 OS keyring/secret scope
//!   이전으로 미룬다 (위협 모델: 신뢰하는 에이전트의 버그 격리).
//!
//! 만료/revoke 정책:
//! - `expires_at_ms` 가 있고 `now_ms` 가 그보다 크면 만료 — `resolve` 가 `None`
//!   을 돌려주고, `list`/`gc` 가 메모리에서 제거.
//! - `revoke` 는 `revoked=true` 로 마킹만 한다. revoked 세션은 `resolve` 에서
//!   `None`. `gc` 가 함께 제거.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use tasty_memory::{ListOpts, MemoryError, MemoryStore, MemoryValue, PutOpts, Scope};

use crate::ipc::caller::SessionToken;
use crate::plugin::manifest::Permission;

pub const SESSION_KEY_PREFIX: &str = "tasty.session.";

fn session_key(token: &SessionToken) -> String {
    format!("{SESSION_KEY_PREFIX}{}", token.as_str())
}

/// 디스크에 저장되는 세션 레코드.
///
/// `permissions` 는 `Permission::as_token` 으로 직렬화 — Permission enum 자체에
/// serde derive 를 강제하지 않기 위함. 역직렬화 시 `Permission::from_token` 으로
/// 복원하며, 알 수 없는 토큰은 조용히 drop 한다 (앞으로 매니페스트에서 빠진
/// permission token 이 디스크에 남아도 정상 동작 유지).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentSession {
    /// 호스트가 부여한 식별자 (예: `claude:child-1`, `cli:0xabcdef`).
    pub agent_id: String,
    /// 부모 caller 식별자 — 보통 plugin_id (claude plugin 이 자식을 spawn 한 경우).
    /// `None` 이면 Local/Internal 이 직접 발급.
    pub parent: Option<String>,
    /// `Permission::as_token` 직렬화. 일부 토큰은 `ipc.invoke:<prefix>` 동적 형태.
    pub permissions: Vec<String>,
    /// unix ms.
    pub created_at_ms: u64,
    /// unix ms. 없으면 자식 프로세스 lifetime 과 동일 (revoke 만으로 종료).
    pub expires_at_ms: Option<u64>,
    /// `revoke` 호출 후 `true`.
    #[serde(default)]
    pub revoked: bool,
}

impl AgentSession {
    /// `Permission` 셋으로 변환 (알 수 없는 토큰은 drop).
    pub fn permission_set(&self) -> HashSet<Permission> {
        self.permissions
            .iter()
            .filter_map(|t| Permission::from_token(t))
            .collect()
    }

    fn is_expired(&self, now_ms: u64) -> bool {
        matches!(self.expires_at_ms, Some(exp) if now_ms >= exp)
    }
}

#[derive(Debug)]
pub enum SessionError {
    InvalidArgument(String),
    Memory(MemoryError),
    Serde(serde_json::Error),
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidArgument(s) => write!(f, "invalid argument: {s}"),
            Self::Memory(e) => write!(f, "memory: {e}"),
            Self::Serde(e) => write!(f, "serde: {e}"),
        }
    }
}

impl std::error::Error for SessionError {}

impl From<MemoryError> for SessionError {
    fn from(e: MemoryError) -> Self {
        Self::Memory(e)
    }
}

impl From<serde_json::Error> for SessionError {
    fn from(e: serde_json::Error) -> Self {
        Self::Serde(e)
    }
}

pub type Result<T> = std::result::Result<T, SessionError>;

pub struct SessionStore<'a> {
    mem: &'a mut MemoryStore,
    owner: String,
}

impl<'a> SessionStore<'a> {
    pub fn new(mem: &'a mut MemoryStore, owner: impl Into<String>) -> Self {
        Self {
            mem,
            owner: owner.into(),
        }
    }

    /// 새 세션 발급. 토큰은 호출자가 만들어 환경변수 등으로 주입할 수 있도록 함께
    /// 돌려준다. agent_id 는 비어 있을 수 없다.
    pub fn issue(
        &mut self,
        agent_id: impl Into<String>,
        parent: Option<String>,
        permissions: impl IntoIterator<Item = Permission>,
        ttl_ms: Option<u64>,
        now_ms: u64,
    ) -> Result<(SessionToken, AgentSession)> {
        let agent_id = agent_id.into();
        if agent_id.is_empty() {
            return Err(SessionError::InvalidArgument(
                "agent_id must be non-empty".into(),
            ));
        }
        let permissions: Vec<String> = permissions
            .into_iter()
            .map(|p| p.as_token())
            .collect();
        let expires_at_ms = ttl_ms.map(|t| now_ms.saturating_add(t));
        let session = AgentSession {
            agent_id,
            parent,
            permissions,
            created_at_ms: now_ms,
            expires_at_ms,
            revoked: false,
        };
        let token = SessionToken::generate();
        self.put(&token, &session)?;
        Ok((token, session))
    }

    fn put(&mut self, token: &SessionToken, session: &AgentSession) -> Result<()> {
        let value = MemoryValue::Json(serde_json::to_value(session)?);
        self.mem.put(
            &self.owner,
            &Scope::Global,
            &session_key(token),
            &value,
            &PutOpts::default(),
        )?;
        Ok(())
    }

    fn get_raw(&self, token: &SessionToken) -> Result<Option<AgentSession>> {
        let entry = self.mem.get(&Scope::Global, &session_key(token))?;
        match entry {
            Some(e) => match e.value {
                MemoryValue::Json(v) => Ok(Some(serde_json::from_value(v)?)),
                _ => Err(SessionError::InvalidArgument(
                    "session entry is not json".into(),
                )),
            },
            None => Ok(None),
        }
    }

    /// 토큰을 검증. 만료/revoked 세션은 `Ok(None)`.
    /// 만료된 항목은 디스크에서도 함께 정리 (lazy gc).
    pub fn resolve(&mut self, token: &SessionToken, now_ms: u64) -> Result<Option<AgentSession>> {
        let session = match self.get_raw(token)? {
            Some(s) => s,
            None => return Ok(None),
        };
        if session.revoked {
            return Ok(None);
        }
        if session.is_expired(now_ms) {
            self.delete(token)?;
            return Ok(None);
        }
        Ok(Some(session))
    }

    /// 토큰 무효화. 존재하지 않으면 `Ok(false)`.
    pub fn revoke(&mut self, token: &SessionToken) -> Result<bool> {
        let mut session = match self.get_raw(token)? {
            Some(s) => s,
            None => return Ok(false),
        };
        session.revoked = true;
        self.put(token, &session)?;
        Ok(true)
    }

    fn delete(&mut self, token: &SessionToken) -> Result<()> {
        self.mem
            .delete(&self.owner, &Scope::Global, &session_key(token), None)?;
        Ok(())
    }

    /// 모든 활성 세션 반환. 만료 항목은 evict.
    pub fn list(&mut self, now_ms: u64) -> Result<Vec<AgentSession>> {
        let opts = ListOpts {
            prefix: Some(SESSION_KEY_PREFIX.to_string()),
            ..Default::default()
        };
        let entries = self.mem.list(&Scope::Global, &opts)?;
        let mut alive = Vec::with_capacity(entries.len());
        let mut to_evict: Vec<String> = Vec::new();
        for e in entries {
            let MemoryValue::Json(v) = e.value else {
                continue;
            };
            let session: AgentSession = match serde_json::from_value(v) {
                Ok(s) => s,
                Err(_) => continue,
            };
            if session.revoked || session.is_expired(now_ms) {
                to_evict.push(e.key);
            } else {
                alive.push(session);
            }
        }
        for key in to_evict {
            let _ = self.mem.delete(&self.owner, &Scope::Global, &key, None);
        }
        Ok(alive)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn fresh() -> (TempDir, MemoryStore) {
        let td = tempfile::tempdir().unwrap();
        let mem = MemoryStore::open(&td.path().join("mem.db")).unwrap();
        (td, mem)
    }

    fn perms(items: &[Permission]) -> Vec<Permission> {
        items.to_vec()
    }

    #[test]
    fn issue_then_resolve_returns_session() {
        let (_td, mut mem) = fresh();
        let mut store = SessionStore::new(&mut mem, "_host");
        let (token, session) = store
            .issue(
                "child:1",
                Some("com.tasty.claude".into()),
                perms(&[Permission::SurfaceRead, Permission::AgentManage]),
                None,
                1_000,
            )
            .unwrap();
        let resolved = store.resolve(&token, 2_000).unwrap().unwrap();
        assert_eq!(resolved.agent_id, "child:1");
        assert_eq!(resolved.parent.as_deref(), Some("com.tasty.claude"));
        assert!(resolved.permission_set().contains(&Permission::SurfaceRead));
        assert!(resolved.permission_set().contains(&Permission::AgentManage));
        assert_eq!(session.created_at_ms, 1_000);
    }

    #[test]
    fn unknown_token_resolves_to_none() {
        let (_td, mut mem) = fresh();
        let mut store = SessionStore::new(&mut mem, "_host");
        let fake = SessionToken::generate();
        assert!(store.resolve(&fake, 1_000).unwrap().is_none());
    }

    #[test]
    fn expired_token_is_evicted_on_resolve() {
        let (_td, mut mem) = fresh();
        let mut store = SessionStore::new(&mut mem, "_host");
        let (token, _) = store
            .issue("a1", None, perms(&[]), Some(1_000), 1_000)
            .unwrap();
        // ttl=1000 → expires_at=2000. now=2000 이면 expire.
        assert!(store.resolve(&token, 2_000).unwrap().is_none());
        // 한 번 더 호출해도 None (이미 evict).
        assert!(store.resolve(&token, 3_000).unwrap().is_none());
    }

    #[test]
    fn revoked_token_resolves_to_none() {
        let (_td, mut mem) = fresh();
        let mut store = SessionStore::new(&mut mem, "_host");
        let (token, _) = store
            .issue("a", None, perms(&[Permission::SurfaceRead]), None, 0)
            .unwrap();
        assert!(store.revoke(&token).unwrap());
        assert!(store.resolve(&token, 1_000).unwrap().is_none());
    }

    #[test]
    fn revoke_unknown_returns_false() {
        let (_td, mut mem) = fresh();
        let mut store = SessionStore::new(&mut mem, "_host");
        let t = SessionToken::generate();
        assert!(!store.revoke(&t).unwrap());
    }

    #[test]
    fn list_returns_alive_only_and_evicts_expired() {
        let (_td, mut mem) = fresh();
        let mut store = SessionStore::new(&mut mem, "_host");
        let (alive_token, _) = store.issue("alive", None, perms(&[]), Some(10_000), 0).unwrap();
        let (revoked_token, _) = store.issue("dead", None, perms(&[]), None, 0).unwrap();
        store.revoke(&revoked_token).unwrap();
        let (_expired_token, _) = store.issue("oldie", None, perms(&[]), Some(1), 0).unwrap();
        let all = store.list(1_000).unwrap();
        assert_eq!(all.len(), 1, "only alive should remain");
        assert_eq!(all[0].agent_id, "alive");
        // alive_token 은 그대로 resolve 가능.
        assert!(store.resolve(&alive_token, 2_000).unwrap().is_some());
    }

    #[test]
    fn empty_agent_id_rejected() {
        let (_td, mut mem) = fresh();
        let mut store = SessionStore::new(&mut mem, "_host");
        let err = store.issue("", None, perms(&[]), None, 0).unwrap_err();
        assert!(matches!(err, SessionError::InvalidArgument(_)));
    }

    #[test]
    fn unknown_permission_tokens_are_dropped_on_load() {
        // 미래에 plugin manifest 에서 permission token 이 사라져도 디스크 데이터는
        // 정상적으로 load 되어야 한다.
        let (_td, mut mem) = fresh();
        let mut store = SessionStore::new(&mut mem, "_host");
        let (token, _) = store
            .issue("a", None, perms(&[Permission::SurfaceRead]), None, 0)
            .unwrap();
        // 강제로 알 수 없는 토큰 삽입.
        let mut session = store.get_raw(&token).unwrap().unwrap();
        session.permissions.push("future.unknown.token".into());
        store.put(&token, &session).unwrap();
        let resolved = store.resolve(&token, 1_000).unwrap().unwrap();
        let set = resolved.permission_set();
        assert!(set.contains(&Permission::SurfaceRead));
        assert_eq!(set.len(), 1, "unknown token dropped");
    }
}
