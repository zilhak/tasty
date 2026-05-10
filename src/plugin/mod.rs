//! Plugin 시스템 — 외부 plugin 프로세스의 매니페스트 파싱·디스커버리·생명주기 관리.
//!
//! 단계 05A: 매니페스트 + 디스커버리 + enabled/disabled 영속화. 프로세스 spawn은 05B.

pub mod manifest;
pub mod discovery;
pub mod registry_state;

pub use manifest::{Manifest, PluginPackage, HOST_API_VERSION};
pub use discovery::{discover, plugin_root};
pub use registry_state::PluginsConfig;
