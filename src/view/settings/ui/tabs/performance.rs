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
        SettingsRow::new(t("settings.performance.scrollback_disk_lines"))
            .hint(t("settings.performance.scrollback_disk_lines_desc")),
    ];
    let col = settings_label_column(ui, &th, &rows);
    let [polling, disk_swap, disk_lines] = rows;
    ui.spacing_mut().item_spacing.y = th.settings_row_gap().value();

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
    // -1 is the unlimited value, so the field accepts it as its minimum.
    disk_lines.show(ui, &th, col, |ui| {
        let mut lines = settings.performance.scrollback_disk_lines as f64;
        if super::number::number_field(
            ui,
            &th,
            "performance_scrollback_disk_lines",
            &super::number::NumberSpec::int(
                crate::settings::UNLIMITED_SCROLLBACK_DISK_LINES as f64,
                SCROLLBACK_DISK_LINES_MAX,
            )
            .enabled(settings.performance.scrollback_disk_swap),
            &mut lines,
        ) {
            settings.performance.scrollback_disk_lines = lines as i64;
        }
    });
}

/// Largest value the field accepts; a larger value in the settings file is kept as written.
const SCROLLBACK_DISK_LINES_MAX: f64 = 100_000_000.0;
