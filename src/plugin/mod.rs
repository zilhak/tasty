//! Plugin 시스템 — 외부 plugin 프로세스의 매니페스트 파싱·디스커버리·생명주기 관리.

#![allow(unused_imports)]

pub mod builtin;
pub mod command_registry;
pub mod discovery;
pub mod host_actions;
pub mod host_cmd;
pub mod ipc_namespace;
pub mod key_dispatch;
pub mod listener;
pub mod manager;
pub mod manifest;
pub mod process;
pub mod protocol;
pub mod registry_state;
pub mod remote_kind;
pub mod remote_surface;
pub mod ui_tree;
pub mod ui_tree_render;

pub use builtin::{
    bundle_root, install_builtins_if_needed, is_builtin_plugin, mark_builtin_removed,
    BUILTIN_PLUGIN_IDS,
};
pub use discovery::{discover, plugin_root};
pub use listener::HostListener;
pub use manager::PluginManager;
pub use manifest::{Manifest, PluginPackage, HOST_API_VERSION};
pub use process::PluginProcess;
pub use protocol::{AuthMessage, PluginEvent, PluginRequest, PluginResponse};
pub use registry_state::PluginsConfig;
