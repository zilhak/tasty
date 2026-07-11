//! 공유 훅 핸들러 레지스트리 — **MVP: 인메모리 최소**.
//!
//! 파일 핸들러(`src/file/handler/registry.rs`)의 3출처 병합·patch semantics·user
//! config 영속화는 후속 stage(S1b)에서 정식화한다. MVP 는 등록/조회/해제만 갖춘
//! 단순 맵 + 프로세스 전역 싱글턴이다. 웹훅 리스너(off-main thread)와 IPC 핸들러
//! (main thread)가 같은 레지스트리를 봐야 하므로 `OnceLock<Mutex<..>>` 싱글턴으로
//! 공유한다(`method_meta::PLUGIN_PREFIXES` 선례).

use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

use super::types::{HookHandler, HookHandlerAction, HookHandlerId, HookSource};

/// 레지스트리 등록 실패 사유.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryError {
    /// 같은 id 가 이미 등록됨.
    Duplicate { id: String },
    /// 셸 action 은 `source: Hook` 만 허용(불변식 강제).
    ShellMustBeHookSource { id: String },
}

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Duplicate { id } => write!(f, "hook handler '{id}' already registered"),
            Self::ShellMustBeHookSource { id } => write!(
                f,
                "hook handler '{id}' is a shell command and must declare source=hook"
            ),
        }
    }
}

/// 인메모리 훅 핸들러 레지스트리 (MVP).
#[derive(Debug, Default)]
pub struct HookHandlerRegistry {
    handlers: BTreeMap<HookHandlerId, HookHandler>,
}

impl HookHandlerRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 핸들러 등록. 중복 id 는 거부하고, 셸 action 은 `source=Hook` 을 강제한다
    /// (불변식: 셸 웹훅 거부의 등록단계 방어선).
    pub fn register(&mut self, handler: HookHandler) -> Result<(), RegistryError> {
        if matches!(handler.action, HookHandlerAction::ShellCommand { .. })
            && handler.source != HookSource::Hook
        {
            return Err(RegistryError::ShellMustBeHookSource {
                id: handler.id.0.clone(),
            });
        }
        if self.handlers.contains_key(&handler.id) {
            return Err(RegistryError::Duplicate {
                id: handler.id.0.clone(),
            });
        }
        self.handlers.insert(handler.id.clone(), handler);
        Ok(())
    }

    /// 이미 존재하면 덮어쓰는 등록(익명 핸들러 재등록 등). 셸 불변식은 동일 적용.
    pub fn upsert(&mut self, handler: HookHandler) -> Result<(), RegistryError> {
        if matches!(handler.action, HookHandlerAction::ShellCommand { .. })
            && handler.source != HookSource::Hook
        {
            return Err(RegistryError::ShellMustBeHookSource {
                id: handler.id.0.clone(),
            });
        }
        self.handlers.insert(handler.id.clone(), handler);
        Ok(())
    }

    pub fn get(&self, id: &HookHandlerId) -> Option<&HookHandler> {
        self.handlers.get(id)
    }

    pub fn contains(&self, id: &HookHandlerId) -> bool {
        self.handlers.contains_key(id)
    }

    /// 전체 핸들러 (id 정렬순). 포커스 독립 — 전 범위 조회.
    pub fn all(&self) -> Vec<HookHandler> {
        self.handlers.values().cloned().collect()
    }

    pub fn remove(&mut self, id: &HookHandlerId) -> Option<HookHandler> {
        self.handlers.remove(id)
    }

    pub fn len(&self) -> usize {
        self.handlers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.handlers.is_empty()
    }
}

/// 프로세스 전역 싱글턴. 웹훅 리스너 thread 와 IPC 핸들러 thread 가 공유.
static REGISTRY: OnceLock<Mutex<HookHandlerRegistry>> = OnceLock::new();

/// 전역 훅 핸들러 레지스트리 핸들. lock 이 poison 되면(다른 thread panic) 복구해
/// 반환한다 — 레지스트리는 단순 맵이라 부분갱신 위험이 없다.
pub fn global() -> std::sync::MutexGuard<'static, HookHandlerRegistry> {
    REGISTRY
        .get_or_init(|| Mutex::new(HookHandlerRegistry::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hook_handler::types::{
        HookHandlerOwner, IpcCall, TriggerSource, validate_binding,
    };

    fn ipc_handler(id: &str, source: HookSource) -> HookHandler {
        HookHandler {
            id: HookHandlerId::new(id),
            source,
            priority: 0,
            owner: HookHandlerOwner::Host,
            action: HookHandlerAction::IpcSequence {
                calls: vec![IpcCall {
                    method: "notification.create".to_string(),
                    params: serde_json::json!({"body": "${body.message}"}),
                }],
            },
            display_name_i18n_key: None,
            disabled: false,
        }
    }

    fn shell_handler(id: &str, source: HookSource) -> HookHandler {
        HookHandler {
            id: HookHandlerId::new(id),
            source,
            priority: 0,
            owner: HookHandlerOwner::Host,
            action: HookHandlerAction::ShellCommand {
                command: "echo".to_string(),
                args: vec!["hi".to_string()],
            },
            display_name_i18n_key: None,
            disabled: false,
        }
    }

    #[test]
    fn register_and_get() {
        let mut reg = HookHandlerRegistry::new();
        reg.register(ipc_handler("host/notify", HookSource::Webhook))
            .unwrap();
        assert!(reg.contains(&HookHandlerId::new("host/notify")));
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn duplicate_rejected() {
        let mut reg = HookHandlerRegistry::new();
        reg.register(ipc_handler("host/notify", HookSource::Webhook))
            .unwrap();
        let err = reg
            .register(ipc_handler("host/notify", HookSource::Webhook))
            .unwrap_err();
        assert!(matches!(err, RegistryError::Duplicate { .. }));
    }

    #[test]
    fn shell_with_webhook_source_rejected_at_registration() {
        let mut reg = HookHandlerRegistry::new();
        // 불변식: 셸 action 은 source=hook 만 허용.
        let err = reg
            .register(shell_handler("host/sh", HookSource::Webhook))
            .unwrap_err();
        assert!(matches!(err, RegistryError::ShellMustBeHookSource { .. }));
        let err = reg
            .register(shell_handler("host/sh2", HookSource::Any))
            .unwrap_err();
        assert!(matches!(err, RegistryError::ShellMustBeHookSource { .. }));
        // source=hook 셸은 허용.
        reg.register(shell_handler("host/sh3", HookSource::Hook))
            .unwrap();
    }

    #[test]
    fn shell_hook_cannot_bind_to_webhook() {
        // 등록은 되지만(source=hook), 웹훅 바인딩 게이트에서 거부.
        let h = shell_handler("host/sh", HookSource::Hook);
        let err = validate_binding(&h, TriggerSource::Webhook).unwrap_err();
        assert!(matches!(
            err,
            crate::hook_handler::types::BindingError::SourceMismatch { .. }
        ));
    }

    #[test]
    fn webhook_ipc_handler_binds_to_webhook_not_hook() {
        let h = ipc_handler("host/notify", HookSource::Webhook);
        assert!(validate_binding(&h, TriggerSource::Webhook).is_ok());
        assert!(validate_binding(&h, TriggerSource::Hook).is_err());
    }

    #[test]
    fn any_source_binds_both() {
        let h = ipc_handler("host/notify", HookSource::Any);
        assert!(validate_binding(&h, TriggerSource::Webhook).is_ok());
        assert!(validate_binding(&h, TriggerSource::Hook).is_ok());
    }
}
