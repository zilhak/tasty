use tasty_ui_widgets::{Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant, Input};

use crate::adapters::ui::icons;
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

    // 마우스 캡처 비활성화 블랙리스트 — 행 리스트 에디터(패턴 mono + × 제거) +
    // 하단 Add 입력/버튼. 디자인 `BlacklistEditorG`(overlays-shared.jsx) 전사로,
    // 옛 멀티라인 textarea(줄바꿈 구분)를 폐기한다. 매칭(trim/대소문자 무시/`*`)은
    // 별도 헬퍼가 담당하므로 여기선 패턴 문자열만 보관한다.
    ui.add_space(12.0);
    ui.label(t("settings.terminal.mouse_capture_blacklist_label"));
    ui.add_space(4.0);

    if settings.general.mouse_capture_blacklist.is_empty() {
        // 빈 상태 — neutral 톤(경고색 아님).
        ui.label(
            egui::RichText::new(t("settings.terminal.mouse_capture_blacklist_empty"))
                .small()
                .color(th.text_muted()),
        );
    } else {
        let mut remove_idx: Option<usize> = None;
        for (i, pattern) in settings.general.mouse_capture_blacklist.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(pattern)
                        .monospace()
                        .color(th.text_primary()),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if IconButton::new()
                        .variant(IconButtonVariant::Ghost)
                        .size(ControlSize::Sm)
                        .show(ui, &th, &|ui, rect, c| {
                            icons::CLOSE.image(rect.height(), c).paint_at(ui, rect);
                        })
                        .clicked()
                    {
                        remove_idx = Some(i);
                    }
                });
            });
        }
        if let Some(i) = remove_idx {
            settings.general.mouse_capture_blacklist.remove(i);
        }
    }

    // Add 행 — 입력 필드(남는 폭) + Add 버튼(입력 비면 disabled). 입력 버퍼는
    // 프레임 간 egui temp memory 에 보관한다(Settings 모델은 확정 패턴만 담는다).
    ui.add_space(6.0);
    let add_id = ui.id().with("mouse_capture_blacklist_add");
    let mut add_buf: String = ui.data_mut(|d| d.get_temp::<String>(add_id).unwrap_or_default());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
        let can_add = !add_buf.trim().is_empty();
        // 우측 Add 버튼을 먼저 배치 → Input 이 남는 폭을 채운다(디자인 block).
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let add_clicked = Button::new(t("settings.terminal.mouse_capture_blacklist_add_button"))
                .variant(ButtonVariant::Secondary)
                .size(ControlSize::Sm)
                .enabled(can_add)
                .show(ui, &th)
                .clicked();
            let resp = Input::new()
                .placeholder(t("settings.terminal.mouse_capture_blacklist_add_placeholder"))
                .mono(true)
                .show(ui, &th, &mut add_buf);
            let submit = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (add_clicked || submit) && !add_buf.trim().is_empty() {
                settings
                    .general
                    .mouse_capture_blacklist
                    .push(add_buf.trim().to_string());
                add_buf.clear();
            }
        });
    });
    ui.data_mut(|d| d.insert_temp(add_id, add_buf));

    // match-rule notice — accent-warning 톤(빈 상태 neutral 과 구분).
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(t("settings.terminal.mouse_capture_blacklist_notice"))
            .small()
            .color(th.accent_warning()),
    );
}
