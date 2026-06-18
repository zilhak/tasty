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
use tasty_memory::{ListOpts, MemoryError, MemoryStorage, MemoryValue, PutOpts, Scope};

use crate::caller::SessionToken;
use tasty_plugin_manifest::Permission;

pub const SESSION_KEY_PREFIX: &str = "tasty.session.";

fn session_key(token: &SessionToken) -> String {
    format!("{SESSION_KEY_PREFIX}{}", token.as_str())
}

/// Phase 6.3 — 임시 권한 grant. base `permissions` 외에 런타임에 추가/회수되는
/// 권한을 별도로 관리해 base 와 분리한다. `expires_at_ms=None` 이면 명시적
/// revoke 전까지 유효, `Some` 이면 시점 도달 시 자동 만료.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TempGrant {
    /// `Permission::as_token` 직렬화 (forward-compat 동일 정책).
    pub permission: String,
    /// unix ms. `None` 이면 무기한.
    pub expires_at_ms: Option<u64>,
}

impl TempGrant {
    fn is_expired(&self, now_ms: u64) -> bool {
        matches!(self.expires_at_ms, Some(exp) if now_ms >= exp)
    }
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
    /// 발급 시 부여된 base permission. `Permission::as_token` 직렬화.
    /// 일부 토큰은 `ipc.invoke:<prefix>` 동적 형태.
    pub permissions: Vec<String>,
    /// Phase 6.3 — 런타임에 추가된 임시 grant. base 와 합쳐 effective set 을 만든다.
    #[serde(default)]
    pub temp_grants: Vec<TempGrant>,
    /// unix ms.
    pub created_at_ms: u64,
    /// unix ms. 없으면 자식 프로세스 lifetime 과 동일 (revoke 만으로 종료).
    pub expires_at_ms: Option<u64>,
    /// `revoke` 호출 후 `true`.
    #[serde(default)]
    pub revoked: bool,
}

impl AgentSession {
    /// base + 만료되지 않은 temp grant 의 합집합. 알 수 없는 토큰은 drop.
    pub fn permission_set(&self) -> HashSet<Permission> {
        // now 미지정 호출 호환을 위해 만료 검사 없이 모두 합친다 — 만료된 항목은
        // store level 에서 evict 후 호출돼야 한다. 호출자가 만료를 신경 쓰지 않는
        // 만료된 temp grant 도 그대로 포함됨 — resolve 가 store 에서 만료 처리한
        // 후 호출되는 것이 일반 흐름.
        self.permissions
            .iter()
            .chain(self.temp_grants.iter().map(|g| &g.permission))
            .filter_map(|t| Permission::from_token(t))
            .collect()
    }

    fn is_expired(&self, now_ms: u64) -> bool {
        matches!(self.expires_at_ms, Some(exp) if now_ms >= exp)
    }

    /// 만료된 temp grant 제거. 변화가 있었으면 `true`.
    fn evict_expired_grants(&mut self, now_ms: u64) -> bool {
        let before = self.temp_grants.len();
        self.temp_grants.retain(|g| !g.is_expired(now_ms));
        before != self.temp_grants.len()
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
    mem: &'a mut dyn MemoryStorage,
    owner: String,
}

impl<'a> SessionStore<'a> {
    pub fn new(mem: &'a mut dyn MemoryStorage, owner: impl Into<String>) -> Self {
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
        let permissions: Vec<String> = permissions.into_iter().map(|p| p.as_token()).collect();
        let expires_at_ms = ttl_ms.map(|t| now_ms.saturating_add(t));
        let session = AgentSession {
            agent_id,
            parent,
            permissions,
            temp_grants: Vec::new(),
            created_at_ms: now_ms,
            expires_at_ms,
            revoked: false,
        };
        let token = SessionToken::generate();
        self.put(&token, &session)?;
        Ok((token, session))
    }

