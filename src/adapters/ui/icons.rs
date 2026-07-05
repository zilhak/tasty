//! ui_kit line-icon set — canonical 소스는 [`tasty_icons`] 크레이트.
//!
//! 이 모듈은 크레이트 아이콘을 재노출하고 host 로컬 이름 별칭만 유지한다(중복 path
//! 정의 제거). egui_extras 의 svg 로더(`gpu.rs` 의 `install_image_loaders`)가
//! 텍스처화하고, `Icon::image` 의 `tint` 로 테마 색을 입힌다.

pub use tasty_icons::*;

// host 로컬 이름 → canonical 별칭 (기존 사용처 무변경 목적).
pub use tasty_icons::{
    COPY as CLIPBOARD, LAYOUT_DETAIL as DETAIL, LAYOUT_GRID as GRID, LIST as LIST_VIEW,
    MARKDOWN as MD, REMOTE as TERMINAL_PROMPT, TERMINAL as TERM,
};
