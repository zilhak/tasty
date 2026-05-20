//! 사이드바 UI — collapsed / full 두 모드를 sub-module 로 분리.

mod collapsed;
mod full;

pub use collapsed::draw_collapsed_sidebar;
pub use full::draw_full_sidebar;