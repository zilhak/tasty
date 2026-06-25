use crate::i18n::t;
use crate::settings::Settings;

pub fn draw_performance_tab(ui: &mut egui::Ui, settings: &mut Settings) {
    let th = crate::theme::theme();
    ui.add_space(8.0);
    ui.label(
        egui::RichText::new(t("settings.performance.restart_notice"))
            .small()
            .color(th.accent_warning()),
    );
    ui.add_space(12.0);

    ui.checkbox(
        &mut settings.performance.targeted_pty_polling,
        t("settings.performance.targeted_pty_polling"),
    );
    ui.label(
        egui::RichText::new(t("settings.performance.targeted_pty_polling_desc"))
            .small()
            .color(th.subtext0),
    );
    ui.add_space(8.0);

    ui.checkbox(
        &mut settings.performance.scrollback_disk_swap,
        t("settings.performance.scrollback_disk_swap"),
    );
    ui.label(
        egui::RichText::new(t("settings.performance.scrollback_disk_swap_desc"))
            .small()
            .color(th.subtext0),
    );
}
