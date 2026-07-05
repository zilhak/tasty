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

// ── divergent holdout (디자인 SoT 통일 커밋에서 제거) ──
// STAR/IMAGE 는 디자인 번들 SoT 와 path 가 달라, 지금은 host-legacy 렌더를 그대로
// 유지한다(순수 dedup — 이 커밋에서 렌더 무변경). 다음 커밋에서 이 로컬 정의를 지워
// 크레이트(디자인 SoT)로 통일하며, 그때 STAR/IMAGE 렌더가 바뀐다.
macro_rules! legacy_line_icon {
    ($name:ident, $uri:literal, $body:literal) => {
        pub const $name: Icon = Icon {
            svg: concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">"#,
                $body,
                "</svg>"
            ),
            body: $body,
            uri: concat!("bytes://tasty_icon_", $uri, ".svg"),
            filled: false,
        };
    };
}
legacy_line_icon!(
    STAR,
    "star",
    r#"<path d="M12 2l3.09 6.26L22 9.27l-5 4.87 1.18 6.88L12 17.77l-6.18 3.25L7 14.14l-5-4.87 6.91-1.01L12 2z"/>"#
);
legacy_line_icon!(
    IMAGE,
    "image",
    r#"<rect x="3" y="3" width="18" height="18" rx="2"/><circle cx="9" cy="9" r="2"/><path d="m21 15-3.1-3.1a2 2 0 0 0-2.8 0L6 21"/>"#
);
