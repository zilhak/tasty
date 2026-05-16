//! IPC 호출자 컨텍스트 — local CLI/사용자 vs plugin process 구분.
//!
//! `route_engine_handler`/`route_gui_handler` 진입점에서 `CallerContext::ensure_allowed`로
//! 권한을 검사한 뒤 분기한다. Local은 모든 메서드 통과, Plugin은 매니페스트에
//! 선언된 권한과 [`crate::ipc::method_meta`] 테이블을 대조한다.

use std::collections::HashSet;
use std::sync::Arc;

use crate::ipc::method_meta::method_meta;
use crate::plugin::manifest::Permission;

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
}

#[derive(Debug)]
pub enum CallerError {
    UnknownMethod(String),
    NotPluginCallable { plugin_id: String, method: String },
    MissingPermission {
        plugin_id: String,
        method: String,
        permission: Permission,
    },
}

impl std::fmt::Display for CallerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CallerError::UnknownMethod(m) => write!(f, "unknown ipc method: {m}"),
            CallerError::NotPluginCallable { plugin_id, method } => {
                write!(f, "method '{method}' is not callable from plugin '{plugin_id}'")
            }
            CallerError::MissingPermission {
                plugin_id,
                method,
                permission,
            } => write!(
                f,
                "plugin '{plugin_id}' missing permission '{}' for method '{method}'",
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
            } => {
                let meta = method_meta(method)
                    .ok_or_else(|| CallerError::UnknownMethod(method.to_string()))?;
                if !meta.plugin_callable {
                    return Err(CallerError::NotPluginCallable {
                        plugin_id: plugin_id.clone(),
                        method: method.to_string(),
                    });
                }
                for needed in meta.required {
                    if !permissions.contains(needed) {
                        return Err(CallerError::MissingPermission {
                            plugin_id: plugin_id.clone(),
                            method: method.to_string(),
                            permission: needed.clone(),
                        });
                    }
                }
                Ok(())
            }
        }
    }

    #[allow(dead_code)]
    pub fn is_plugin(&self) -> bool {
        matches!(self, CallerContext::Plugin { .. })
    }

    /// memory.db `owner` 값 도출. Local 은 `_host` sentinel, plugin 은 자신의 id.
    /// [`tasty_memory::HOST_OWNER`] 와 동기화돼야 한다.
    pub fn owner(&self) -> &str {
        match self {
            CallerContext::Local => tasty_memory::HOST_OWNER,
            CallerContext::Plugin { plugin_id, .. } => plugin_id.as_str(),
        }
    }
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
        assert!(CallerContext::Local
            .ensure_allowed("not.a.real.method")
            .is_ok());
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
