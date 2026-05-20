//! 설정 윈도우 탭별 draw 함수 모음. 각 sub-module 은 한 탭의 `pub fn draw_*_tab`
//! + 그에 종속된 helper 만 보관한다.

mod accessibility;
mod appearance;
mod clipboard;
mod general;
mod language;
mod misc;
mod notifications;
mod performance;
mod terminal;

pub use accessibility::draw_accessibility_tab;
pub use appearance::draw_appearance_tab;
pub use clipboard::draw_clipboard_tab;
pub use general::draw_general_tab;
pub use language::draw_language_tab;
pub use misc::draw_misc_tab;
pub use notifications::draw_notifications_tab;
pub use performance::draw_performance_tab;
pub use terminal::draw_terminal_tab;
