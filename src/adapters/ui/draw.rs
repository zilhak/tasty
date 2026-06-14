//! UI 전체 진입점 — sidebar 를 그리고 남은 terminal 영역 PhysicalRect 를 반환.

use crate::core::CoreState;
use crate::intent::Intent;
use crate::model::PhysicalRect;
use crate::state::AppState;
use tasty_type_geometry::length::PhysicalPx;

use super::sidebar;

/// Render the egui UI and return the remaining terminal area rect (in physical pixels).
pub fn draw_ui(
    ctx: &egui::Context,
    state: &mut AppState,
    engine: &mut CoreState,
    scale_factor: f32,
) -> PhysicalRect {
    let sidebar_width = state.sidebar_width.value();

    if !state.sidebar_visible {
        // Sidebar hidden — skip rendering entirely
    } else if state.sidebar_collapsed {
        let r = sidebar::draw_collapsed_sidebar(ctx, state, engine, sidebar_width);

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
            sidebar::tools::open_tools_menu(state, engine, btn_rect);
        }
        if let Some(i) = r.switch_ws {
            state.switch_workspace(engine, i);
        }
        if r.add_ws {
            state.dispatch_intent(
                Intent::NewWorkspace {
                    kind: None,
                    params: serde_json::Value::Null,
                }
                .from_user_menu("sidebar_add_workspace"),
            );
        }
    } else {
        let r = sidebar::draw_full_sidebar(ctx, state, engine, sidebar_width);

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
            sidebar::tools::open_tools_menu(state, engine, btn_rect);
        }
    }

    // Compute remaining terminal area in physical pixels (below the titlebar inset).
    let screen_rect = ctx.screen_rect();
    let top_inset = super::titlebar::top_inset(scale_factor);
    let terminal_x = PhysicalPx(sidebar_width * scale_factor);
    let terminal_y = top_inset;
    let terminal_width = PhysicalPx((screen_rect.width() - sidebar_width) * scale_factor);
    let terminal_height = PhysicalPx(screen_rect.height() * scale_factor - top_inset.value());

    PhysicalRect {
        x: terminal_x,
        y: terminal_y,
        width: PhysicalPx(terminal_width.value().max(1.0)),
        height: PhysicalPx(terminal_height.value().max(1.0)),
    }
}
