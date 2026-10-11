//! Item drags use the same edge acceleration as marquee selection.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;

use super::drag::{DragState, Payload};
use super::marquee;

#[derive(Clone, Copy)]
struct Area {
    id: egui::Id,
    viewport: egui::Rect,
}

#[derive(Clone)]
struct Pending {
    area: Area,
    delta: LogicalPx,
    max_offset: LogicalPx,
}

#[derive(Clone, Default)]
struct Areas(Vec<Pending>);

fn in_column(rect: egui::Rect, x: LogicalPx) -> bool {
    x >= LogicalPx(rect.left()) && x < LogicalPx(rect.right())
}

fn owner<'a>(areas: impl IntoIterator<Item = &'a Area>, pos: egui::Pos2) -> Option<egui::Id> {
    let contains = |area: &Area| {
        in_column(area.viewport, LogicalPx(pos.x))
            && LogicalPx(pos.y) >= LogicalPx(area.viewport.top())
            && LogicalPx(pos.y) < LogicalPx(area.viewport.bottom())
    };
    let distance = |area: &Area| LogicalPx(area.viewport.distance_to_pos(pos));
    areas
        .into_iter()
        .min_by(|a, b| {
            contains(b)
                .cmp(&contains(a))
                .then_with(|| {
                    distance(a)
                        .partial_cmp(&distance(b))
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .then_with(|| a.id.value().cmp(&b.id.value()))
        })
        .map(|area| area.id)
}

/// Register completed ScrollAreas; ownership is resolved from this pass only.
pub(crate) fn feed<R>(
    ui: &egui::Ui,
    theme: &Theme,
    drag: &DragState,
    output: &egui::scroll_area::ScrollAreaOutput<R>,
    row: LogicalPx,
) {
    let ctx = ui.ctx();
    let key = egui::Id::new("explorer_drag_scroll_areas");
    let install = ctx.data_mut(|data| {
        let install = data.get_temp::<Areas>(key).is_none();
        data.get_temp_mut_or_default::<Areas>(key);
        install
    });
    if install {
        ctx.on_end_pass("explorer_drag_scroll", std::sync::Arc::new(finish));
    }
    let viewport = output.inner_rect.intersect(ui.clip_rect());
    if !viewport.is_positive() {
        return;
    }
    let enabled = !drag.blocked && egui::DragAndDrop::has_payload_of_type::<Payload>(ctx);
    let dy = ctx.input(|i| {
        i.pointer
            .latest_pos()
            .filter(|_| i.pointer.primary_down() && enabled)
            .map_or(LogicalPx(0.0), |pos| {
                delta(
                    viewport,
                    pos,
                    theme.explorer_autoscroll_zone(),
                    row,
                    i.stable_dt,
                )
            })
    });
    ctx.data_mut(|data| {
        data.get_temp_mut_or_default::<Areas>(key).0.push(Pending {
            area: Area {
                id: output.id,
                viewport,
            },
            delta: dy,
            max_offset: LogicalPx((output.content_size.y - output.inner_rect.height()).max(0.0)),
        });
    });
}

fn finish(ctx: &egui::Context) {
    let areas = ctx.data_mut(|data| {
        std::mem::take(
            &mut data
                .get_temp_mut_or_default::<Areas>(egui::Id::new("explorer_drag_scroll_areas"))
                .0,
        )
    });
    let Some(pos) = ctx.pointer_latest_pos() else {
        return;
    };
    let Some(id) = owner(areas.iter().map(|pending| &pending.area), pos) else {
        return;
    };
    let pending = areas.iter().find(|pending| pending.area.id == id).unwrap();
    if pending.delta == LogicalPx(0.0) {
        return;
    }
    if let Some(mut state) = egui::scroll_area::State::load(ctx, id) {
        // Add to the completed wheel/marquee result, without carrying old geometry forward.
        state.offset.y = (LogicalPx(state.offset.y) + pending.delta)
            .value()
            .clamp(0.0, pending.max_offset.value());
        state.store(ctx, id);
        ctx.request_repaint();
    }
}

fn delta(
    viewport: egui::Rect,
    pos: egui::Pos2,
    zone: LogicalPx,
    row: LogicalPx,
    dt: f32,
) -> LogicalPx {
    if !viewport.is_positive() || !in_column(viewport, LogicalPx(pos.x)) {
        return LogicalPx(0.0);
    }
    marquee::autoscroll(viewport.y_range(), LogicalPx(pos.y), zone, row, dt)
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
            assert_eq!(scroll(400.0, 500.0), 0.0);
            assert_eq!(scroll(401.0, 500.0), 0.0);
            assert_eq!(scroll(200.0, 500.0), 2.0 * row);
            assert_eq!(scroll(200.0, 900.0), 2.0 * row);
            assert_eq!(scroll(200.0, 0.0), -2.0 * row);
            assert_eq!(scroll(200.0, 488.0), 0.5 * row);
        }
    }
}

#[cfg(test)]
mod ownership_tests {
    use super::*;

    #[test]
    fn vertically_aligned_panes_choose_one_viewport_and_keep_the_nearest_edge_outside() {
        let area = |name, top, bottom| Area {
            id: egui::Id::new(name),
            viewport: egui::Rect::from_min_max(egui::pos2(100.0, top), egui::pos2(400.0, bottom)),
        };
        let a = area("top", 100.0, 300.0);
        let b = area("bottom", 350.0, 550.0);
        let areas = [a, b];
        assert_eq!(owner(&areas, egui::pos2(200.0, 500.0)), Some(b.id));
        assert_eq!(owner(&areas, egui::pos2(200.0, 900.0)), Some(b.id));
        assert_eq!(owner(&areas, egui::pos2(200.0, 0.0)), Some(a.id));
        assert_eq!(owner(&areas, egui::pos2(400.0, 250.0)), Some(a.id));
        assert_eq!(
            owner(&areas, egui::pos2(200.0, 325.0)),
            owner(&[b, a], egui::pos2(200.0, 325.0))
        );
    }

    #[test]
    fn preview_space_does_not_select_a_distant_wider_pane() {
        let top = Area {
            id: egui::Id::new("top"),
            viewport: egui::Rect::from_min_max(egui::pos2(100.0, 100.0), egui::pos2(800.0, 300.0)),
        };
        let bottom = Area {
            id: egui::Id::new("bottom"),
            viewport: egui::Rect::from_min_max(egui::pos2(100.0, 350.0), egui::pos2(1100.0, 550.0)),
        };
        let pos = egui::pos2(900.0, 200.0);
        assert_eq!(owner(&[top, bottom], pos), Some(top.id));
        assert_eq!(
            delta(top.viewport, pos, LogicalPx(24.0), LogicalPx(28.0), 0.1),
            LogicalPx(0.0)
        );
        let right = Area {
            id: egui::Id::new("right"),
            viewport: egui::Rect::from_min_max(egui::pos2(800.0, 100.0), egui::pos2(1100.0, 300.0)),
        };
        assert_eq!(
            owner(&[top, right], egui::pos2(800.0, 200.0)),
            Some(right.id)
        );
    }
}
