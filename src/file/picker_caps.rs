//! 파일 피커 path bar 가 쓰는 치수 — `component.fp-*` 토큰을 한 번 읽어 배율을 건다.
//!
//! **왜 뷰 밖인가.** `src/design_token_guard.rs` 는 UI 층이 생성 DTCG 길이 상수를 직접
//! 소비하는 것을 막는다 — const 는 `zoomed()` 밖이라 `ui_scale` 을 안 타기 때문이고,
//! 그 처방은 "같은 값의 `Theme` 필드를 경유해라" 다. 그런데 **component 층 토큰에는 그
//! 길이 없다**: `Theme` 은 `tasty-type-appearance` 에 살고 그 크레이트는 자기 설명에
//! `tasty-design-tokens` 의존을 금지하며, 토큰→Theme 매핑표
//! (`dtcg::SEMANTIC_DIM_TO_THEME_FIELD`)는 semantic 층만 담는다. 본체 UI 가 component
//! 층 **길이**를 쓰는 것은 이 자리가 처음이다(기존 둘은 무차원 opacity 라 가드가
//! 통과시킨다).
//!
//! 그래서 토큰을 읽는 자리를 뷰에서 떼어 여기 둔다. 배율은 여기서 한 번만 걸고
//! (ADR-0135: 배율은 토큰에만 건다), 뷰는 이미 배율이 걸린 수만 받는다 — 가드가
//! 막으려는 실패(배율을 빠뜨리는 것)가 뷰 쪽에서 구조적으로 불가능해진다.

use tasty_ui_widgets::crumb_alloc::Caps;

/// 토큰 값을 이 프레임의 배율로 잰다.
fn scaled(v: tasty_type_geometry::length::LogicalPx, ui_zoom: f32) -> f32 {
    (v.value() * ui_zoom).round()
}

/// path bar 프레임의 치수 — 전부 `component.fp-*` 토큰이다.
pub fn caps(ui_zoom: f32) -> Caps {
    use tasty_design_tokens::generated::component::fp::{
        BAR_HYSTERESIS, CRUMB_CURRENT_MIN_WIDTH, CRUMB_MAX_WIDTH, CRUMB_MIN_WIDTH,
    };
    Caps {
        crumb_max: scaled(CRUMB_MAX_WIDTH, ui_zoom),
        ancestor_min: scaled(CRUMB_MIN_WIDTH, ui_zoom),
        current_min: scaled(CRUMB_CURRENT_MIN_WIDTH, ui_zoom),
        hysteresis: scaled(BAR_HYSTERESIS, ui_zoom),
    }
}

/// `…` 메뉴 폭의 밴드 — `component.fp-crumb-menu-{min,max}-width`.
pub fn menu_band(ui_zoom: f32) -> (f32, f32) {
    use tasty_design_tokens::generated::component::fp::{
        CRUMB_MENU_MAX_WIDTH, CRUMB_MENU_MIN_WIDTH,
    };
    (
        scaled(CRUMB_MENU_MIN_WIDTH, ui_zoom),
        scaled(CRUMB_MENU_MAX_WIDTH, ui_zoom),
    )
}
