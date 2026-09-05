//! Markdown open — 디자인(4) Overlays `markdown` Spec.
//!
//! 420px 모달. title + 파일명 mono; 2개 Choice 카드버튼(Edit / Preview, on =
//! accent border + ring); 각 카드 icon + title + sub; Cancel / Open preview.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{Button, ButtonVariant};

use crate::catalog::icons::{self, MockGlyph};
use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::catalog::widgets::dialog as kit;

const WIDTH: f32 = 420.0;

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        kit::frame_card(ui, theme, LogicalPx(WIDTH), kit::panel_fill(theme), |ui| {
            kit::region_sym(
                ui,
                theme.spacing_md.value(),
                theme.spacing_md.value(),
                |ui| {
                    ui.spacing_mut().item_spacing.y = theme.spacing_md.value();
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = theme.spacing_xs.value();
                        kit::title(ui, theme, "Open markdown file");
                        kit::caption(ui, theme, "README.md", true);
                    });
                    // 2 Choice 카드.
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = theme.spacing_md.value();
                        let cw = (WIDTH - theme.spacing_md.value() * 3.0) * 0.5;
                        choice(
                            ui,
                            theme,
                            cw,
                            icons::MARKDOWN,
                            "Rendered preview",
                            "Formatted view with headings and links.",
                            true,
                        );
                        choice(
                            ui,
                            theme,
                            cw,
                            icons::EDIT,
                            "Raw text",
                            "Edit the source in the editor surface.",
                            false,
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            Button::new("Open preview")
                                .variant(ButtonVariant::Primary)
                                .show(ui, theme);
                            Button::new("Cancel")
                                .variant(ButtonVariant::Ghost)
                                .show(ui, theme);
                        });
                    });
                },
            );
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            ("frame", "420px · bg-panel"),
            ("file", "filename mono caption"),
            ("choices", "2 cards · flex 1 · padding 12"),
            ("selected", "accent border + focus ring"),
            ("footer", "Cancel · Open <choice>"),
        ],
        &[
            TokenChip::new(
                "accent-primary",
                "selected card",
                theme.accent_primary().to_egui(),
            ),
            TokenChip::new(
                "border-default",
                "idle card",
                theme.border_default().to_egui(),
            ),
            TokenChip::new(
                "surface-raised",
                "card fill",
                theme.surface_raised().to_egui(),
            ),
        ],
    );
}

fn choice(
    ui: &mut egui::Ui,
    theme: &Theme,
    width: f32,
    glyph: MockGlyph,
    title: &str,
    sub: &str,
    selected: bool,
) {
    let border = if selected {
        theme.accent_primary()
    } else {
        theme.border_default()
    };
    let bw = if selected {
        theme.focus_ring_width.value()
    } else {
        theme.border_width.value()
    };
    egui::Frame::new()
        .fill(theme.surface_raised().to_egui())
        .stroke(egui::Stroke::new(bw, border.to_egui()))
        .corner_radius(theme.corner_radius.value())
        .inner_margin(egui::Margin::same(theme.spacing_md.value() as i8))
        .show(ui, |ui| {
            ui.set_width(width - theme.spacing_md.value() * 2.0);
            ui.spacing_mut().item_spacing.y = theme.spacing_xs.value();
            let icon_color = if selected {
                theme.accent_primary()
            } else {
                theme.text_secondary()
            };
            kit::icon(
                ui,
                glyph,
                theme.icon_glyph_size_md.value(),
                icon_color.to_egui(),
            );
            ui.label(
                egui::RichText::new(title)
                    .size(theme.font_size_body.value())
                    .strong()
                    .color(theme.text_primary().to_egui()),
            );
            ui.label(
                egui::RichText::new(sub)
                    .size(theme.font_size_caption.value())
                    .color(theme.text_muted().to_egui()),
            );
        });
}
