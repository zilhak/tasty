use crate::model::Rect;
use crate::state::AppState;
use crate::theme;

/// Draw pane dividers (borders between split panes).
pub fn draw_pane_dividers(ctx: &egui::Context, dividers: &[Rect], scale_factor: f32) {
    let th = theme::theme();
    if dividers.is_empty() {
        return;
    }
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Middle,
        egui::Id::new("pane_dividers"),
    ));
    let border_color = th.surface2;
    for div in dividers {
        let rect = egui::Rect::from_min_size(
            egui::pos2(div.x / scale_factor, div.y / scale_factor),
            egui::vec2(div.width / scale_factor, div.height / scale_factor),
        );
        painter.rect_filled(rect, 0.0, border_color);
    }
}

/// Draw highlight borders around surfaces that have unread notifications.
pub fn draw_surface_highlights(ctx: &egui::Context, state: &AppState, terminal_rect: Rect, scale_factor: f32) {
    let th = theme::theme();
    let regions = state.render_regions(terminal_rect);
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Middle,
        egui::Id::new("surface_highlights"),
    ));
    for (_pane_id, _pane_rect, terminal_regions) in &regions {
        for (surface_id, _terminal, rect) in terminal_regions {
            if state.engine.notifications.is_surface_highlighted(*surface_id) {
                let egui_rect = egui::Rect::from_min_size(
                    egui::pos2(rect.x / scale_factor, rect.y / scale_factor),
                    egui::vec2(rect.width / scale_factor, rect.height / scale_factor),
                );
                painter.rect_stroke(egui_rect, 0.0, egui::Stroke::new(1.0, th.blue), egui::StrokeKind::Inside);
            }
        }
    }
}
