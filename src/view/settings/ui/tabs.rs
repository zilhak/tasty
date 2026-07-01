//! 설정 윈도우 탭별 draw 함수 모음. 각 sub-module 은 한 탭의 `pub fn draw_*_tab`
//! + 그에 종속된 helper 만 보관한다.

mod accessibility;
mod appearance;
mod general;
mod misc;
mod notifications;
mod performance;
mod plugin;
mod terminal;

pub use accessibility::draw_accessibility_tab;
pub use appearance::draw_appearance_tab;
pub use general::draw_general_tab;
#[cfg(windows)]
pub use misc::draw_tastyrc_subtab;
pub use misc::{ScriptsUiState, draw_scripts_subtab};
pub use notifications::draw_notifications_tab;
pub use performance::draw_performance_tab;
pub use plugin::draw_plugin_tab;
pub use terminal::draw_terminal_tab;
