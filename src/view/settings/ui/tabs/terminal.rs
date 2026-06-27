use crate::i18n::t;
use crate::settings::{GeneralSettings, Settings};

pub fn draw_terminal_tab(ui: &mut egui::Ui, settings: &mut Settings) {
    let th = crate::theme::theme();
    ui.add_space(8.0);

    if !settings.general.is_shell_valid() {
        ui.label(
            egui::RichText::new(t("settings.terminal.shell_not_found")).color(th.accent_warning()),
        );
        ui.add_space(4.0);
    }

    egui::Grid::new("terminal_grid")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label(t("settings.terminal.shell_label"));
            if let Some(detected) = GeneralSettings::detect_bash()
                && (settings.general.shell.is_empty() || !settings.general.is_shell_valid())
            {
                settings.general.shell = detected;
            }
            ui.text_edit_singleline(&mut settings.general.shell);
            ui.end_row();

            // 셸 모드는 Windows 전용 — OSC7/MSYS PATH 빌트인 적용 여부 결정.
            // 비-Windows 에서는 의미가 없어 UI 에서도 노출하지 않는다.
            #[cfg(windows)]
            {
                ui.label(t("settings.terminal.shell_mode_label"));
                egui::ComboBox::from_id_salt("shell_mode")
                    .selected_text(match settings.general.shell_mode.as_str() {
                        "tasty" => t("settings.terminal.shell_mode_tasty"),
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
                    });
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
            tasty_ui_widgets::switch(
                ui,
                &th,
                &mut settings.general.confirm_close_running,
                None,
                true,
            );
            ui.end_row();

            ui.label(t("settings.terminal.inherit_cwd_label"));
            tasty_ui_widgets::switch(ui, &th, &mut settings.general.inherit_cwd, None, true);
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

            ui.label(t("settings.terminal.allow_clipboard_read_label"));
            tasty_ui_widgets::switch(
                ui,
                &th,
                &mut settings.general.allow_clipboard_read,
                None,
                true,
            );
            ui.end_row();

            ui.label(t("settings.terminal.mouse_capture_hint_label"));
            tasty_ui_widgets::switch(
                ui,
                &th,
                &mut settings.general.mouse_capture_hint,
                None,
                true,
            );
            ui.end_row();
        });

    ui.add_space(8.0);
    ui.label(
        egui::RichText::new(t("settings.terminal.allow_clipboard_read_notice"))
            .small()
            .color(th.accent_warning()),
    );

    // 마우스 캡처 비활성화 블랙리스트 — 줄바꿈 구분 멀티라인 → Vec<String>.
    // Vec ↔ String 변환은 split/join 을 정규화 없이 왕복시켜(빈 줄 보존) egui
    // 즉시모드에서 커서 점프를 막는다. trim/빈줄 무시는 매칭 헬퍼가 담당한다.
    ui.add_space(12.0);
    ui.label(t("settings.terminal.mouse_capture_blacklist_label"));
    ui.add_space(4.0);
    let mut buf = settings.general.mouse_capture_blacklist.join("\n");
    let resp = ui.add(
        egui::TextEdit::multiline(&mut buf)
            .desired_rows(3)
            .desired_width(f32::INFINITY),
    );
    if resp.changed() {
        settings.general.mouse_capture_blacklist = buf.split('\n').map(|s| s.to_string()).collect();
    }
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(t("settings.terminal.mouse_capture_blacklist_notice"))
            .small()
            .color(th.accent_warning()),
    );
}
