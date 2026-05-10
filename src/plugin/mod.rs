//! Plugin 시스템 — 외부 plugin 프로세스의 매니페스트 파싱·디스커버리·생명주기 관리.

pub mod discovery;
pub mod listener;
pub mod manifest;
pub mod process;
pub mod protocol;
pub mod registry_state;

pub use discovery::{discover, plugin_root};
pub use listener::HostListener;
pub use manifest::{Manifest, PluginPackage, HOST_API_VERSION};
pub use process::PluginProcess;
pub use protocol::{AuthMessage, PluginEvent, PluginRequest, PluginResponse};
pub use registry_state::PluginsConfig;
