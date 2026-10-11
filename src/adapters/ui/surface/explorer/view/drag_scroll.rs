//! Item drags use the same edge acceleration as marquee selection.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;

use super::drag::{DragState, Payload};
use super::marquee;

pub(crate) fn feed(
    ui: &egui::Ui,
    theme: &Theme,
    drag: &DragState,
    viewport: egui::Rect,
    row: LogicalPx,
) {
    if drag.blocked || !egui::DragAndDrop::has_payload_of_type::<Payload>(ui.ctx()) {
        return;
    }
    let (down, pos, dt) = ui.input(|i| {
        (
            i.pointer.primary_down(),
            i.pointer.latest_pos(),
            i.stable_dt,
        )
    });
    let Some(pos) = pos.filter(|_| down) else {
        return;
    };
    let dy = delta(
        viewport.intersect(ui.clip_rect()),
        pos,
        theme.explorer_autoscroll_zone(),
        row,
        dt,
    );
    if dy != LogicalPx(0.0) {
        // Feed the next ScrollArea without replacing wheel input or its stored offset.
        ui.scroll_with_delta_animation(
            egui::vec2(0.0, -dy.value()),
            egui::style::ScrollAnimation::none(),
        );
        ui.ctx().request_repaint();
    }
}

fn delta(
    viewport: egui::Rect,
    pos: egui::Pos2,
    zone: LogicalPx,
    row: LogicalPx,
    dt: f32,
) -> LogicalPx {
    if !viewport.is_positive() || !viewport.x_range().contains(pos.x) {
        return LogicalPx(0.0);
    }
    LogicalPx(marquee::autoscroll(
        viewport.y_range(),
        pos.y,
        zone.value(),
        row.value(),
        dt,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_row_pitch_clipped_edges_and_horizontal_ownership_control_scroll() {
        let rect = egui::Rect::from_min_max(egui::pos2(100.0, 100.0), egui::pos2(400.0, 500.0));
        for row in [22.0, 28.0, 114.0] {
            let scroll =
                |x, y| delta(rect, egui::pos2(x, y), LogicalPx(24.0), LogicalPx(row), 0.1).value();
            assert_eq!(scroll(200.0, 300.0), 0.0);
            assert_eq!(scroll(99.0, 500.0), 0.0);
            assert_eq!(scroll(401.0, 500.0), 0.0);
            assert_eq!(scroll(200.0, 500.0), 2.0 * row);
            assert_eq!(scroll(200.0, 900.0), 2.0 * row);
            assert_eq!(scroll(200.0, 0.0), -2.0 * row);
            assert_eq!(scroll(200.0, 488.0), 0.5 * row);
        }
    }
}
