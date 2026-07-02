//! `SIZING`(`tasty-type-appearance`) ↔ 생성된 토큰 const 정합 가드.
//!
//! 대응표 전거: 디자인 changelog 2026-07-02 token-coverage 의 Request 2 표.
//! 여기서 어긋나면 소스 치수와 디자인 토큰이 드리프트한 것 — 값을 임의로 맞추지
//! 말고 디자인 판정을 먼저 확인할 것.

use tasty_design_tokens::generated::{component, semantic};
use tasty_type_appearance::theme::SIZING;

/// f32 정합 — 토큰 값은 전부 유한 소수라 정확 일치를 요구한다.
macro_rules! assert_px {
    ($sizing:expr, $token:expr) => {
        assert_eq!(
            $sizing.0,
            $token.0,
            "SIZING.{} ({}) != {} ({})",
            stringify!($sizing).trim_start_matches("SIZING."),
            $sizing.0,
            stringify!($token),
            $token.0,
        );
    };
}

/// semantic 치수 ↔ SIZING (테마 불변 치수/타이포).
#[test]
fn sizing_matches_semantic_tokens() {
    // spacing (4px 그리드 5단)
    assert_px!(SIZING.spacing_xs, semantic::SPACE_XS);
    assert_px!(SIZING.spacing_sm, semantic::SPACE_SM);
    assert_px!(SIZING.spacing_md, semantic::SPACE_MD);
    assert_px!(SIZING.spacing_lg, semantic::SPACE_LG);
    assert_px!(SIZING.spacing_xl, semantic::SPACE_XL);

    // 보더/라운드
    assert_px!(SIZING.border_width, semantic::BORDER_WIDTH);
    assert_px!(SIZING.focus_ring_width, semantic::FOCUS_RING_WIDTH);
    assert_px!(SIZING.corner_radius, semantic::RADIUS);
    assert_px!(SIZING.corner_radius_sm, semantic::RADIUS_SM);
    // corner_radius_lg(8) 은 semantic 대응 토큰 없음 (primitive.radius-8 만) — 제외.

    // 컨트롤 높이 / 탭
    assert_px!(SIZING.item_height_tree, semantic::CONTROL_HEIGHT_TREE);
    assert_px!(SIZING.item_height_interactive, semantic::CONTROL_HEIGHT);
    assert_px!(SIZING.item_height_tab, semantic::CONTROL_HEIGHT_TAB);
    assert_px!(SIZING.tab_width, semantic::TAB_WIDTH);

    // 타이포 (UI 스케일 10/11/13/14 + prose/터미널)
    assert_px!(SIZING.font_size_micro, semantic::FONT_SIZE_MICRO);
    assert_px!(SIZING.font_size_caption, semantic::FONT_SIZE_CAPTION);
    assert_px!(SIZING.font_size_body, semantic::FONT_SIZE_BODY);
    assert_px!(SIZING.font_size_heading, semantic::FONT_SIZE_HEADING);
    assert_px!(SIZING.font_size_max, semantic::FONT_SIZE_MAX);
    assert_px!(SIZING.font_size_prose_h1, semantic::FONT_SIZE_PROSE_H1);
    assert_px!(SIZING.font_size_prose_h2, semantic::FONT_SIZE_PROSE_H2);
    assert_px!(SIZING.font_size_term_sm, semantic::FONT_SIZE_TERM_SM);
    assert_px!(SIZING.font_size_term, semantic::FONT_SIZE_TERM);
    assert_px!(SIZING.font_size_term_lg, semantic::FONT_SIZE_TERM_LG);
    assert_eq!(SIZING.line_height_prose, semantic::LINE_HEIGHT_PROSE);

    // 아이콘 글리프
    assert_px!(SIZING.icon_glyph_size_xs, semantic::ICON_SIZE_XS);
    assert_px!(SIZING.icon_glyph_size_sm, semantic::ICON_SIZE_SM);
    assert_px!(SIZING.icon_glyph_size_md, semantic::ICON_SIZE_MD);

    // readable width / form-control width (Request 2-1 / 2-2)
    assert_px!(SIZING.measure_sm, semantic::MEASURE_SM);
    assert_px!(SIZING.measure_md, semantic::MEASURE_MD);
    assert_px!(SIZING.measure_lg, semantic::MEASURE_LG);
    assert_px!(SIZING.measure_xl, semantic::MEASURE_XL);
    assert_px!(SIZING.field_width_xs, semantic::FIELD_WIDTH_XS);
    assert_px!(SIZING.field_width_color, semantic::FIELD_WIDTH_COLOR);
    assert_px!(SIZING.field_width_md, semantic::FIELD_WIDTH_MD);
    assert_px!(SIZING.field_width_lg, semantic::FIELD_WIDTH_LG);

    // 탭바/상태바/타이틀바 (Request 2-4 — zoom 제외 host chrome)
    assert_px!(SIZING.tab_bar_height, semantic::CONTROL_HEIGHT_TAB);
    assert_px!(SIZING.tab_bar_label_font_size, semantic::FONT_SIZE_BODY);
    assert_px!(SIZING.tab_bar_arrow_font_size, semantic::FONT_SIZE_CAPTION);
    assert_px!(SIZING.status_bar_height, semantic::STATUS_BAR_HEIGHT);
    assert_px!(SIZING.titlebar_height, semantic::TITLEBAR_HEIGHT);

    // 싱글턴 (Request 2-6)
    assert_px!(SIZING.overlay_top_offset, semantic::OVERLAY_TOP_OFFSET);
}

