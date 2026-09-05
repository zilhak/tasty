//! File handler picker — 디자인(4) Overlays `filehandler` Spec.
//!
//! 420px 모달. 헤더(title + 경로 mono, border-bottom) · 핸들러 행(icon + name +
//! origin, selected = surface-active + 2px accent inset · plugin = agent색) ·
//! footer(Always 체크 + Cancel/Open).

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::tokens::STRUCT_GAP_1;
use tasty_ui_widgets::{Button, ButtonVariant};

use crate::catalog::icons::{self, MockGlyph};
use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::catalog::widgets::dialog as kit;

const WIDTH: LogicalPx = LogicalPx(420.0);

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        kit::frame_card(ui, theme, WIDTH, kit::panel_fill(theme), |ui| {
            // 헤더.
            kit::region_sym(ui, theme.spacing_md, theme.spacing_sm, |ui| {
                ui.spacing_mut().item_spacing.y = theme.spacing_xs.value();
                kit::title(ui, theme, "Open file with…");
                kit::caption(ui, theme, "docs/architecture.md", true);
            });
            kit::hsep(ui, theme);

            // 핸들러 행.
            kit::region_sym(ui, theme.spacing_sm, theme.spacing_sm, |ui| {
                handler(
                    ui,
                    theme,
                    icons::MARKDOWN,
                    "Markdown preview",
                    "built-in",
                    true,
                    false,
                );
                handler(
                    ui,
                    theme,
                    icons::EDIT,
                    "Text editor",
                    "built-in",
                    false,
                    false,
                );
                handler(
                    ui,
                    theme,
                    icons::TERMINAL,
                    "Terminal (less)",
                    "built-in",
                    false,
                    false,
                );
                handler(
                    ui,
                    theme,
                    icons::FILE,
                    "Git diff",
                    "plugin · git-helper",
                    false,
                    true,
                );
            });
            kit::hsep(ui, theme);

            // footer.
            kit::region_sym(ui, theme.spacing_md, theme.spacing_sm, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                    check(ui, theme, true);
                    kit::body(ui, theme, "Always open .md with this");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        Button::new("Open")
                            .variant(ButtonVariant::Primary)
                            .show(ui, theme);
                        Button::new("Cancel")
                            .variant(ButtonVariant::Ghost)
                            .show(ui, theme);
                    });
                });
            });
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            ("frame", "420px · bg-panel"),
            ("header", "title · path mono · border-bottom"),
            ("row", "icon · name · origin · selected surface-active"),
            ("selected", "2px accent inset bar"),
            ("plugin", "agent-tinted origin"),
            ("footer", "Always check · Cancel · Open"),
        ],
        &[
            TokenChip::new(
                "surface-active",
                "selected row",
                theme.surface_active().to_egui(),
            ),
            TokenChip::new(
                "accent-primary",
                "inset bar",
                theme.accent_primary().to_egui(),
            ),
            TokenChip::new(
                "accent-agent",
                "plugin origin",
                theme.accent_agent().to_egui(),
            ),
        ],
    );
}

fn handler(
    ui: &mut egui::Ui,
    theme: &Theme,
    glyph: MockGlyph,
    name: &str,
    origin: &str,
    selected: bool,
    plugin: bool,
) {
    let h = theme.item_height_interactive.value() + theme.spacing_md.value();
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    if selected {
        ui.painter().rect_filled(
            rect,
            theme.corner_radius_sm.value(),
            theme.surface_active().to_egui(),
        );
        let bar = egui::Rect::from_min_size(
            rect.min,
            egui::vec2(theme.tab_indicator_width.value(), rect.height()),
        );
        ui.painter()
            .rect_filled(bar, 0.0, theme.accent_primary().to_egui());
    }
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(egui::vec2(theme.spacing_md.value(), 0.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child.spacing_mut().item_spacing.x = theme.spacing_sm.value();
    kit::icon(
        &mut child,
        glyph,
        theme.icon_glyph_size_md,
        theme.text_secondary().to_egui(),
    );
    child.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = STRUCT_GAP_1.value();
        ui.label(
            egui::RichText::new(name)
                .size(theme.font_size_body.value())
                .color(theme.text_primary().to_egui()),
        );
        let origin_color = if plugin {
            theme.accent_agent()
        } else {
            theme.text_muted()
        };
        ui.label(
            egui::RichText::new(origin)
                .size(theme.font_size_caption.value())
                .color(origin_color.to_egui()),
        );
    });
}

fn check(ui: &mut egui::Ui, theme: &Theme, checked: bool) {
    let s = theme.icon_glyph_size_md.value();
    let (r, _) = ui.allocate_exact_size(egui::vec2(s, s), egui::Sense::hover());
    if checked {
        ui.painter().rect_filled(
            r,
            theme.corner_radius_sm.value(),
            theme.accent_primary().to_egui(),
        );
        icons::SHIELD_CHECK
            .image(s, theme.text_on_accent().to_egui())
            .paint_at(ui, r);
    } else {
        ui.painter().rect_stroke(
            r,
            theme.corner_radius_sm.value(),
            egui::Stroke::new(theme.border_width.value(), theme.border_default().to_egui()),
            egui::StrokeKind::Inside,
        );
    }
}
