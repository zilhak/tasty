use crate::i18n::t;
use crate::settings::Settings;
use tasty_ui_widgets::vspace;

pub fn draw_notifications_tab(ui: &mut egui::Ui, settings: &mut Settings) {
    let th = crate::theme::theme();
    vspace(ui, th.spacing_sm);

    egui::Grid::new("notification_grid")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label(t("settings.notifications.enabled"));
            tasty_ui_widgets::switch(ui, &th, &mut settings.notification.enabled, None, true);
            ui.end_row();

            ui.label(t("settings.notifications.sound"));
            tasty_ui_widgets::switch(ui, &th, &mut settings.notification.sound, None, true);
            ui.end_row();

            ui.label(t("settings.notifications.coalesce_interval_label"));
            ui.add(
                egui::DragValue::new(&mut settings.notification.coalesce_ms)
                    .range(0..=5000)
                    .speed(50),
            );
            ui.end_row();
        });
}
