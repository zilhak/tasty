//! Plugin 매니저와 `App` 사이의 glue 코드.
//!
//! 핫 패스(IPC dispatch) 가 아닌, 모달/단축키/스냅샷 같은 보조 경로.

pub(crate) mod lifecycle;
pub(crate) mod palette_commands;
pub(crate) mod shortcut;
pub(crate) mod snapshot;
pub(crate) mod tool_registry;
pub(crate) mod window_actions;
