use super::*;
use crate::{Button, ButtonVariant, ControlSize};

#[derive(Clone, Debug, Default)]
pub struct NotificationPanelState {
    pub expanded: Option<u64>,
    previous_order: Vec<u64>,
    anchor: Option<(u64, LogicalPx)>,
    viewport_width: Option<LogicalPx>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotificationAction {
    Read(u64),
    ReadAll,
    Jump(u64),
    Toggle(u64),
}

fn anchor_offset(rows: &[(u64, LogicalPx)], anchor: (u64, LogicalPx)) -> Option<LogicalPx> {
    let mut prefix = LogicalPx(0.0);
    for (id, height) in rows {
        if *id == anchor.0 {
            return Some(prefix + anchor.1);
        }
        prefix += *height;
    }
    None
}

pub fn notification_panel(
    ui: &mut egui::Ui,
    th: &Theme,
    rows: &[NotificationRow<'_>],
    labels: &NotificationLabels<'_>,
    state: &mut NotificationPanelState,
    wide: bool,
) -> Vec<NotificationAction> {
    let mut actions = Vec::new();
    let counts = counts(rows);
    let any = counts.iter().any(|n| *n > 0);
    ui.spacing_mut().item_spacing.y = 0.0;
    let (toolbar, _) = ui.allocate_exact_size(
        egui::vec2(
            ui.available_width(),
            th.notifications_toolbar_height().value(),
        ),
        egui::Sense::hover(),
    );
    let mut bar = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(toolbar.shrink2(egui::vec2(th.spacing_xs.value(), 0.0)))
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    if Button::new(labels.mark_all)
        .size(ControlSize::Sm)
        .variant(ButtonVariant::Ghost)
        .enabled(any)
        .show(&mut bar, th)
        .clicked()
    {
        actions.push(NotificationAction::ReadAll);
    }
    let badge = badges_size(ui, th, counts, false);
    let start = egui::pos2(
        toolbar.left() + th.notification_item_pad_x().value(),
        toolbar.center().y - badge.y * 0.5,
    );
    if any {
        paint_badges(
            ui,
            th,
            egui::Rect::from_min_size(start, badge),
            counts,
            labels.levels,
            false,
        );
    } else {
        ui.painter().text(
            egui::pos2(start.x, toolbar.center().y),
            egui::Align2::LEFT_CENTER,
            labels.no_unread,
            egui::FontId::proportional(th.font_size_caption.value()),
            th.text_muted().into(),
        );
    }
    ui.painter().hline(
        toolbar.x_range(),
        toolbar.bottom(),
        egui::Stroke::new(
            th.border_width.value(),
            th.separator.to_egui_premultiplied(),
        ),
    );
    if rows.is_empty() {
        egui::ScrollArea::vertical()
            .drag_to_scroll(false)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.centered_and_justified(|ui| {
                    ui.label(
                        egui::RichText::new(labels.empty)
                            .size(th.font_size_body.value())
                            .color(th.text_muted()),
                    );
                });
            });
        *state = NotificationPanelState::default();
        return actions;
    }
    let width = state
        .viewport_width
        .unwrap_or(LogicalPx(ui.available_width()));
    let plans: Vec<_> = rows
        .iter()
        .map(|row| item::plan(ui, th, row, width, state.expanded == Some(row.id), wide))
        .collect();
    let mut heights: Vec<_> = rows
        .iter()
        .zip(&plans)
        .map(|(row, plan)| (row.id, plan.height))
        .collect();
    let order: Vec<_> = rows.iter().map(|r| r.id).collect();
    let mut scroll = egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .drag_to_scroll(false);
    // A fullscreen close clears its ScrollArea state; do not resurrect its old anchor.
    let scroll_id = ui.make_persistent_id(egui::Id::new("scroll_area"));
    let was_scrolled = egui::containers::scroll_area::State::load(ui.ctx(), scroll_id)
        .is_some_and(|s| s.offset.y > 0.0);
    if was_scrolled
        && order != state.previous_order
        && let Some(anchor) = state.anchor
        && let Some(offset) = anchor_offset(&heights, anchor)
    {
        scroll = scroll.vertical_scroll_offset(offset.value());
    }
    let output = scroll.show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let viewport_width = LogicalPx(ui.available_width());
        state.viewport_width = Some(viewport_width);
        heights.clear();
        for row in rows {
            let plan = item::plan(
                ui,
                th,
                row,
                viewport_width,
                state.expanded == Some(row.id),
                wide,
            );
            heights.push((row.id, plan.height));
            ui.push_id(row.id, |ui| {
                if let Some(action) =
                    item::draw(ui, th, row, labels, &plan, state.expanded == Some(row.id))
                {
                    actions.push(action);
                }
            });
        }
    });
    let mut y = LogicalPx(0.0);
    state.anchor = None;
    if output.state.offset.y > 0.0 {
        for (id, height) in &heights {
            if y.value() + height.value() > output.state.offset.y {
                state.anchor = Some((*id, LogicalPx(output.state.offset.y - y.value())));
                break;
            }
            y += *height;
        }
    }
    state.previous_order = order;
    for action in &mut actions {
        if let NotificationAction::Toggle(id) = *action {
            if state.expanded == Some(id) {
                state.expanded = None;
            } else {
                state.expanded = Some(id);
                *action = NotificationAction::Read(id);
            }
        }
    }
    actions
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn arrival_preserves_the_visible_item_even_when_the_oldest_is_evicted() {
        let rows = [
            (4, LogicalPx(80.0)),
            (3, LogicalPx(120.0)),
            (2, LogicalPx(60.0)),
        ];
        assert_eq!(
            anchor_offset(&rows, (3, LogicalPx(15.0))),
            Some(LogicalPx(95.0))
        );
        assert_eq!(anchor_offset(&rows, (1, LogicalPx(15.0))), None);
    }
}
