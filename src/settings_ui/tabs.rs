use crate::i18n::t;
use crate::settings::{GeneralSettings, Settings};

pub fn draw_general_tab(ui: &mut egui::Ui, settings: &mut Settings) {
    let th = crate::theme::theme();
    ui.add_space(8.0);
    ui.heading(t("settings.general.heading"));
    ui.add_space(4.0);

    if !settings.general.is_shell_valid() {
        ui.label(
            egui::RichText::new(t("settings.general.shell_not_found"))
                .color(th.yellow),
        );
        ui.add_space(4.0);
    }

    egui::Grid::new("general_grid")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label(t("settings.general.shell_label"));
            if let Some(detected) = GeneralSettings::detect_bash() {
                if settings.general.shell.is_empty() || !settings.general.is_shell_valid() {
                    settings.general.shell = detected;
                }
            }
            ui.text_edit_singleline(&mut settings.general.shell);
            ui.end_row();

            ui.label(t("settings.general.shell_mode_label"));
            egui::ComboBox::from_id_salt("shell_mode")
                .selected_text(match settings.general.shell_mode.as_str() {
                    "fast" => t("settings.general.shell_mode_fast"),
                    "custom" => t("settings.general.shell_mode_custom"),
                    _ => t("settings.general.shell_mode_default"),
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut settings.general.shell_mode, "default".to_string(), t("settings.general.shell_mode_default"));
                    ui.selectable_value(&mut settings.general.shell_mode, "fast".to_string(), t("settings.general.shell_mode_fast"));
                    ui.selectable_value(&mut settings.general.shell_mode, "custom".to_string(), t("settings.general.shell_mode_custom"));
                });
            ui.end_row();

            if settings.general.shell_mode == "custom" {
                ui.label(t("settings.general.shell_args_label"));
                ui.text_edit_singleline(&mut settings.general.shell_args);
                ui.end_row();
            }

            ui.label(t("settings.general.startup_command_label"));
            ui.text_edit_singleline(&mut settings.general.startup_command);
            ui.end_row();

            ui.label(t("settings.general.scrollback_lines_label"));
            ui.add(egui::DragValue::new(&mut settings.general.scrollback_lines)
                .range(0..=100000)
                .speed(100));
            ui.end_row();

            ui.label(t("settings.general.confirm_close_label"));
            ui.checkbox(&mut settings.general.confirm_close_running, "");
            ui.end_row();

            ui.label(t("settings.general.inherit_cwd_label"));
            ui.checkbox(&mut settings.general.inherit_cwd, "");
            ui.end_row();
        });
}

pub fn draw_appearance_tab(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.add_space(8.0);
    ui.heading(t("settings.appearance.heading"));
    ui.add_space(4.0);

    egui::Grid::new("appearance_grid")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label(t("settings.appearance.font_family_label"));
            ui.text_edit_singleline(&mut settings.appearance.font_family);
            ui.end_row();

            ui.label(t("settings.appearance.font_size_label"));
            ui.add(egui::DragValue::new(&mut settings.appearance.font_size)
                .range(6.0..=72.0)
                .speed(0.5));
            ui.end_row();

            ui.label(t("settings.appearance.background_opacity_label"));
            ui.add(egui::Slider::new(&mut settings.appearance.background_opacity, 0.0..=1.0));
            ui.end_row();

            ui.label(t("settings.appearance.sidebar_width_label"));
            ui.add(egui::DragValue::new(&mut settings.appearance.sidebar_width)
                .range(100.0..=400.0)
                .speed(1.0));
            ui.end_row();

            ui.label("UI Scale");
            egui::ComboBox::from_id_salt("ui_scale")
                .selected_text(match settings.appearance.ui_scale.as_str() {
                    "small" => "Small",
                    "large" => "Large",
                    _ => "Medium",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut settings.appearance.ui_scale, "small".to_string(), "Small");
                    ui.selectable_value(&mut settings.appearance.ui_scale, "medium".to_string(), "Medium");
                    ui.selectable_value(&mut settings.appearance.ui_scale, "large".to_string(), "Large");
                });
            ui.end_row();
        });
}

pub fn draw_clipboard_tab(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.add_space(8.0);
    ui.heading(t("settings.clipboard.heading"));
    ui.add_space(4.0);

    ui.checkbox(&mut settings.clipboard.macos_style, t("settings.clipboard.macos_style"));
    ui.checkbox(&mut settings.clipboard.linux_style, t("settings.clipboard.linux_style"));
    ui.checkbox(&mut settings.clipboard.windows_style, t("settings.clipboard.windows_style"));

    ui.add_space(12.0);
    ui.heading(t("settings.zoom.heading"));
    ui.add_space(4.0);

    ui.checkbox(&mut settings.zoom.ctrl_style, t("settings.zoom.ctrl_style"));
    ui.checkbox(&mut settings.zoom.alt_style, t("settings.zoom.alt_style"));
}

pub fn draw_notifications_tab(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.add_space(8.0);
    ui.heading(t("settings.notifications.heading"));
    ui.add_space(4.0);

    ui.checkbox(&mut settings.notification.enabled, t("settings.notifications.enabled"));
    ui.checkbox(&mut settings.notification.system_notification, t("settings.notifications.system_notification"));
    ui.checkbox(&mut settings.notification.sound, t("settings.notifications.sound"));

    ui.add_space(8.0);
    egui::Grid::new("notification_grid")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label(t("settings.notifications.coalesce_interval_label"));
            ui.add(egui::DragValue::new(&mut settings.notification.coalesce_ms)
                .range(0..=5000)
                .speed(50));
            ui.end_row();
        });
}

pub fn draw_language_tab(ui: &mut egui::Ui, settings: &mut Settings) {
    let th = crate::theme::theme();
    ui.add_space(8.0);
    ui.heading(t("settings.language.heading"));
    ui.add_space(4.0);

    egui::Grid::new("language_grid")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label(t("settings.language.label"));
            egui::ComboBox::from_id_salt("language_select")
                .selected_text(language_display_name(&settings.general.language))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut settings.general.language, "en".to_string(), "English");
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

pub fn draw_performance_tab(ui: &mut egui::Ui, settings: &mut Settings) {
    let th = crate::theme::theme();
    ui.heading("Performance");
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new("Changes require restart to take effect.")
            .small()
            .color(th.yellow),
    );
    ui.add_space(12.0);

    ui.checkbox(
        &mut settings.performance.targeted_pty_polling,
        "Targeted PTY polling",
    );
    ui.label(
        egui::RichText::new("Only process terminals with new output instead of polling all. Reduces CPU with many surfaces.")
            .small()
            .color(egui::Color32::GRAY),
    );
    ui.add_space(8.0);

    ui.checkbox(
        &mut settings.performance.scrollback_disk_swap,
        "Scrollback disk swap",
    );
    ui.label(
        egui::RichText::new("Swap old scrollback lines to disk to reduce memory usage.")
            .small()
            .color(egui::Color32::GRAY),
    );
    ui.add_space(8.0);

    ui.checkbox(
        &mut settings.performance.lazy_pty_init,
        "Lazy PTY initialization",
    );
    ui.label(
        egui::RichText::new("Spawn shell processes only when a tab is first focused, not at creation.")
            .small()
            .color(egui::Color32::GRAY),
    );
}
