//! Design LoadingOverlay and PendingIndicator. Callers own time, layout and focus.
use crate::Spinner;
use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;

#[derive(Clone, Copy)]
pub enum LoadingPlacement {
    Surface,
    Tab,
    Lnb,
    Rail,
}

/// Paint-only overlay: neither allocates layout nor takes keyboard/pointer input.
pub fn paint_loading_overlay(
    ui: &mut egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    placement: LoadingPlacement,
    bed: egui::Color32,
) {
    let (slot, size) = match placement {
        LoadingPlacement::Surface => {
            let inset = theme.loading_overlay_surface_inset();
            let bed_size = theme.loading_overlay_surface_bed_size();
            let slot = egui::Rect::from_min_size(
                rect.right_bottom() - egui::Vec2::splat((inset + bed_size).value()),
                egui::Vec2::splat(bed_size.value()),
            );
            ui.painter().rect(
                slot,
                theme.corner_radius.value(),
                theme.loading_overlay_surface_bed_bg().to_egui(),
                egui::Stroke::new(
                    theme.border_width.value(),
                    theme.loading_overlay_surface_bed_border().to_egui(),
                ),
                egui::StrokeKind::Inside,
            );
            (slot, theme.loading_overlay_surface_size().value())
        }
        LoadingPlacement::Tab => (rect, theme.loading_overlay_tab_size().value()),
        LoadingPlacement::Lnb => (rect, theme.loading_overlay_lnb_size().value()),
        LoadingPlacement::Rail => (rect, theme.loading_overlay_rail_size().value()),
    };
    if matches!(placement, LoadingPlacement::Tab | LoadingPlacement::Lnb) {
        ui.painter().rect_filled(slot, 0.0, bed);
    }
    // LoadingOverlay.jsx specifies a 1.72 viewBox stroke for the rail ring.
    let spinner = if matches!(placement, LoadingPlacement::Rail) {
        Spinner::new().stroke(1.72)
    } else {
        Spinner::new()
    };
    spinner
        .size(size)
        .color(theme.loading_overlay_indicator().to_egui())
        .still_arc()
        .paint_in(ui, theme, slot);
}

pub struct PendingIndicator<'a> {
    pub label: &'a str,
    pub time: &'a str,
    pub compact: bool,
    /// One line per pending kind, each with its own elapsed time.
    pub details: &'a str,
}
impl PendingIndicator<'_> {
    pub fn show(&self, ui: &mut egui::Ui, theme: &Theme) -> egui::Response {
        let gap = theme.pending_indicator_gap();
        let padding = theme.pending_indicator_padding_x();
        let spinner = theme.pending_indicator_spinner_size();
        let height = theme.pending_indicator_height();
        let font = egui::FontId::proportional(theme.font_size_caption.value());
        let mono = egui::FontId::monospace(theme.font_size_caption.value());
        let time = ui.painter().layout_no_wrap(
            self.time.to_owned(),
            mono,
            theme.pending_indicator_time_fg().to_egui(),
        );
        let available = LogicalPx(ui.available_width());
        let compact = self.compact || available < theme.pending_indicator_compact_below();
        let label = ui.painter().layout_no_wrap(
            self.label.to_owned(),
            font.clone(),
            theme.pending_indicator_fg().to_egui(),
        );
        let separator = ui.painter().layout_no_wrap(
            "·".into(),
            font,
            theme.pending_indicator_sep_fg().to_egui(),
        );
        let extra = if compact {
            LogicalPx(0.0)
        } else {
            LogicalPx(label.size().x + separator.size().x) + gap + gap
        };
        let width =
            (padding + spinner + gap + LogicalPx(time.size().x) + extra + padding).min(available);
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(width.value(), height.value()),
            egui::Sense::hover(),
        );
        ui.painter().rect(
            rect,
            theme.pending_indicator_radius().value(),
            theme.pending_indicator_bg().to_egui(),
            egui::Stroke::new(
                theme.border_width.value(),
                theme.pending_indicator_border().to_egui(),
            ),
            egui::StrokeKind::Inside,
        );
        let mut x = LogicalPx(rect.left()) + padding;
        let slot = egui::Rect::from_center_size(
            egui::pos2((x + spinner * 0.5).value(), rect.center().y),
            egui::Vec2::splat(spinner.value()),
        );
        Spinner::new()
            .size(spinner.value())
            .still_arc()
            .color(theme.loading_overlay_indicator().to_egui())
            .paint_in(ui, theme, slot);
        x += spinner + gap;
        if !compact {
            let budget = (LogicalPx(rect.right())
                - padding
                - LogicalPx(time.size().x)
                - gap
                - LogicalPx(separator.size().x)
                - gap
                - x)
                .max(LogicalPx(0.0));
            let painter = ui.painter().with_clip_rect(egui::Rect::from_min_max(
                egui::pos2(x.value(), rect.top()),
                egui::pos2((x + budget).value(), rect.bottom()),
            ));
            painter.galley(
                egui::pos2(x.value(), rect.center().y - label.size().y * 0.5),
                label.clone(),
                theme.pending_indicator_fg().to_egui(),
            );
            x += budget.min(LogicalPx(label.size().x)) + gap;
            ui.painter().galley(
                egui::pos2(x.value(), rect.center().y - separator.size().y * 0.5),
                separator.clone(),
                theme.pending_indicator_sep_fg().to_egui(),
            );
            x += LogicalPx(separator.size().x) + gap;
        }
        ui.painter().galley(
            egui::pos2(x.value(), rect.center().y - time.size().y * 0.5),
            time,
            theme.pending_indicator_time_fg().to_egui(),
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Label,
                true,
                format!("{} · {}", self.label, self.time),
            )
        });
        response.on_hover_text(self.details)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loading_slots_do_not_allocate_or_take_input() {
        let mut theme = tasty_themes::mocha_fallback();
        theme.reduced_motion = true;
        let ctx = egui::Context::default();
        drop(ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let cursor = ui.cursor();
                let minimum = ui.min_rect();
                for placement in [
                    LoadingPlacement::Surface,
                    LoadingPlacement::Tab,
                    LoadingPlacement::Lnb,
                    LoadingPlacement::Rail,
                ] {
                    paint_loading_overlay(
                        ui,
                        &theme,
                        ui.max_rect(),
                        placement,
                        theme.bg_panel().to_egui(),
                    );
                    assert_eq!(ui.cursor(), cursor);
                    assert_eq!(ui.min_rect(), minimum);
                }
            });
        }));
    }

    #[test]
    fn pending_chip_uses_design_height_and_hover_only_sense() {
        let theme = tasty_themes::mocha_fallback();
        let ctx = egui::Context::default();
        drop(ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let full = PendingIndicator {
                    label: "Waiting for 3 responses",
                    time: "1m 04s",
                    compact: false,
                    details: "Close tab · 1m 04s\nInput · 3s",
                }
                .show(ui, &theme);
                let compact = PendingIndicator {
                    label: "Waiting for 3 responses",
                    time: "1m 04s",
                    compact: true,
                    details: "Close tab · 1m 04s\nInput · 3s",
                }
                .show(ui, &theme);
                assert_eq!(full.rect.height(), theme.pending_indicator_height().value());
                assert_eq!(compact.rect.height(), full.rect.height());
                assert!(compact.rect.width() < full.rect.width());
                assert!(!full.sense.senses_click() && !full.sense.senses_drag());
            });
        }));
    }
}
