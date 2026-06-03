//! Plugin 시스템 — 외부 plugin 프로세스의 매니페스트 파싱·디스커버리·생명주기 관리.
//!
//! Plugin host API (lifecycle/registry/protocol) 는 gui 의 dispatcher/popup
//! 경유로 활성화. headless 빌드에선 호출자가 cfg(gui) 차단되어 미사용 경고.
//! library API surface 이므로 *headless 한정* dead_code 침묵.
#![allow(unused_imports)]
#![cfg_attr(not(feature = "gui"), allow(dead_code))]

pub mod builtin;
pub mod command_registry;
pub mod discovery;
pub mod event_bus;
pub mod extension_registry;
pub mod handle_channel;
pub mod host_actions;
pub mod host_cmd;
// (moved to surface_registry/host_rendered)
pub mod ipc_namespace;
pub mod listener;
pub mod manager;
pub mod manifest;
pub mod process;
pub mod protocol;
pub mod registry_state;
pub mod tool_registry;
pub mod ui_tree;

pub use builtin::{
    bundle_root, install_builtins_if_needed, is_builtin_plugin, mark_builtin_removed,
};
pub use discovery::{discover, plugin_root};
pub use listener::HostListener;
pub use manager::PluginManager;
pub use manifest::{HOST_API_VERSION, Manifest, PluginPackage};
pub use process::PluginProcess;
pub use protocol::{AuthMessage, PluginEvent, PluginRequest, PluginResponse};
pub use registry_state::PluginsConfig;
