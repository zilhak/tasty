use crate::i18n::t;
use crate::settings::Settings;

pub fn draw_performance_tab(ui: &mut egui::Ui, settings: &mut Settings) {
    let th = crate::theme::theme();
    ui.add_space(8.0);
    ui.label(
        egui::RichText::new(t("settings.performance.restart_notice"))
            .small()
            .color(th.yellow),
    );
    ui.add_space(12.0);

    ui.checkbox(
        &mut settings.performance.targeted_pty_polling,
        t("settings.performance.targeted_pty_polling"),
    );
    ui.label(
        egui::RichText::new(t("settings.performance.targeted_pty_polling_desc"))
            .small()
            .color(egui::Color32::GRAY),
    );
    ui.add_space(8.0);

    ui.checkbox(
        &mut settings.performance.scrollback_disk_swap,
        t("settings.performance.scrollback_disk_swap"),
    );
    ui.label(
        egui::RichText::new(t("settings.performance.scrollback_disk_swap_desc"))
            .small()
            .color(egui::Color32::GRAY),
    );
    ui.add_space(8.0);

    ui.checkbox(
        &mut settings.performance.lazy_pty_init,
        t("settings.performance.lazy_pty_init"),
    );
    ui.label(
        egui::RichText::new(t("settings.performance.lazy_pty_init_desc"))
            .small()
            .color(egui::Color32::GRAY),
    );
}

