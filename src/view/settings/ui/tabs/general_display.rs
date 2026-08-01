//! macOS 전용 — Alt/Option/Shift 단축키 표시 스타일.
//!
//! 저장 포맷(바인딩 문자열)은 건드리지 않고 화면 표시 문자열만 바꾼다
//! (`docs/design/policies/key-mapping.md` 저장↔표시 분리 원칙). 백엔드는
//! `GeneralSettings::{alt,option,shift}_display_style`.

use crate::i18n::t;
use crate::settings::Settings;
use tasty_ui_widgets::vspace;

pub fn draw_general_display_tab(ui: &mut egui::Ui, settings: &mut Settings) {
    let th = crate::theme::theme();
    vspace(ui, th.spacing_sm);

    egui::Grid::new("general_display_grid")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label(t("settings.general.alt_display_style_label"));
            egui::ComboBox::from_id_salt("alt_display_style")
                .selected_text(match settings.general.alt_display_style.as_str() {
                    "cmd" => t("settings.general.alt_display_style_cmd"),
                    "symbol" => t("settings.general.alt_display_style_symbol"),
                    _ => t("settings.general.alt_display_style_alt"),
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut settings.general.alt_display_style,
                        "alt".to_string(),
                        t("settings.general.alt_display_style_alt"),
                    );
                    ui.selectable_value(
                        &mut settings.general.alt_display_style,
                        "cmd".to_string(),
                        t("settings.general.alt_display_style_cmd"),
                    );
                    ui.selectable_value(
                        &mut settings.general.alt_display_style,
                        "symbol".to_string(),
                        t("settings.general.alt_display_style_symbol"),
                    );
                });
            ui.end_row();

            ui.label(t("settings.general.option_display_style_label"));
            egui::ComboBox::from_id_salt("option_display_style")
                .selected_text(match settings.general.option_display_style.as_str() {
                    "symbol" => t("settings.general.option_display_style_symbol"),
                    _ => t("settings.general.option_display_style_option"),
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut settings.general.option_display_style,
                        "option".to_string(),
                        t("settings.general.option_display_style_option"),
                    );
                    ui.selectable_value(
                        &mut settings.general.option_display_style,
                        "symbol".to_string(),
                        t("settings.general.option_display_style_symbol"),
                    );
                });
            ui.end_row();

            ui.label(t("settings.general.shift_display_style_label"));
            egui::ComboBox::from_id_salt("shift_display_style")
                .selected_text(match settings.general.shift_display_style.as_str() {
                    "symbol" => t("settings.general.shift_display_style_symbol"),
                    _ => t("settings.general.shift_display_style_shift"),
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut settings.general.shift_display_style,
                        "shift".to_string(),
                        t("settings.general.shift_display_style_shift"),
                    );
                    ui.selectable_value(
                        &mut settings.general.shift_display_style,
                        "symbol".to_string(),
                        t("settings.general.shift_display_style_symbol"),
                    );
                });
            ui.end_row();
        });
}
