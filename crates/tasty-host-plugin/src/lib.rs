//! Tasty plugin manager — 호스트 측 lifecycle/IPC routing/manifest registry.
//!
//! 본 crate 는 본 바이너리 `src/adapters/plugin/` 의 manager / handle_channel /
//! process / listener / protocol / discovery / builtin / event_bus 등 다수
//! 모듈을 흡수한다. host 본 바이너리 결합은 Phase F.B.0 의 6 host_port trait
//! (SurfaceRegistry / FileFormatRegistryPort / FileHandlerRegistryPort /
//! I18nNamespaceRegistrar / IpcHostFacade) + plugin_bridge/ 잔존 5 모듈로 격리.
#![allow(dead_code)]

pub mod builtin;
pub mod bundle_sig;
pub mod command_registry;
pub mod discovery;
pub mod event_bus;
pub mod extension_registry;
pub mod handle_channel;
pub mod host_actions;
pub mod host_cmd;
pub mod ipc_namespace;
pub mod listener;
pub mod manager;
pub mod process;
pub mod protocol;
pub mod registry_state;
pub mod tool_registry;
pub mod ui_tree;

// 테스트는 event_bus.rs 에서 event_bus_tests.rs 를 로드 (co-located).

pub use builtin::{
    BuiltinUpgradeAction, BuiltinUpgradeItem, BuiltinUpgradeReport, bundle_root,
    install_builtins_if_needed, is_builtin_plugin, mark_builtin_removed, upgrade_builtins,
};
pub use discovery::{discover, plugin_root};
pub use listener::HostListener;
pub use manager::{PluginManager, PopupInstance};
pub use process::PluginProcess;
pub use registry_state::PluginsConfig;
pub use tasty_plugin_manifest::PluginPackage;
