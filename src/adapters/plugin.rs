//! Plugin 시스템 — tasty-host-plugin crate 의 thin re-export.
//!
//! Phase F.B.11-2 ~ B.11-4 후, manager / process / listener / protocol /
//! discovery / builtin / event_bus / ui_tree / registry_state / extension_registry
//! / tool_registry / ipc_namespace / command_registry / handle_channel 가 모두
//! `tasty-host-plugin` 으로 이동. 본 모듈은 기존 호출처 `crate::plugin::*` 의
//! 하위 호환을 위해 thin re-export 만 유지한다.
//!
//! `manifest` 는 `tasty-plugin-manifest` 가 owning (F.B.6).
#![allow(unused_imports)]

pub mod manifest;

pub use tasty_host_plugin::{
    builtin, command_registry, discovery, event_bus, extension_registry, handle_channel,
    ipc_namespace, listener, manager, process, protocol, registry_state, tool_registry, ui_tree,
};

pub use manifest::{HOST_API_VERSION, Manifest};
pub use tasty_host_plugin::protocol::{AuthMessage, PluginEvent, PluginRequest, PluginResponse};
pub use tasty_host_plugin::{
    HostListener, PluginManager, PluginPackage, PluginProcess, PluginsConfig, bundle_root,
    discover, install_builtins_if_needed, is_builtin_plugin, mark_builtin_removed, plugin_root,
};
