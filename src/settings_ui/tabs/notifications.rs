use crate::i18n::t;
use crate::settings::Settings;

pub fn draw_notifications_tab(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.add_space(8.0);

    ui.checkbox(
        &mut settings.notification.enabled,
        t("settings.notifications.enabled"),
    );
    ui.checkbox(
        &mut settings.notification.system_notification,
        t("settings.notifications.system_notification"),
    );
    ui.checkbox(
        &mut settings.notification.sound,
        t("settings.notifications.sound"),
    );

    ui.add_space(8.0);
    egui::Grid::new("notification_grid")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label(t("settings.notifications.coalesce_interval_label"));
            ui.add(
                egui::DragValue::new(&mut settings.notification.coalesce_ms)
                    .range(0..=5000)
                    .speed(50),
            );
            ui.end_row();
        });
}
