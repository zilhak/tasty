//! Plugin manager 가 본 바이너리 도메인 (engine / file / shortcuts / model 등)
//! 과 결합한 코드를 모아 두는 bin-side glue.
//!
//! tasty-host-plugin (manager crate) 가 본 바이너리를 역참조할 수 없으므로,
//! 본 모듈이 *protocol port impl* 의 본 바이너리 잔존 지점 역할을 한다.

pub mod egui_mesh_surface;
#[cfg(feature = "gui")]
pub mod key_dispatch;
#[cfg(feature = "gui")]
pub mod manifest_validate;
#[cfg(feature = "gui")]
pub mod popup_render;
#[cfg(feature = "gui")]
pub mod remote_kind;
pub mod remote_surface;
#[cfg(feature = "gui")]
pub mod ui_tree_render;

// host_cmd / host_actions 는 tasty-host-plugin crate 가 owning (manager 가 채널
// 송신자). 본 바이너리에서는 그대로 같은 경로로 노출하기 위해 re-export.
// host_actions 는 gui-only (keybindings_tab/plugins), host_cmd 는 headless 도
// 사용 (remote_surface).
#[cfg(feature = "gui")]
pub use tasty_host_plugin::host_actions;
pub use tasty_host_plugin::host_cmd;
