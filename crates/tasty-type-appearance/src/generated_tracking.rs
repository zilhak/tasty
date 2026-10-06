//! Generated from `dtcg/tasty.tokens.json` — DO NOT EDIT.
//! 재생성: `cargo run -p tasty-design-tokens --bin generate`.
//!
//! em 단위 자간 토큰을 Theme를 통해 읽는다. 접근자는 글자 크기를 받아
//! `em × 글자 크기`를 egui `extra_letter_spacing` 값으로 돌려준다.
//! 글자 크기가 이미 UI 배율을 반영하므로 배율을 다시 곱하지 않는다.

use tasty_type_geometry::length::LogicalPx;

impl crate::theme::Theme {
    /// `semantic.letter-spacing-caps` → `{primitive.letter-spacing-04}` = 0.04em
    #[inline]
    pub fn letter_spacing_caps(&self, font_size: LogicalPx) -> LogicalPx {
        LogicalPx(0.04 * font_size.value())
    }

    /// `component.sidebar-section-heading-tracking` → `{semantic.letter-spacing-caps}` = 0.04em
    #[inline]
    pub fn sidebar_section_heading_tracking(&self, font_size: LogicalPx) -> LogicalPx {
        self.letter_spacing_caps(font_size)
    }

    /// `component.table-header-tracking` → `{semantic.letter-spacing-caps}` = 0.04em
    #[inline]
    pub fn table_header_tracking(&self, font_size: LogicalPx) -> LogicalPx {
        self.letter_spacing_caps(font_size)
    }
}
