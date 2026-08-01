//! 호스트의 file format / handler 레지스트리를 plugin manager 가 의존 없이 받기 위한 trait.
//!
//! manager 는 plugin enable/disable 시점에 plugin 의 detector/handler contribute 를
//! 등록/해제한다. 본 trait 는 그 두 동작만 노출한다. 페이로드는 opaque
//! `serde_json::Value` 로 받아 호스트 측에서 concrete 타입으로 deserialize 한다.

pub trait FileFormatRegistryPort: Send + Sync {
    fn install_plugin_detectors(&self, plugin_id: &str, detectors: &[serde_json::Value]);
    fn uninstall_plugin(&self, plugin_id: &str);
}

pub trait FileHandlerRegistryPort: Send + Sync {
    fn install_plugin_handlers(&self, plugin_id: &str, handlers: &[serde_json::Value]);
    fn uninstall_plugin(&self, plugin_id: &str);
}

/// 공유 훅 핸들러 레지스트리(webhook/hook 트리거 공유)를 plugin manager 가 의존 없이
/// 받기 위한 trait. 파일 핸들러와 동일 형태 — plugin enable/disable 시점에
/// `[[contributes.hook_handler]]` opaque payload 를 등록/해제한다. 호스트 impl 이
/// concrete `HookHandlerDecl<PluginHookHandlerActionDecl>` 로 deserialize 한다.
pub trait HookHandlerRegistryPort: Send + Sync {
    fn install_plugin_hook_handlers(&self, plugin_id: &str, handlers: &[serde_json::Value]);
    fn uninstall_plugin(&self, plugin_id: &str);
}

/// 완료 판정 전략 레지스트리(TODO80 §B)를 plugin manager 가 의존 없이 받기 위한
/// trait. 훅 핸들러와 동일 형태 — plugin enable/disable 시점에
/// `[[contributes.completion_strategy]]` opaque payload 를 등록/해제한다. 호스트
/// impl 이 concrete `CompletionStrategyDecl` 로 deserialize 한다.
pub trait CompletionStrategyRegistryPort: Send + Sync {
    fn install_plugin_completion_strategies(
        &self,
        plugin_id: &str,
        strategies: &[serde_json::Value],
    );
    fn uninstall_plugin(&self, plugin_id: &str);
}
