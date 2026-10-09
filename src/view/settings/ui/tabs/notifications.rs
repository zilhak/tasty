use crate::i18n::t;
use crate::settings::Settings;
use tasty_ui_widgets::{SettingsRow, settings_label_column, vspace};

pub fn draw_notifications_tab(ui: &mut egui::Ui, settings: &mut Settings) {
    let th = crate::theme::theme();
    vspace(ui, th.spacing_sm);

    let rows = [
        SettingsRow::new(t("settings.notifications.enabled")),
        SettingsRow::new(t("settings.notifications.sound")),
        SettingsRow::new(t("settings.notifications.coalesce_interval_label")),
    ];
    let col = settings_label_column(ui, &th, &rows);
    let [enabled, sound, coalesce_row] = rows;
    ui.spacing_mut().item_spacing.y = th.settings_row_gap().value();

    enabled.show(ui, &th, col, |ui| {
        tasty_ui_widgets::switch(ui, &th, &mut settings.notification.enabled, None, true);
    });
    sound.show(ui, &th, col, |ui| {
        tasty_ui_widgets::switch(ui, &th, &mut settings.notification.sound, None, true);
    });
    coalesce_row.show(ui, &th, col, |ui| {
        let mut coalesce = settings.notification.coalesce_ms as f64;
        if super::number::number_field(
            ui,
            &th,
            "notification_coalesce_ms",
            &super::number::NumberSpec::int(0.0, 5000.0),
            &mut coalesce,
        ) {
            settings.notification.coalesce_ms = coalesce as u64;
        }
    });
}
