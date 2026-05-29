use crate::i18n::t;
use crate::settings::Settings;

pub fn draw_language_tab(ui: &mut egui::Ui, settings: &mut Settings) {
    let th = crate::theme::theme();
    ui.add_space(8.0);

    egui::Grid::new("language_grid")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label(t("settings.language.label"));
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
        egui::RichText::new(t("settings.language.restart_notice"))
            .small()
            .color(th.yellow),
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
