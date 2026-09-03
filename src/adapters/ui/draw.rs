//! UI 전체 진입점 — sidebar 를 그리고 남은 terminal 영역 PhysicalRect 를 반환.

use crate::core::CoreState;
use crate::intent::Intent;
use crate::model::PhysicalRect;
use crate::state::AppState;
use tasty_type_geometry::length::{LogicalPx, PhysicalPx};

use super::sidebar;

/// Render the egui UI and return the remaining terminal area rect (in physical pixels).
pub fn draw_ui(
    ctx: &egui::Context,
    state: &mut AppState,
    engine: &mut CoreState,
    scale_factor: f32,
    plugin_alert: usize,
) -> PhysicalRect {
    let sidebar_width = state.sidebar_width.value();

    if !state.sidebar_visible {
        // Sidebar hidden — skip rendering entirely
    } else if state.sidebar_collapsed {
        let r = sidebar::draw_collapsed_sidebar(ctx, state, engine, sidebar_width, plugin_alert);

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
                    category: None,
                }
                .from_user_menu("sidebar_add_workspace"),
            );
        }
    } else {
        let r = sidebar::draw_full_sidebar(ctx, state, engine, sidebar_width, plugin_alert);

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
    // egui 가 준 값은 전부 logical 이다 — 물리로 올리는 곱셈은 `to_physical` 한 곳으로.
    // 생성자로 감싸기만 하면 곱셈을 빠뜨려도 컴파일되므로 변환을 타입에 맡긴다.
    let terminal_x = LogicalPx(sidebar_width).to_physical(scale_factor);
    let terminal_y = top_inset;
    let terminal_width = LogicalPx(screen_rect.width() - sidebar_width).to_physical(scale_factor);
    let terminal_height = LogicalPx(screen_rect.height()).to_physical(scale_factor) - top_inset;

    PhysicalRect {
        x: terminal_x,
        y: terminal_y,
        width: PhysicalPx(terminal_width.value().max(1.0)),
        height: PhysicalPx(terminal_height.value().max(1.0)),
    }
}
