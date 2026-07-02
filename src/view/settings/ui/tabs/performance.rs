use crate::i18n::t;
use crate::settings::Settings;
use tasty_ui_widgets::vspace;

pub fn draw_performance_tab(ui: &mut egui::Ui, settings: &mut Settings) {
    let th = crate::theme::theme();
    vspace(ui, th.spacing_sm);
    ui.label(
        egui::RichText::new(t("settings.performance.restart_notice"))
            .small()
            .color(th.accent_warning()),
    );
    vspace(ui, th.spacing_md);

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
            .color(th.text_muted()),
    );
    vspace(ui, th.spacing_sm);

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
            .color(th.text_muted()),
    );
}
