pub(crate) mod brand;
pub(crate) mod divider;
mod draw;
mod egui_panels;
pub(crate) mod icons;
mod sidebar;
pub(crate) mod status_bar;
pub(crate) mod switch_overlay;
pub(crate) mod tab_bar;
pub(crate) mod titlebar;

pub mod banner;
pub(crate) mod category_actions;
pub(crate) mod dialog;
pub(crate) mod drop_overlay;
pub mod font_registry;
pub mod fullscreen;
pub(crate) mod info_modal;
pub mod layout_context;
pub(crate) mod modifier_hint_overlay;
pub(crate) mod mouse_capture_menu;
pub(crate) mod notification;
pub(crate) mod overlay;
pub mod popup;
pub mod preset;
pub(crate) mod search_bar;
pub mod surface;
pub mod terminal_link;
pub mod toast;
pub(crate) mod tools_menu;
pub(crate) mod tutorial;

pub mod input;

pub use banner::{BannerManager, BannerScope, BannerState, PluginBannerCloseKind};
pub use divider::{draw_pane_dividers, draw_surface_highlights};
pub use draw::draw_ui;
pub use egui_panels::draw_egui_panels;
pub use layout_context::LayoutContext;
pub use popup::{PopupAction, PopupManager};
pub use status_bar::{draw_status_bar, status_bar_bottom_inset};
pub use tab_bar::draw_pane_tab_bars;
pub use toast::{ToastKind, ToastManager, ToastScope};

/// 배율 밖에 있는 호스트 chrome 치수를 현재 UI 배율로 올린다.
///
/// `Theme` 필드는 생성 시점에 `zoomed()` 를 한 번 거치지만, 대응 디자인 토큰이 없어
/// 파일 안 명명 const 로 남은 치수는 그 경로 밖이라 배율을 안 탄다. 그 상태로 두면
/// **그릇만 고정되고 안의 글자·아이콘은 커진다** — 0.85 에서 여백이 뜨고 1.2 에서 내용이
/// 잘린다(ADR-0126 "그릇과 내용은 같은 편이어야 한다"). 반올림 지점을 `zoomed()` 와 같게
/// 맞춰 배율 1 에서 값이 변하지 않는다.
///
/// **토큰이 생기면 이 함수가 아니라 `Theme` 필드로 옮겨간다** — 여기 있는 것은 토큰이
/// 아직 없다는 표시이지 별도 스케일이라는 뜻이 아니다.
#[inline]
pub(crate) fn zoomed_px(
    theme: &tasty_type_appearance::theme::Theme,
    px: tasty_type_geometry::length::LogicalPx,
) -> tasty_type_geometry::length::LogicalPx {
    tasty_type_geometry::length::LogicalPx((px.value() * theme.ui_zoom).round())
}

/// popup + 오버레이 체인(toast/banner/modifier-hint/tutorial) 조립 진입점. 매 프레임
/// `egui_bridge.rs` 가 호출한다.
///
/// **z-order (뒤→앞)**: popup 자체 → toast → banner → modifier-hint → tutorial.
/// ADR-0024 가 Modal/Popup/Toast/Banner 를 서로 다른 개념으로 나눠 popup 루프
/// (`popup::frame::draw_popup_layer`)와 오버레이 체인(`overlay::draw_overlays`)이
/// 별도 모듈로 쪼개져 있지만, 화면 겹침 순서는 아래 두 호출의 순서 자체로 여전히
/// 여기 고정된다 — 순서를 바꾸면 뒤 레이어가 앞 레이어를 가리는 시각적 회귀가 된다.
/// 전체화면 무대 draw 진입점. **일반 프레임과 별개인 무대 프레임**에서만 호출된다
/// (`Gpu::render` 의 무대 분기) — 무대가 켜져 있으면 host chrome·popup·오버레이는
/// 이 프레임에 아예 그려지지 않는다.
pub fn draw_fullscreen_stage(
    ctx: &egui::Context,
    state: &mut crate::state::AppState,
    engine: &mut crate::core::CoreState,
) {
    fullscreen::draw_fullscreen_stage(ctx, state, engine);
}

pub fn draw_popups(
    ctx: &egui::Context,
    state: &mut crate::state::AppState,
    engine: &mut crate::core::CoreState,
    pane_rects: &[(u32, crate::model::PhysicalRect)],
    terminal_rect: crate::model::PhysicalRect,
    scale_factor: f32,
) {
    let draw_ctx = layout_context::build_layout_context(
        state,
        engine,
        pane_rects,
        terminal_rect,
        scale_factor,
    );

    // 무대 닫힘 훅 drain — 무대를 나온 다음 프레임은 일반 프레임이라 무대 draw 경로가
    // 돌지 않는다. 두 경로 모두에서 drain 해야 훅이 유실되지 않는다.
    fullscreen::drain_on_close_hooks(ctx, state, engine);

    popup::frame::draw_popup_layer(ctx, state, engine, &draw_ctx);
    overlay::draw_overlays(ctx, state, engine, &draw_ctx, terminal_rect, scale_factor);
}

/// 물리 사각형을 egui 가 그리는 논리 좌표 사각형으로 내린다.
///
/// 변환 자체는 [`PhysicalRect::to_logical`] 이 하고 여기서는 egui 타입으로 옮기기만
/// 한다. 네 변을 각각 `÷ scale_factor` 하던 자리를 이 한 곳으로 모은 것 — 나눗셈이
/// 네 번이면 하나를 빠뜨려도 컴파일이 통과하지만, 변환이 한 번이면 빠뜨릴 것이 없다.
///
/// `.value()` 로 타입을 벗기는 것은 여기서 정당하다. egui 는 `f32` 를 받는 외부
/// 라이브러리이고, 정책이 허용하는 것이 정확히 그 경계다
/// (`docs/concepts/typed-length.md` "외부 API 경계에서만 `.value()`").
pub(crate) fn to_egui_rect(rect: crate::model::PhysicalRect, scale_factor: f32) -> egui::Rect {
    let logical = rect.to_logical(scale_factor);
    egui::Rect::from_min_size(
        egui::pos2(logical.x.value(), logical.y.value()),
        egui::vec2(logical.width.value(), logical.height.value()),
    )
}
