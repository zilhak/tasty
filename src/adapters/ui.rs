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

/// popup + 오버레이 체인(toast/banner/modifier-hint/tutorial) 조립 진입점. 매 프레임
/// `egui_bridge.rs` 가 호출한다.
///
/// **z-order (뒤→앞)**: popup 자체 → toast → banner → modifier-hint → tutorial.
/// ADR-0024 가 Modal/Popup/Toast/Banner 를 서로 다른 개념으로 나눠 popup 루프
/// (`popup::frame::draw_popup_layer`)와 오버레이 체인(`overlay::draw_overlays`)이
/// 별도 모듈로 쪼개져 있지만, 화면 겹침 순서는 아래 두 호출의 순서 자체로 여전히
/// 여기 고정된다 — 순서를 바꾸면 뒤 레이어가 앞 레이어를 가리는 시각적 회귀가 된다.
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

    popup::frame::draw_popup_layer(ctx, state, engine, &draw_ctx);
    overlay::draw_overlays(ctx, state, engine, &draw_ctx, terminal_rect, scale_factor);
}
