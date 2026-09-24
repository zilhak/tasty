//! 플러그인 관리자가 파일 형식·핸들러를 등록하고 해제할 때 사용하는 trait.
//! 호스트 구현이 JSON 값을 실제 선언 타입으로 변환한다.

pub trait FileFormatRegistryPort: Send + Sync {
    fn install_plugin_detectors(&self, plugin_id: &str, detectors: &[serde_json::Value]);
    fn uninstall_plugin(&self, plugin_id: &str);
}

pub trait FileHandlerRegistryPort: Send + Sync {
    fn install_plugin_handlers(&self, plugin_id: &str, handlers: &[serde_json::Value]);
    fn uninstall_plugin(&self, plugin_id: &str);
}

/// 플러그인의 훅 핸들러를 호스트 레지스트리에 등록하거나 해제한다.
pub trait HookHandlerRegistryPort: Send + Sync {
    fn install_plugin_hook_handlers(&self, plugin_id: &str, handlers: &[serde_json::Value]);
    fn uninstall_plugin(&self, plugin_id: &str);
}

/// 플러그인의 완료 판정 전략을 호스트 레지스트리에 등록하거나 해제한다.
pub trait CompletionStrategyRegistryPort: Send + Sync {
    fn install_plugin_completion_strategies(
        &self,
        plugin_id: &str,
        strategies: &[serde_json::Value],
    );
    fn uninstall_plugin(&self, plugin_id: &str);
}
