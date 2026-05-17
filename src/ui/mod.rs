pub(crate) mod approval_popup;
pub(crate) mod convert_popup;
pub(crate) mod dialog;
mod divider;
pub(crate) mod drop_overlay;
mod egui_panels;
pub mod image_view;
pub mod markdown_view;
pub(crate) mod file_handler_picker_popup;
pub(crate) mod file_open_popup;
pub mod font_registry;
pub(crate) mod info_modal;
pub mod layout_context;
pub(crate) mod notification;
pub(crate) mod notification_popup;
pub mod popup;
pub(crate) mod popup_defs;
pub(crate) mod port_scanner_popup;
mod sidebar;
mod tab_bar;
pub(crate) mod search_bar;
pub(crate) mod tools_menu;
pub mod toast;

pub use divider::{draw_pane_dividers, draw_surface_highlights};
pub use egui_panels::draw_egui_panels;
pub use layout_context::LayoutContext;
pub use notification::draw_popups;
pub use popup::{PopupAction, PopupManager};
pub use tab_bar::draw_pane_tab_bars;
pub use toast::{ToastKind, ToastManager, ToastScope};

use crate::model::Rect;
use crate::state::AppState;

/// 도구 버튼 위쪽, 좌측에 붙여서 tools_menu 팝업을 연다.
fn open_tools_menu(state: &mut AppState, btn_rect: egui::Rect) {
    // tools_menu의 default_size를 popup_defs에서 가져온다
    let menu_size = popup_defs::find("tools_menu")
        .map(|d| d.default_size)
        .unwrap_or(egui::vec2(160.0, 36.0));
    // 버튼 좌측에 맞추고, 버튼 위쪽으로 올라가도록 배치
    let pos = egui::pos2(btn_rect.min.x, btn_rect.min.y - menu_size.y);
    state.popups.open_at_focused("tools_menu", pos);
}

/// Render the egui UI and return the remaining terminal area rect (in physical pixels).
pub fn draw_ui(ctx: &egui::Context, state: &mut AppState, scale_factor: f32) -> Rect {
    let sidebar_width = state.sidebar_width.value();

    if !state.sidebar_visible {
        // Sidebar hidden — skip rendering entirely
    } else if state.sidebar_collapsed {
        let r = sidebar::draw_collapsed_sidebar(ctx, state, sidebar_width);

        if r.expand_clicked {
            state.sidebar_collapsed = false;
        }
        if r.plugins_clicked {
            state.plugins_open = true;
        }
        if r.settings_clicked {
            state.settings_open = true;
        }
        if let Some(btn_rect) = r.tools_rect {
            open_tools_menu(state, btn_rect);
        }
        if let Some(i) = r.switch_ws {
            state.switch_workspace(i);
        }
        if r.add_ws {
            if let Err(e) = state.add_workspace() {
                tracing::warn!("add_workspace failed: {e}");
            }
        }
    } else {
        let r = sidebar::draw_full_sidebar(ctx, state, sidebar_width);

        if r.collapse_clicked {
            state.sidebar_collapsed = true;
        }
        if r.plugins_clicked {
            state.plugins_open = true;
        }
        if r.settings_clicked {
            state.settings_open = true;
        }
        if let Some(btn_rect) = r.tools_rect {
            open_tools_menu(state, btn_rect);
        }
    }

    // Compute remaining terminal area in physical pixels
    use crate::model::length::PhysicalPx;
    let screen_rect = ctx.screen_rect();
    let terminal_x = PhysicalPx(sidebar_width * scale_factor);
    let terminal_y = PhysicalPx(0.0);
    let terminal_width = PhysicalPx((screen_rect.width() - sidebar_width) * scale_factor);
    let terminal_height = PhysicalPx(screen_rect.height() * scale_factor);

    Rect {
        x: terminal_x,
        y: terminal_y,
        width: PhysicalPx(terminal_width.value().max(1.0)),
        height: PhysicalPx(terminal_height.value().max(1.0)),
    }
}
