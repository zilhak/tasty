use crate::i18n::t;
use crate::settings::Settings;

pub fn draw_general_tab(ui: &mut egui::Ui, settings: &mut Settings) {
    let th = crate::theme::theme();
    ui.add_space(8.0);

    egui::Grid::new("general_grid")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label(t("settings.general.restore_layout_label"));
            ui.checkbox(&mut settings.general.restore_layout, "");
            ui.end_row();

            ui.label(t("settings.general.restore_terminal_content_label"));
            ui.checkbox(&mut settings.general.restore_terminal_content, "");
            ui.end_row();

            ui.label(t("settings.general.close_behavior_label"));
            egui::ComboBox::from_id_salt("close_behavior")
                .selected_text(match settings.general.close_behavior.as_str() {
                    "quit" => t("settings.general.close_behavior_quit"),
                    "minimize" => t("settings.general.close_behavior_minimize"),
                    _ => t("settings.general.close_behavior_ask"),
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut settings.general.close_behavior,
                        "ask".to_string(),
                        t("settings.general.close_behavior_ask"),
                    );
                    ui.selectable_value(
                        &mut settings.general.close_behavior,
                        "minimize".to_string(),
                        t("settings.general.close_behavior_minimize"),
                    );
                    ui.selectable_value(
                        &mut settings.general.close_behavior,
                        "quit".to_string(),
                        t("settings.general.close_behavior_quit"),
                    );
                });
            ui.end_row();

            ui.label(t("settings.general.language_label"));
            egui::ComboBox::from_id_salt("language_select")
                .selected_text(language_display_name(&settings.general.language))
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut settings.general.language,
                        "en".to_string(),
                        "English",
                    );
                    ui.selectable_value(&mut settings.general.language, "ko".to_string(), "한국어");
                    ui.selectable_value(&mut settings.general.language, "ja".to_string(), "日本語");
                });
            ui.end_row();
        });

    ui.add_space(8.0);
    ui.label(
        egui::RichText::new(t("settings.general.language_restart_notice"))
            .small()
            .color(th.accent_warning()),
    );
}

fn language_display_name(code: &str) -> &str {
    match code {
        "en" => "English",
        "ko" => "한국어",
        "ja" => "日本語",
        _ => code,
    }
}
