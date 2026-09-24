//! 기존 crate::plugin 경로로 tasty-host-plugin을 재노출한다.
//! 매니페스트 타입은 tasty-plugin-manifest에서 가져온다.
#![allow(unused_imports)]

pub use tasty_host_plugin::{
    builtin, command_registry, discovery, event_bus, extension_registry, handle_channel, listener,
    manager, process, protocol, registry_state, tool_registry,
};
// namespace 등록표는 메서드 분류와 함께 tasty-ipc가 소유한다.
pub use tasty_ipc::ipc_namespace;
pub use tasty_plugin_manifest as manifest;
pub use tasty_plugin_manifest::{HOST_API_VERSION, Manifest};

pub use tasty_host_plugin::protocol::{AuthMessage, PluginEvent, PluginRequest, PluginResponse};
pub use tasty_host_plugin::{
    BuiltinUpgradeAction, BuiltinUpgradeItem, BuiltinUpgradeReport, HostListener, PluginManager,
    PluginPackage, PluginProcess, PluginsConfig, bundle_root, discover, install_builtins_if_needed,
    is_builtin_plugin, mark_builtin_removed, plugin_root, upgrade_builtins,
};
