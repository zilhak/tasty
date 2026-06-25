//! Update (Tier 3) — 디자인(4) Overlays `update` Spec.
//!
//! 380px 모달. 헤더(release icon accent + title + "tier 3" Tag, border-bottom) ·
//! 버전 델타(mono) + 요약 + Release notes 링크 · Later / Update now.

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{tag, Button, ButtonVariant, TagVariant};

use crate::catalog::icons;
use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::catalog::widgets::dialog as kit;

const WIDTH: f32 = 380.0;

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        kit::frame_card(ui, theme, WIDTH, kit::panel_fill(theme), |ui| {
            // 헤더.
            kit::region_sym(ui, theme.spacing_md.value(), theme.spacing_md.value(), |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                    kit::icon(ui, icons::ROCKET, theme.icon_glyph_size_md.value(), theme.accent_primary().to_egui());
                    kit::title(ui, theme, "Update available");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        tag(ui, theme, "tier 3", TagVariant::Accent, false);
                    });
                });
            });
            kit::hsep(ui, theme);

            // 본문.
            kit::region_sym(ui, theme.spacing_md.value(), theme.spacing_md.value(), |ui| {
                ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
                // 버전 델타.
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                    ui.label(
                        egui::RichText::new("1.4.2")
                            .monospace()
                            .size(theme.font_size_body.value())
                            .color(theme.text_muted().to_egui()),
                    );
                    kit::icon(ui, icons::CHEVRON_RIGHT, theme.icon_glyph_size_sm.value(), theme.text_muted().to_egui());
                    ui.label(
                        egui::RichText::new("1.5.0")
                            .monospace()
                            .size(theme.font_size_body.value())
                            .strong()
                            .color(theme.accent_primary().to_egui()),
                    );
                });
                kit::body(ui, theme, "Mouse reporting for tracking apps, gallery parity, and a faster VTE path.");
                ui.label(
                    egui::RichText::new("Release notes →")
                        .size(theme.font_size_caption.value())
                        .color(theme.accent_info().to_egui()),
                );
            });
            kit::hsep(ui, theme);

            // footer.
            kit::region_sym(ui, theme.spacing_md.value(), theme.spacing_sm.value(), |ui| {
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        Button::new("Update now").variant(ButtonVariant::Primary).show(ui, theme);
                        Button::new("Later").variant(ButtonVariant::Ghost).show(ui, theme);
                    });
                });
            });
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            ("frame", "380px · bg-panel"),
            ("header", "release icon · title · tier 3 Tag"),
            ("delta", "mono 1.4.2 → 1.5.0"),
            ("body", "summary · Release notes link"),
            ("footer", "Later · Update now"),
        ],
        &[
            TokenChip::new("accent-primary", "new version", theme.accent_primary().to_egui()),
            TokenChip::new("accent-info", "notes link", theme.accent_info().to_egui()),
            TokenChip::new("text-muted", "old version", theme.text_muted().to_egui()),
        ],
    );
}