/// component 치수 ↔ SIZING.
#[test]
fn sizing_matches_component_tokens() {
    // 사이드바 (Request 2-3 — CHROME · SIDEBAR 신설 그룹)
    assert_px!(SIZING.sidebar_logo_size, component::sidebar::LOGO_SIZE);
    assert_px!(
        SIZING.sidebar_logo_collapsed_size,
        component::sidebar::LOGO_COLLAPSED_SIZE
    );
    assert_px!(
        SIZING.sidebar_wordmark_font_size,
        component::sidebar::WORDMARK_FONT_SIZE
    );
    assert_px!(
        SIZING.sidebar_section_heading_font_size,
        component::sidebar::SECTION_HEADING_FONT_SIZE
    );
    // 디자인 판정 (changelog Request 2-3): 12 → font-size-caption(11) 스냅.
    assert_px!(
        SIZING.sidebar_button_label_font_size,
        component::sidebar::BUTTON_LABEL_FONT_SIZE
    );
    assert_px!(
        SIZING.sidebar_collapsed_slot_width,
        component::sidebar::COLLAPSED_SLOT_WIDTH
    );
    assert_px!(
        SIZING.sidebar_collapsed_icon_height,
        component::sidebar::COLLAPSED_ICON_HEIGHT
    );
    assert_px!(
        SIZING.sidebar_collapsed_workspace_height,
        component::sidebar::COLLAPSED_WORKSPACE_HEIGHT
    );

    // OS 윈도우 컨트롤 (Request 2-5 — zoom 제외 OS affordance)
    assert_px!(SIZING.traffic_size, component::titlebar::TRAFFIC_SIZE);
    assert_px!(SIZING.caption_width, component::titlebar::CAPTION_WIDTH);
    assert_px!(
        SIZING.window_button_size,
        component::titlebar::WINDOW_BUTTON_SIZE
    );

    // 토스트/상태 표시 (Request 2-6 + 기존 대응)
    assert_px!(SIZING.toast_max_width, component::toast::MAX_WIDTH);
    assert_px!(SIZING.toast_accent_width, component::toast::ACCENT_WIDTH);
    assert_px!(SIZING.status_dot_size, component::status_dot::SIZE);
    assert_px!(SIZING.spinner_size, component::spinner::SIZE);
    assert_px!(SIZING.tab_indicator_width, component::tab::INDICATOR_WIDTH);
}
