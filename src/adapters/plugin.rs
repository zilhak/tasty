//! Plugin 시스템 — tasty-host-plugin crate 의 thin re-export.
//!
//! manager / process / listener / protocol / discovery / builtin / event_bus /
//! registry_state / extension_registry / tool_registry / ipc_namespace /
//! command_registry / handle_channel 은 모두 `tasty-host-plugin` 으로 이동했다.
//! 본 모듈은 기존 호출처 `crate::plugin::*` 의 하위 호환을 위해 thin re-export 만
//! 유지한다.
//!
//! `manifest` 는 `tasty-plugin-manifest` 가 owning.
#![allow(unused_imports)]

pub use tasty_host_plugin::{
    builtin, command_registry, discovery, event_bus, extension_registry, handle_channel, listener,
    manager, process, protocol, registry_state, tool_registry,
};
// namespace 소유 표는 `tasty-ipc` 가 든다 — 해소(`method_meta`)가 그 crate 에 있고
// 표가 하나뿐이기 때문이다. 종전 경로(`crate::plugin::ipc_namespace`)의 호환만 남긴다.
pub use tasty_ipc::ipc_namespace;
pub use tasty_plugin_manifest as manifest;
pub use tasty_plugin_manifest::{HOST_API_VERSION, Manifest};

pub use tasty_host_plugin::protocol::{AuthMessage, PluginEvent, PluginRequest, PluginResponse};
pub use tasty_host_plugin::{
    BuiltinUpgradeAction, BuiltinUpgradeItem, BuiltinUpgradeReport, HostListener, PluginManager,
    PluginPackage, PluginProcess, PluginsConfig, bundle_root, discover, install_builtins_if_needed,
    is_builtin_plugin, mark_builtin_removed, plugin_root, upgrade_builtins,
};
