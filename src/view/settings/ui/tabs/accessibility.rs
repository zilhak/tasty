use crate::i18n::t;
use crate::settings::Settings;

pub fn draw_accessibility_tab(ui: &mut egui::Ui, settings: &mut Settings) {
    let th = crate::theme::theme();
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
            .color(th.subtext0),
    );
}
