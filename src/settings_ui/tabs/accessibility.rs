use crate::i18n::t;
use crate::settings::Settings;

pub fn draw_accessibility_tab(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.add_space(8.0);
    ui.heading(t("settings.accessibility.heading"));
    ui.add_space(12.0);

    ui.checkbox(
        &mut settings.accessibility.reduced_motion,
        t("settings.accessibility.reduced_motion"),
    );
    ui.label(
        egui::RichText::new(t("settings.accessibility.reduced_motion_desc"))
            .small()
            .color(egui::Color32::GRAY),
    );
    ui.add_space(12.0);

    ui.add_enabled_ui(false, |ui| {
        ui.checkbox(
            &mut settings.accessibility.high_contrast,
            t("settings.accessibility.high_contrast"),
        );
    });
    ui.label(
        egui::RichText::new(t("settings.accessibility.high_contrast_desc"))
            .small()
            .color(egui::Color32::GRAY),
    );
}
