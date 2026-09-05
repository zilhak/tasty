//! Tasty plugin manager — 호스트 측 lifecycle/IPC routing/manifest registry.
//!
//! 본 crate 가 manager / handle_channel / process / listener / protocol /
//! discovery / builtin / event_bus 를 들고 있다 — 본 바이너리에 흩어져 있던 것을
//! 흡수해 온 것이고, 그쪽에는 더 이상 남아 있지 않다. host 본 바이너리 결합은 6 개 host_port trait
//! (SurfaceRegistry / FileFormatRegistryPort / FileHandlerRegistryPort /
//! I18nNamespaceRegistrar / IpcHostFacade) + plugin_bridge/ 잔존 5 모듈로 격리.

// 이유: 테스트 본문의 `let _ =` 는 정책이 사유를 요구하지 않는 자리라
// `clippy::let_underscore_must_use` 명부에 섞이면 안 된다 — 그 명부는 프로덕션에서
// 값을 버리는 자리의 목록이고, 테스트가 늘 때마다 숫자만 흔들리면 새 프로덕션
// 자리가 그 안에 묻힌다(docs/dev-guide/error-handling.md). `cfg_attr(test, ..)` 라
// 라이브러리 타깃의 판정은 그대로다 — 프로덕션 자리는 여전히 명부에 오른다.
#![cfg_attr(test, allow(clippy::let_underscore_must_use))]

pub mod builtin;
pub mod bundle_sig;
pub mod command_registry;
pub mod discovery;
pub mod event_bus;
pub mod extension_registry;
pub mod handle_channel;
pub mod host_actions;
pub mod host_cmd;
pub mod known_plugins;
pub mod listener;
pub mod manager;
pub mod process;
pub mod protocol;
pub mod reaper;
pub mod registry_state;
pub mod settings_registry;
#[cfg(test)]
mod test_support;
pub mod tool_registry;
// Phase J.C WASM POC stub — `wasm-poc` feature 가 활성일 때만 컴파일.
// default 빌드 surface 변경 0.
pub mod wasm_poc;

// 테스트는 event_bus.rs 에서 event_bus_tests.rs 를 로드 (co-located).

pub use builtin::{
    BuiltinUpgradeAction, BuiltinUpgradeItem, BuiltinUpgradeReport, bundle_root,
    install_builtins_if_needed, is_builtin_plugin, mark_builtin_removed, upgrade_builtins,
};
pub use discovery::{discover, plugin_root};
pub use listener::HostListener;
pub use manager::{EguiMeshFrame, PluginManager, PopupInstance, next_popup_z_seq};
pub use process::PluginProcess;
pub use registry_state::PluginsConfig;
pub use settings_registry::{SettingsPageEntry, SettingsPageRegistry};
pub use tasty_plugin_manifest::PluginPackage;
