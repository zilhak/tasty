//! `Spinner` primitive specimen — `tasty_ui_widgets::Spinner` 격리 카탈로그.
//!
//! 디자인 gallery `components/feedback/Spinner` 대조용. 기본 16px 회전 arc(저대비
//! track) · 크기 변형 · 커스텀 색 · reduced-motion 3-dot fallback.

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::Spinner;

use crate::catalog::specimen::caption;

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    ui.spacing_mut().item_spacing = egui::vec2(16.0, 10.0);

    caption(ui, theme, "default 16px · 회전 arc + 저대비 track (0.22 alpha)");
    ui.horizontal(|ui| {
        Spinner::new().show(ui, theme);
        ui.label(
            egui::RichText::new("Collecting…")
                .size(theme.font_size_body.value())
                .color(egui::Color32::from(theme.subtext0)),
        );
    });

    ui.add_space(10.0);
    caption(ui, theme, "size — 12 · 16 · 24 · 32");
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 16.0;
        Spinner::new().size(12.0).show(ui, theme);
        Spinner::new().size(16.0).show(ui, theme);
        Spinner::new().size(24.0).show(ui, theme);
        Spinner::new().size(32.0).show(ui, theme);
    });

    ui.add_space(10.0);
    caption(ui, theme, "color — accent (text-muted 기본값을 덮어씀)");
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 16.0;
        Spinner::new()
            .size(24.0)
            .color(egui::Color32::from(theme.accent_primary()))
            .show(ui, theme);
        Spinner::new()
            .size(24.0)
            .color(egui::Color32::from(theme.accent_success()))
            .show(ui, theme);
    });

    ui.add_space(10.0);
    caption(ui, theme, "reduced motion — 회전 정지 + 3-dot 정적 fallback");
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 16.0;
        Spinner::new().size(16.0).reduced_motion(true).show(ui, theme);
        Spinner::new().size(24.0).reduced_motion(true).show(ui, theme);
    });
}
