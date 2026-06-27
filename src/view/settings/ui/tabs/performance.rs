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

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = th.spacing_lg.value();
        ui.label(t("settings.performance.targeted_pty_polling"));
        tasty_ui_widgets::switch(
            ui,
            &th,
            &mut settings.performance.targeted_pty_polling,
            None,
            true,
        );
    });
    ui.label(
        egui::RichText::new(t("settings.performance.targeted_pty_polling_desc"))
            .small()
            .color(th.subtext0),
    );
    ui.add_space(8.0);

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = th.spacing_lg.value();
        ui.label(t("settings.performance.scrollback_disk_swap"));
        tasty_ui_widgets::switch(
            ui,
            &th,
            &mut settings.performance.scrollback_disk_swap,
            None,
            true,
        );
    });
    ui.label(
        egui::RichText::new(t("settings.performance.scrollback_disk_swap_desc"))
            .small()
            .color(th.subtext0),
    );
}
