//! Plugin manager 가 본 바이너리 도메인 (engine / file / shortcuts / model 등)
//! 과 결합한 코드를 모아 두는 bin-side glue.
//!
//! tasty-host-plugin (manager crate) 가 본 바이너리를 역참조할 수 없으므로,
//! 본 모듈이 *protocol port impl* 의 본 바이너리 잔존 지점 역할을 한다.

pub mod host_actions;
pub mod host_cmd;
#[cfg(feature = "gui")]
pub mod key_dispatch;
pub mod manifest_validate;
#[cfg(feature = "gui")]
pub mod popup_render;
pub mod remote_kind;
pub mod remote_surface;
#[cfg(feature = "gui")]
pub mod ui_tree_render;
