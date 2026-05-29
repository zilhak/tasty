//! IPC 호출자 컨텍스트 — local CLI/사용자 vs plugin process vs agent(자식 Claude 등) 구분.
//!
//! `route_engine_handler`/`route_gui_handler` 진입점에서 `CallerContext::ensure_allowed`로
//! 권한을 검사한 뒤 분기한다. Local/Internal 은 모든 메서드 통과, Plugin/Agent 는
//! 매니페스트(또는 grant) 에 선언된 권한과 [`crate::ipc::method_meta`] 테이블을 대조한다.

use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;

use crate::ipc::method_meta::method_meta;
use crate::plugin::manifest::Permission;

/// Phase 6.2 — 자식 프로세스에 발급하는 32바이트 random session token.
///
/// 환경변수 `TASTY_SESSION_TOKEN` 으로 자식에게 주입되고, 자식이 IPC envelope 에
/// 함께 실어 보내면 호스트가 `SessionStore` 에서 검증해 `CallerContext::Agent` 로
/// resolve 한다. 위조 방어가 1차 목표 — agent_id 만으로는 환경변수 set 만으로
/// 가장 가능하므로 token 검증이 필수.
///
/// 인코딩은 hex (base64 의존성 추가 회피). 64 char ascii.
#[derive(Clone, Eq, PartialEq, Hash)]
pub struct SessionToken(String);

impl SessionToken {
    /// 32바이트 OS random → hex 64 chars.
    pub fn generate() -> Self {
        use rand::RngCore;
        let mut bytes = [0u8; 32];
        rand::rng().fill_bytes(&mut bytes);
        let mut s = String::with_capacity(64);
        for b in bytes {
            use std::fmt::Write;
            write!(&mut s, "{b:02x}").unwrap();
        }
        Self(s)
    }

    /// 외부 입력(envelope, env) 으로부터 token 을 받는다. hex 64 chars 검증.
    pub fn from_str(s: &str) -> Option<Self> {
        if s.len() != 64 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        Some(Self(s.to_ascii_lowercase()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SessionToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // 토큰 자체는 비밀이므로 prefix 만 노출.
        write!(f, "SessionToken({}…)", &self.0[..8])
    }
}

impl fmt::Display for SessionToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// IPC 명령을 누가 보냈는가.
#[derive(Debug, Clone)]
pub enum CallerContext {
    /// CLI/네트워크 IPC 클라이언트(사용자) — 모든 메서드 자동 허용.
    /// TCP 포트는 로컬 기기 액세스를 전제로 하므로 별도 권한 검사 없음.
    Local,
    /// 외부 plugin process가 호출. 매니페스트의 `permissions`만 허용.
    Plugin {
        plugin_id: String,
        /// 매니페스트에 선언되고 사용자가 grant한 권한. 런타임에 in-place로
        /// 갱신되지 않고 전체를 새 Arc로 교체하므로, 동시 호출 안전.
        permissions: Arc<HashSet<Permission>>,
    },
    /// Phase 6.2 — claude.spawn 등으로 호스트가 띄운 child Claude / 외부 agent.
    /// SessionToken 으로 신원 검증한 뒤에만 이 variant 가 만들어진다.
    Agent {
        /// 'child:1', 'claude:abc' 등 호스트-부여 식별자.
        agent_id: String,
        /// 부모로부터 상속된 권한 + grant 로 보강된 권한.
        permissions: Arc<HashSet<Permission>>,
    },
}

#[derive(Debug)]
pub enum CallerError {
    UnknownMethod(String),
    NotPluginCallable {
        caller_label: String,
        method: String,
    },
    MissingPermission {
        caller_label: String,
        method: String,
        permission: Permission,
    },
}

impl std::fmt::Display for CallerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CallerError::UnknownMethod(m) => write!(f, "unknown ipc method: {m}"),
            CallerError::NotPluginCallable {
                caller_label,
                method,
            } => {
                write!(f, "method '{method}' is not callable from '{caller_label}'")
            }
            CallerError::MissingPermission {
                caller_label,
                method,
                permission,
            } => write!(
                f,
                "'{caller_label}' missing permission '{}' for method '{method}'",
                permission.as_token()
            ),
        }
    }
}

impl std::error::Error for CallerError {}

impl CallerContext {
    /// 호출하려는 메서드가 caller에게 허용되는지 확인.
    pub fn ensure_allowed(&self, method: &str) -> Result<(), CallerError> {
        match self {
            CallerContext::Local => Ok(()),
            CallerContext::Plugin {
                plugin_id,
                permissions,
            } => check_permissions(plugin_id, permissions, method),
            CallerContext::Agent {
                agent_id,
                permissions,
                ..
            } => check_permissions(agent_id, permissions, method),
        }
    }

    pub fn is_plugin(&self) -> bool {
        matches!(self, CallerContext::Plugin { .. })
    }

