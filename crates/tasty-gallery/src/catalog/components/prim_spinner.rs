//! `Spinner` primitive specimen — 디자인(4) `components/feedback/Spinner` 카드.
//!
//! 기본 spinner-size(16) 회전 arc + 저대비 track · 크기 램프 · accent 색 ·
//! reduced-motion 3-dot fallback. 하단 `meta` 로 치수/토큰 노출.

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::Spinner;

use crate::catalog::spec::{StageVariant, TokenChip, cluster, meta, stage};

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    let base = theme.spinner_size.value();
    stage(ui, theme, StageVariant::Column, |ui| {
        cluster(ui, theme, "default — spinner-size arc + low-contrast track", |ui| {
            Spinner::new().show(ui, theme);
            ui.label(
                egui::RichText::new("Collecting…")
                    .size(theme.font_size_body.value())
                    .color(egui::Color32::from(theme.text_muted())),
            );
        });
        cluster(ui, theme, "size — 12 · 16 · 20 · 24", |ui| {
            Spinner::new().size(12.0).show(ui, theme);
            Spinner::new().size(base).show(ui, theme);
            Spinner::new().size(20.0).show(ui, theme);
            Spinner::new().size(24.0).show(ui, theme);
        });
        cluster(ui, theme, "color — accent (currentColor 덮어씀)", |ui| {
            Spinner::new()
                .size(24.0)
                .color(egui::Color32::from(theme.accent_primary()))
                .show(ui, theme);
            Spinner::new()
                .size(24.0)
                .color(egui::Color32::from(theme.accent_success()))
                .show(ui, theme);
        });
        cluster(ui, theme, "reduced motion — 정지 + 3-dot fallback", |ui| {
            Spinner::new().size(base).reduced_motion(true).show(ui, theme);
            Spinner::new().size(24.0).reduced_motion(true).show(ui, theme);
        });
    });

    meta(
        ui,
        theme,
        &[
            ("sizes", "12 · 16 · 20 · 24"),
            ("stroke", "2px arc + track"),
            ("spin", "0.9s"),
            ("reduced", "→ 3 dots"),
        ],
        &[
            TokenChip::new(
                "text-muted",
                "default color",
                egui::Color32::from(theme.text_muted()),
            ),
            TokenChip::new(
                "spinner-size",
                "default 16",
                egui::Color32::from(theme.accent_primary()),
            ),
        ],
    );
}
