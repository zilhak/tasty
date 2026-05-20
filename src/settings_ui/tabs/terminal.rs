use crate::i18n::t;
use crate::settings::{GeneralSettings, Settings};

pub fn draw_terminal_tab(ui: &mut egui::Ui, settings: &mut Settings) {
    let th = crate::theme::theme();
    ui.add_space(8.0);

    if !settings.general.is_shell_valid() {
        ui.label(egui::RichText::new(t("settings.terminal.shell_not_found")).color(th.yellow));
        ui.add_space(4.0);
    }

    egui::Grid::new("terminal_grid")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label(t("settings.terminal.shell_label"));
            if let Some(detected) = GeneralSettings::detect_bash() {
                if settings.general.shell.is_empty() || !settings.general.is_shell_valid() {
                    settings.general.shell = detected;
                }
            }
            ui.text_edit_singleline(&mut settings.general.shell);
            ui.end_row();

            ui.label(t("settings.terminal.shell_mode_label"));
            egui::ComboBox::from_id_salt("shell_mode")
                .selected_text(match settings.general.shell_mode.as_str() {
                    "tasty" => t("settings.terminal.shell_mode_tasty"),
                    "custom" => t("settings.terminal.shell_mode_custom"),
                    _ => t("settings.terminal.shell_mode_default"),
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut settings.general.shell_mode,
                        "default".to_string(),
                        t("settings.terminal.shell_mode_default"),
                    );
                    ui.selectable_value(
                        &mut settings.general.shell_mode,
                        "tasty".to_string(),
                        t("settings.terminal.shell_mode_tasty"),
                    );
                    ui.selectable_value(
                        &mut settings.general.shell_mode,
                        "custom".to_string(),
                        t("settings.terminal.shell_mode_custom"),
                    );
                });
            ui.end_row();

            if settings.general.shell_mode == "custom" {
                ui.label(t("settings.terminal.shell_args_label"));
                ui.text_edit_singleline(&mut settings.general.shell_args);
                ui.end_row();
            }

            ui.label(t("settings.terminal.startup_command_label"));
            ui.text_edit_singleline(&mut settings.general.startup_command);
            ui.end_row();

            ui.label(t("settings.terminal.scrollback_lines_label"));
            ui.add(
                egui::DragValue::new(&mut settings.general.scrollback_lines)
                    .range(0..=100000)
                    .speed(100),
            );
            ui.end_row();

            ui.label(t("settings.terminal.confirm_close_label"));
            ui.checkbox(&mut settings.general.confirm_close_running, "");
            ui.end_row();

            ui.label(t("settings.terminal.inherit_cwd_label"));
            ui.checkbox(&mut settings.general.inherit_cwd, "");
            ui.end_row();

            ui.label(t("settings.terminal.link_modifier_label"));
            egui::ComboBox::from_id_salt("link_modifier")
                .selected_text(match settings.general.link_click_modifier.as_str() {
                    "alt" => t("settings.terminal.link_modifier_alt"),
                    "none" => t("settings.terminal.link_modifier_none"),
                    _ => t("settings.terminal.link_modifier_ctrl"),
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut settings.general.link_click_modifier,
                        "ctrl".to_string(),
                        t("settings.terminal.link_modifier_ctrl"),
                    );
                    ui.selectable_value(
                        &mut settings.general.link_click_modifier,
                        "alt".to_string(),
                        t("settings.terminal.link_modifier_alt"),
                    );
                    ui.selectable_value(
                        &mut settings.general.link_click_modifier,
                        "none".to_string(),
                        t("settings.terminal.link_modifier_none"),
                    );
                });
            ui.end_row();
        });
}
