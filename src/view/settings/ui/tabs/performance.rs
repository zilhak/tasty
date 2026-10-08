use crate::i18n::t;
use crate::settings::Settings;
use tasty_ui_widgets::{SettingsRow, settings_label_column, vspace};

pub fn draw_performance_tab(ui: &mut egui::Ui, settings: &mut Settings) {
    let th = crate::theme::theme();
    vspace(ui, th.spacing_sm);
    ui.label(
        egui::RichText::new(t("settings.performance.restart_notice"))
            .small()
            .color(th.accent_warning()),
    );
    vspace(ui, th.spacing_md);

    let rows = [
        SettingsRow::new(t("settings.performance.targeted_pty_polling"))
            .hint(t("settings.performance.targeted_pty_polling_desc")),
        SettingsRow::new(t("settings.performance.scrollback_disk_swap"))
            .hint(t("settings.performance.scrollback_disk_swap_desc")),
    ];
    let col = settings_label_column(ui, &th, &rows);
    let [polling, disk_swap] = rows;
    ui.spacing_mut().item_spacing.y = th.spacing_sm.value();

    polling.show(ui, &th, col, |ui| {
        tasty_ui_widgets::switch(
            ui,
            &th,
            &mut settings.performance.targeted_pty_polling,
            None,
            true,
        );
    });
    disk_swap.show(ui, &th, col, |ui| {
        tasty_ui_widgets::switch(
            ui,
            &th,
            &mut settings.performance.scrollback_disk_swap,
            None,
            true,
        );
    });
}
