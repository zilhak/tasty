//! 입력 처리 — 저수준 (raw mouse / modifier 검출) + 고수준 (shortcuts).

pub mod click_cursor;
pub mod double_tap;
#[cfg(target_os = "linux")]
pub(crate) mod explorer_ime;
pub mod shortcuts;
pub mod synthetic;