    /// 권한 셋 접근. Local/Internal 은 None (무제한이므로 셋 자체가 의미 없음).
    pub fn permissions(&self) -> Option<&Arc<HashSet<Permission>>> {
        match self {
            CallerContext::Plugin { permissions, .. }
            | CallerContext::Agent { permissions, .. } => Some(permissions),
            _ => None,
        }
    }

    /// memory.db `owner` 값 도출.
    pub fn owner(&self) -> &str {
        match self {
            CallerContext::Local => tasty_memory::HOST_OWNER,
            CallerContext::Plugin { plugin_id, .. } => plugin_id.as_str(),
            CallerContext::Agent { agent_id, .. } => agent_id.as_str(),
        }
    }

    /// Phase 4 ~ 6 agent 식별자.
    ///
    /// - `Agent` → 호스트-부여 `agent_id` (verifiable, session token 검증 통과)
    /// - `Plugin` → manifest 의 `plugin_id`
    /// - `Local` → env `TASTY_AGENT_ID` (없으면 `_host`)
    pub fn agent_id(&self) -> tasty_telemetry::AgentId {
        match self {
            CallerContext::Local => tasty_telemetry::AgentId::from_env(),
            CallerContext::Plugin { plugin_id, .. } => {
                tasty_telemetry::AgentId::new(plugin_id.clone())
            }
            CallerContext::Agent { agent_id, .. } => {
                tasty_telemetry::AgentId::new(agent_id.clone())
            }
        }
    }
}

