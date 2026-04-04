mod context_menu;
mod dialog;
mod divider;
mod non_terminal;
mod notification;
mod sidebar;
mod tab_bar;

pub use context_menu::draw_pane_context_menu;
pub use dialog::{draw_markdown_path_dialog, draw_ws_rename_dialog};
pub use divider::{draw_pane_dividers, draw_surface_highlights};
pub use non_terminal::draw_non_terminal_panels;
pub use notification::draw_notification_panel;
pub use tab_bar::draw_pane_tab_bars;

use crate::model::Rect;
use crate::state::AppState;

/// Render the egui UI and return the remaining terminal area rect (in physical pixels).
pub fn draw_ui(ctx: &egui::Context, state: &mut AppState, scale_factor: f32) -> Rect {
    let sidebar_width = state.sidebar_width;

    if !state.sidebar_visible {
        // Sidebar hidden — skip rendering entirely
    } else if state.sidebar_collapsed {
        let (expand, settings, switch_ws, add_ws) =
            sidebar::draw_collapsed_sidebar(ctx, state, sidebar_width);

        if expand { state.sidebar_collapsed = false; }
        if settings { state.settings_open = true; }
        if let Some(i) = switch_ws { state.switch_workspace(i); }
        if add_ws { let _ = state.add_workspace(); }
    } else {
        let (collapse, settings) =
            sidebar::draw_full_sidebar(ctx, state, sidebar_width);

        if collapse { state.sidebar_collapsed = true; }
        if settings { state.settings_open = true; }
    }

    // Compute remaining terminal area in physical pixels
    let screen_rect = ctx.screen_rect();
    let terminal_x = sidebar_width * scale_factor;
    let terminal_y = 0.0;
    let terminal_width = (screen_rect.width() - sidebar_width) * scale_factor;
    let terminal_height = screen_rect.height() * scale_factor;

    Rect {
        x: terminal_x,
        y: terminal_y,
        width: terminal_width.max(1.0),
        height: terminal_height.max(1.0),
    }
}