    pub(crate) fn put(&mut self, token: &SessionToken, session: &AgentSession) -> Result<()> {
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

    pub(crate) fn get_raw(&self, token: &SessionToken) -> Result<Option<AgentSession>> {
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
    /// 만료된 temp grant 는 leave 시 evict 후 persist 한다.
    pub fn resolve(&mut self, token: &SessionToken, now_ms: u64) -> Result<Option<AgentSession>> {
        let mut session = match self.get_raw(token)? {
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
        if session.evict_expired_grants(now_ms) {
            self.put(token, &session)?;
        }
        Ok(Some(session))
    }

    /// 임시 권한 grant. 이미 같은 token 의 grant 가 있으면 `expires_at_ms` 를 갱신
    /// (가장 늦은 만료 시점 또는 None=무기한 우선). base permission 에 이미 있는
    /// token 은 grant 가 의미 없으므로 skip 하고 `Ok(false)`.
    /// 알 수 없는 토큰은 InvalidArgument.
    pub fn grant_permission(
        &mut self,
        token: &SessionToken,
        permission: &str,
        ttl_ms: Option<u64>,
        now_ms: u64,
    ) -> Result<bool> {
        if Permission::from_token(permission).is_none() {
            return Err(SessionError::InvalidArgument(format!(
                "unknown permission token: {permission}"
            )));
        }
        let mut session = match self.get_raw(token)? {
            Some(s) => s,
            None => {
                return Err(SessionError::InvalidArgument(
                    "session token not found".into(),
                ));
            }
        };
        if session.revoked || session.is_expired(now_ms) {
            return Err(SessionError::InvalidArgument(
                "session is revoked or expired".into(),
            ));
        }
        session.evict_expired_grants(now_ms);
        if session.permissions.iter().any(|p| p == permission) {
            return Ok(false);
        }
        let new_expires = ttl_ms.map(|t| now_ms.saturating_add(t));
        if let Some(existing) = session
            .temp_grants
            .iter_mut()
            .find(|g| g.permission == permission)
        {
            existing.expires_at_ms = match (existing.expires_at_ms, new_expires) {
                (None, _) | (_, None) => None,
                (Some(a), Some(b)) => Some(a.max(b)),
            };
        } else {
            session.temp_grants.push(TempGrant {
                permission: permission.to_string(),
                expires_at_ms: new_expires,
            });
        }
        self.put(token, &session)?;
        Ok(true)
    }

    /// 임시 권한 회수. 해당 grant 가 없으면 `Ok(false)`. base permission 은 건드리지
    /// 않는다 (revoke 의미가 다름 — 발급 시점에 통제).
    pub fn revoke_permission(
        &mut self,
        token: &SessionToken,
        permission: &str,
        now_ms: u64,
    ) -> Result<bool> {
        let mut session = match self.get_raw(token)? {
            Some(s) => s,
            None => return Ok(false),
        };
        let before = session.temp_grants.len();
        session.temp_grants.retain(|g| g.permission != permission);
        let removed = before != session.temp_grants.len();
        // 만료 evict 도 함께 — 어차피 저장하니까.
        let evicted = session.evict_expired_grants(now_ms);
        if removed || evicted {
            self.put(token, &session)?;
        }
        Ok(removed)
    }

    /// agent_id 로 활성 세션 검색. 동일 agent_id 가 여러 개면 첫 매치 반환.
    /// 만료된 temp grant 는 evict 후 반환.
    pub fn find_by_agent_id(
        &mut self,
        agent_id: &str,
        now_ms: u64,
    ) -> Result<Option<(SessionToken, AgentSession)>> {
        let opts = ListOpts {
            prefix: Some(SESSION_KEY_PREFIX.to_string()),
            ..Default::default()
        };
        let entries = self.mem.list(&Scope::Global, &opts)?;
        for e in entries {
            let MemoryValue::Json(v) = e.value else {
                continue;
            };
            let mut session: AgentSession = match serde_json::from_value(v) {
                Ok(s) => s,
                Err(_) => continue,
            };
            if session.agent_id != agent_id {
                continue;
            }
            if session.revoked || session.is_expired(now_ms) {
                continue;
            }
            let token_str = e.key.strip_prefix(SESSION_KEY_PREFIX).unwrap_or(&e.key);
            let token = match token_str.parse::<SessionToken>() {
                Ok(t) => t,
                Err(_) => continue,
            };
            if session.evict_expired_grants(now_ms) {
                self.put(&token, &session)?;
            }
            return Ok(Some((token, session)));
        }
        Ok(None)
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

    /// 모든 활성 세션 반환. 만료된 세션은 evict, 만료된 temp grant 는 persist 갱신.
    pub fn list(&mut self, now_ms: u64) -> Result<Vec<AgentSession>> {
        let opts = ListOpts {
            prefix: Some(SESSION_KEY_PREFIX.to_string()),
            ..Default::default()
        };
        let entries = self.mem.list(&Scope::Global, &opts)?;
        let mut alive = Vec::with_capacity(entries.len());
        let mut to_evict: Vec<String> = Vec::new();
        let mut to_resave: Vec<(SessionToken, AgentSession)> = Vec::new();
        for e in entries {
            let MemoryValue::Json(v) = e.value else {
                continue;
            };
            let mut session: AgentSession = match serde_json::from_value(v) {
                Ok(s) => s,
                Err(_) => continue,
            };
            if session.revoked || session.is_expired(now_ms) {
                to_evict.push(e.key);
                continue;
            }
            if session.evict_expired_grants(now_ms) {
                let token_str = e.key.strip_prefix(SESSION_KEY_PREFIX).unwrap_or(&e.key);
                if let Ok(t) = token_str.parse::<SessionToken>() {
                    to_resave.push((t, session.clone()));
                }
            }
            alive.push(session);
        }
        for key in to_evict {
            let _ = self.mem.delete(&self.owner, &Scope::Global, &key, None);  // best-effort 만료 키 제거 — 실패 무시
        }
        for (t, s) in to_resave {
            let _ = self.put(&t, &s);  // best-effort 재저장 — 실패 무시
        }
        Ok(alive)
    }
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