fn check_permissions(
    caller_label: &str,
    permissions: &Arc<HashSet<Permission>>,
    method: &str,
) -> Result<(), CallerError> {
    let meta = method_meta(method).ok_or_else(|| CallerError::UnknownMethod(method.to_string()))?;
    if !meta.plugin_callable {
        return Err(CallerError::NotPluginCallable {
            caller_label: caller_label.to_string(),
            method: method.to_string(),
        });
    }
    for needed in meta.required {
        if !permissions.contains(needed) {
            return Err(CallerError::MissingPermission {
                caller_label: caller_label.to_string(),
                method: method.to_string(),
                permission: needed.clone(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plugin_with(perms: &[Permission]) -> CallerContext {
        CallerContext::Plugin {
            plugin_id: "com.example.test".into(),
            permissions: Arc::new(perms.iter().cloned().collect()),
        }
    }

    fn agent_with(perms: &[Permission]) -> CallerContext {
        CallerContext::Agent {
            agent_id: "child:1".into(),
            permissions: Arc::new(perms.iter().cloned().collect()),
        }
    }

    #[test]
    fn local_passes_all_known_methods() {
        let c = CallerContext::Local;
        assert!(c.ensure_allowed("surface.list").is_ok());
        assert!(c.ensure_allowed("debug.inject_key").is_ok());
        assert!(c.ensure_allowed("plugin.enable").is_ok());
    }

    #[test]
    fn local_passes_unknown_methods_too() {
        // Local은 method_meta 검사 없이 통과 — 알려지지 않은 메서드도 라우터의
        // method_not_found가 처리하도록 위임.
        assert!(
            CallerContext::Local
                .ensure_allowed("not.a.real.method")
                .is_ok()
        );
    }

    #[test]
    fn plugin_missing_permission_denied() {
        let c = plugin_with(&[Permission::SurfaceRead]);
        let err = c.ensure_allowed("tab.create").unwrap_err();
        assert!(matches!(
            err,
            CallerError::MissingPermission {
                permission: Permission::SurfaceWrite,
                ..
            }
        ));
    }

    #[test]
    fn plugin_with_permission_passes() {
        let c = plugin_with(&[Permission::SurfaceWrite]);
        assert!(c.ensure_allowed("tab.create").is_ok());
    }

    #[test]
    fn plugin_cannot_call_local_only_method() {
        let c = plugin_with(&[Permission::SurfaceWrite]);
        let err = c.ensure_allowed("plugin.enable").unwrap_err();
        assert!(matches!(err, CallerError::NotPluginCallable { .. }));
    }

    #[test]
    fn agent_with_permission_passes() {
        let c = agent_with(&[Permission::SurfaceWrite]);
        assert!(c.ensure_allowed("tab.create").is_ok());
    }

    #[test]
    fn agent_missing_permission_denied() {
        let c = agent_with(&[Permission::SurfaceRead]);
        let err = c.ensure_allowed("tab.create").unwrap_err();
        assert!(matches!(
            err,
            CallerError::MissingPermission {
                permission: Permission::SurfaceWrite,
                ..
            }
        ));
    }

    #[test]
    fn agent_cannot_call_local_only_method() {
        let c = agent_with(&[Permission::SurfaceWrite]);
        let err = c.ensure_allowed("plugin.enable").unwrap_err();
        assert!(matches!(err, CallerError::NotPluginCallable { .. }));
    }

    #[test]
    fn session_token_generate_is_64_hex() {
        let t = SessionToken::generate();
        assert_eq!(t.as_str().len(), 64);
        assert!(t.as_str().bytes().all(|b| b.is_ascii_hexdigit()));
    }

    #[test]
    fn session_token_from_str_validates_length_and_charset() {
        assert!(SessionToken::from_str("").is_none());
        assert!(SessionToken::from_str("too short").is_none());
        let valid = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        assert_eq!(SessionToken::from_str(valid).unwrap().as_str(), valid);
        let invalid = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdez";
        assert!(SessionToken::from_str(invalid).is_none());
    }

    #[test]
    fn session_token_from_str_normalizes_case() {
        let upper = "0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF";
        assert_eq!(
            SessionToken::from_str(upper).unwrap().as_str(),
            upper.to_ascii_lowercase()
        );
    }

    #[test]
    #[cfg(debug_assertions)]
    fn plugin_cannot_call_debug_method_even_with_all_perms() {
        let all: Vec<Permission> = [
            Permission::SurfaceRead,
            Permission::SurfaceWrite,
            Permission::TerminalRead,
            Permission::TerminalWrite,
            Permission::Notification,
        ]
        .into_iter()
        .collect();
        let c = plugin_with(&all);
        let err = c.ensure_allowed("debug.inject_key").unwrap_err();
        assert!(matches!(err, CallerError::NotPluginCallable { .. }));
    }

    /// release 빌드에서는 debug 메서드가 method_meta에 아예 등록되지 않는다.
    /// 따라서 plugin이 호출하면 `NotPluginCallable`이 아니라 `UnknownMethod`로
    /// 떨어진다 — 호출 거부라는 결과는 같지만 메시지가 다르다.
    #[test]
    #[cfg(not(debug_assertions))]
    fn plugin_call_to_debug_method_is_unknown_in_release() {
        let c = plugin_with(&[]);
        let err = c.ensure_allowed("debug.inject_key").unwrap_err();
        assert!(matches!(err, CallerError::UnknownMethod(_)));
    }

    #[test]
    fn plugin_unknown_method_errors() {
        let c = plugin_with(&[]);
        let err = c.ensure_allowed("not.a.real.method").unwrap_err();
        assert!(matches!(err, CallerError::UnknownMethod(_)));
    }

    #[test]
    fn plugin_ipc_invoke_permission_does_not_unlock_static_methods() {
        // ipc.invoke 권한은 namespace forward 경로에서만 의미가 있고,
        // 정적 method_meta 권한 검사를 우회하지 않는다.
        let c = plugin_with(&[Permission::IpcInvoke("codex".into())]);
        let err = c.ensure_allowed("tab.create").unwrap_err();
        assert!(matches!(
            err,
            CallerError::MissingPermission {
                permission: Permission::SurfaceWrite,
                ..
            }
        ));
    }
}

/// Phase 6.2c — envelope 의 `session_token` 필드를 보고 caller 를 결정한다.
///
/// - `session_token` 가 None → `CallerContext::Local`
/// - 형식이 잘못된 토큰(64-char hex 아님) → `Err(deny)`
/// - 유효 형식이지만 store 에 없음/만료/revoked → `Err(deny)` (Local fallback 금지)
/// - 유효 → `CallerContext::Agent { ... }`
///
/// memory store 가 초기화되지 않은 경우: 토큰이 있어도 검증 불가이므로
/// `Err(deny)` 로 막는다 — 부팅 초기의 가장된 호출을 막는다.
pub(crate) fn resolve_caller_from_envelope(
    core: &crate::core::Core,
    request: &crate::ipc::protocol::JsonRpcRequest,
) -> Result<CallerContext, crate::ipc::protocol::JsonRpcResponse> {
    use std::collections::HashSet;
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};

    let token_str = match request.session_token.as_deref() {
        None => return Ok(CallerContext::Local),
        Some(s) => s,
    };
    let id = request.id.clone().unwrap_or(serde_json::Value::Null);
    let deny = |msg: &str| {
        crate::ipc::protocol::JsonRpcResponse::error(
            id.clone(),
            -32001,
            &format!("permission_denied: {msg}"),
        )
    };
    let token = match SessionToken::from_str(token_str) {
        Some(t) => t,
        None => return Err(deny("invalid session_token format (expect 64 hex chars)")),
    };
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let resolved = core.with_memory(|mem| {
        let mut store = crate::ipc::session::SessionStore::new(mem, tasty_memory::HOST_OWNER);
        store.resolve(&token, now_ms)
    });
    let session = match resolved {
        Err(e) => return Err(deny(&format!("session lookup failed: {e}"))),
        Ok(None) => return Err(deny("session_token unknown/expired/revoked")),
        Ok(Some(s)) => s,
    };
    let perms: HashSet<crate::plugin::manifest::Permission> = session.permission_set();
    Ok(CallerContext::Agent {
        agent_id: session.agent_id,
        permissions: Arc::new(perms),
    })
}
