pub(crate) mod bookmark_popup;
pub(crate) mod convert_popup;
mod dialog;
mod divider;
mod egui_panels;
pub(crate) mod file_open_popup;
pub mod font_registry;
pub mod icon;
pub mod layout_context;
pub(crate) mod notification;
pub(crate) mod notification_popup;
pub mod popup;
pub(crate) mod popup_defs;
mod sidebar;
mod tab_bar;
pub mod toast;

pub use dialog::draw_ws_rename_dialog;
pub use divider::{draw_pane_dividers, draw_surface_highlights};
pub use egui_panels::draw_egui_panels;
pub use layout_context::LayoutContext;
pub use notification::draw_popups;
pub use popup::{PopupAction, PopupManager};
pub use tab_bar::draw_pane_tab_bars;
pub use toast::{ToastManager, ToastScope};

use crate::model::Rect;
use crate::state::AppState;

/// Render the egui UI and return the remaining terminal area rect (in physical pixels).
pub fn draw_ui(ctx: &egui::Context, state: &mut AppState, scale_factor: f32) -> Rect {
    let sidebar_width = state.sidebar_width.value();

    if !state.sidebar_visible {
        // Sidebar hidden — skip rendering entirely
    } else if state.sidebar_collapsed {
        let (expand, settings, tools, switch_ws, add_ws) =
            sidebar::draw_collapsed_sidebar(ctx, state, sidebar_width);

        if expand {
            state.sidebar_collapsed = false;
        }
        if settings {
            state.settings_open = true;
        }
        if tools {
            crate::clipboard_viewer_ui::open_clipboard_viewer_popup(state);
        }
        if let Some(i) = switch_ws {
            state.switch_workspace(i);
        }
        if add_ws {
            if let Err(e) = state.add_workspace() {
                tracing::warn!("add_workspace failed: {e}");
            }
        }
    } else {
        let (collapse, settings, tools) = sidebar::draw_full_sidebar(ctx, state, sidebar_width);

        if collapse {
            state.sidebar_collapsed = true;
        }
        if settings {
            state.settings_open = true;
        }
        if tools {
            crate::clipboard_viewer_ui::open_clipboard_viewer_popup(state);
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
