//! Shared attach loading parts from the design loading gallery.
use crate::catalog::spec::{StageVariant, stage};
use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{LoadingPlacement, PendingIndicator, paint_loading_overlay};

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    stage(ui, theme, StageVariant::Column, |ui| {
        for placement in [
            LoadingPlacement::Surface,
            LoadingPlacement::Tab,
            LoadingPlacement::Lnb,
            LoadingPlacement::Rail,
        ] {
            let size =
                theme.loading_overlay_rail_size() + theme.loading_overlay_surface_inset() * 2.0;
            ui.horizontal(|ui| {
                // Toggle comparisons use the same allocated rectangle.
                for visible in [false, true] {
                    let (rect, _) = ui
                        .allocate_exact_size(egui::Vec2::splat(size.value()), egui::Sense::hover());
                    let bed = theme.surface_raised().to_egui();
                    ui.painter()
                        .rect_filled(rect, theme.corner_radius.value(), bed);
                    if visible {
                        paint_loading_overlay(ui, theme, rect, placement, bed);
                    }
                }
            });
        }
        PendingIndicator {
            label: "Waiting for response",
            time: "0s",
            compact: false,
        }
        .show(ui, theme);
        PendingIndicator {
            label: "Waiting for 3 responses",
            time: "1m 04s",
            compact: false,
        }
        .show(ui, theme);
        PendingIndicator {
            label: "Waiting for 3 responses",
            time: "1m 04s",
            compact: true,
        }
        .show(ui, theme);
        let mut still = theme.clone();
        still.reduced_motion = true;
        PendingIndicator {
            label: "Waiting for response",
            time: "1h 02m",
            compact: false,
        }
        .show(ui, &still);
    });
}

// Design storyboard frames (layouts-loading-prelayout.jsx W=560/H=180), not product dimensions.
const FRAME_WIDTH: tasty_type_geometry::length::LogicalPx =
    tasty_type_geometry::length::LogicalPx(560.0);
const FRAME_HEIGHT: tasty_type_geometry::length::LogicalPx =
    tasty_type_geometry::length::LogicalPx(180.0);

pub fn draw_prelayout(ui: &mut egui::Ui, theme: &Theme) {
    stage(ui, theme, StageVariant::Column, |ui| {
        for (name, kind) in [
            ("New tab", 0),
            ("Surface split · columns", 1),
            ("Surface split · rows", 2),
            ("Pane split · columns", 3),
            ("Pane split · rows", 4),
        ] {
            ui.label(name);
            for loading in [false, true] {
                prelayout(ui, theme, kind, loading);
            }
        }
    });
}
fn prelayout(ui: &mut egui::Ui, theme: &Theme, kind: usize, loading: bool) {
    let (frame, _) = ui.allocate_exact_size(
        egui::vec2(
            FRAME_WIDTH.min(LogicalPx(ui.available_width())).value(),
            FRAME_HEIGHT.value(),
        ),
        egui::Sense::hover(),
    );
    let border = theme.border_width;
    ui.painter()
        .rect_filled(frame, 0.0, theme.separator.to_egui_premultiplied());
    let horizontal = kind == 2 || kind == 4;
    let split = |rect: egui::Rect| {
        if horizontal {
            let y = LogicalPx(rect.top()) + (LogicalPx(rect.height()) - border) * 0.5;
            (
                egui::Rect::from_min_max(rect.min, egui::pos2(rect.right(), y.value())),
                egui::Rect::from_min_max(egui::pos2(rect.left(), (y + border).value()), rect.max),
            )
        } else {
            let x = LogicalPx(rect.left()) + (LogicalPx(rect.width()) - border) * 0.5;
            (
                egui::Rect::from_min_max(rect.min, egui::pos2(x.value(), rect.bottom())),
                egui::Rect::from_min_max(egui::pos2((x + border).value(), rect.top()), rect.max),
            )
        }
    };
    if kind >= 3 {
        let (old, new) = split(frame);
        let old_content = preview_tabs(ui, theme, old, false, false);
        preview_content(ui, theme, old_content, false, false);
        let new_content = preview_tabs(ui, theme, new, true, loading);
        preview_content(ui, theme, new_content, true, loading);
    } else {
        let content = preview_tabs(ui, theme, frame, kind == 0, loading);
        if kind == 0 {
            let (old_header, new_header) = split(frame);
            preview_tabs(ui, theme, old_header, false, false);
            preview_tabs(ui, theme, new_header, true, loading);
            preview_content(ui, theme, content, true, loading);
        } else {
            let (old, new) = split(content);
            preview_content(ui, theme, old, false, false);
            preview_content(ui, theme, new, true, loading);
        }
    }
}
fn preview_tabs(
    ui: &mut egui::Ui,
    theme: &Theme,
    frame: egui::Rect,
    new: bool,
    loading: bool,
) -> egui::Rect {
    let header = egui::Rect::from_min_size(
        frame.min,
        egui::vec2(frame.width(), theme.tab_bar_height.value()),
    );
    ui.painter()
        .rect_filled(header, 0.0, theme.surface_raised());
    let inset = theme.spacing_sm;
    let slot = egui::Rect::from_min_size(
        header.min
            + egui::vec2(
                inset.value(),
                ((LogicalPx(header.height()) - theme.loading_overlay_tab_size()) * 0.5).value(),
            ),
        egui::Vec2::splat(theme.loading_overlay_tab_size().value()),
    );
    tasty_icons::TERMINAL
        .image(slot.width(), theme.text_primary().to_egui())
        .paint_at(ui, slot);
    if loading {
        paint_loading_overlay(
            ui,
            theme,
            slot,
            LoadingPlacement::Tab,
            theme.surface_raised().to_egui(),
        );
    }
    ui.painter().text(
        egui::pos2(
            (LogicalPx(slot.right()) + theme.spacing_xs).value(),
            header.center().y,
        ),
        egui::Align2::LEFT_CENTER,
        if new { "New tab" } else { "server" },
        egui::FontId::proportional(theme.tab_bar_label_font_size.value()),
        theme.text_primary().to_egui(),
    );
    egui::Rect::from_min_max(egui::pos2(frame.left(), header.bottom()), frame.max)
}
fn preview_content(ui: &mut egui::Ui, theme: &Theme, rect: egui::Rect, blank: bool, loading: bool) {
    ui.painter().rect_filled(rect, 0.0, theme.bg_panel());
    if !blank {
        ui.painter().text(
            rect.min + egui::Vec2::splat(theme.spacing_sm.value()),
            egui::Align2::LEFT_TOP,
            "$ build",
            egui::FontId::monospace(theme.font_size_caption.value()),
            theme.text_secondary().to_egui(),
        );
    }
    if loading {
        paint_loading_overlay(
            ui,
            theme,
            rect,
            LoadingPlacement::Surface,
            theme.bg_panel().to_egui(),
        );
    }
}
