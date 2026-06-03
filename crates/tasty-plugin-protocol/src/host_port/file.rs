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
