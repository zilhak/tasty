use crate::i18n::t;
use crate::settings::Settings;

/// Active tab in the settings window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingsTab {
    General,
    Appearance,
    Clipboard,
    Notifications,
    Keybindings,
    Language,
}

/// Persistent state for the settings UI between frames.
pub struct SettingsUiState {
    active_tab: SettingsTab,
    /// Working copy of settings being edited.
    draft: Option<Settings>,
}

impl SettingsUiState {
    pub fn new() -> Self {
        Self {
            active_tab: SettingsTab::General,
            draft: None,
        }
    }
}

/// Draw the settings window. Call every frame while `open` is true.
pub fn draw_settings_window(
    ctx: &egui::Context,
    settings: &mut Settings,
    open: &mut bool,
    ui_state: &mut SettingsUiState,
) {
    // Initialize draft on first open
    if ui_state.draft.is_none() {
        ui_state.draft = Some(settings.clone());
    }

    let mut is_open = *open;

    egui::Window::new(t("settings.window.title"))
        .open(&mut is_open)
        .fixed_size(egui::vec2(520.0, 420.0))
        .collapsible(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .interactable(true)
        .show(ctx, |ui| {
            // Tab bar
            ui.horizontal(|ui| {
                let tabs = [
                    (SettingsTab::General, t("settings.tab.general")),
                    (SettingsTab::Appearance, t("settings.tab.appearance")),
                    (SettingsTab::Clipboard, t("settings.tab.clipboard")),
                    (SettingsTab::Notifications, t("settings.tab.notifications")),
                    (SettingsTab::Keybindings, t("settings.tab.keybindings")),
                    (SettingsTab::Language, t("settings.tab.language")),
                ];
                for (tab, label) in &tabs {
                    let selected = ui_state.active_tab == *tab;
                    if ui.selectable_label(selected, *label).clicked() {
                        ui_state.active_tab = *tab;
                    }
                }
            });
            ui.separator();

            {
                let draft = ui_state.draft.as_mut().unwrap();

                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        match ui_state.active_tab {
                            SettingsTab::General => draw_general_tab(ui, draft),
                            SettingsTab::Appearance => draw_appearance_tab(ui, draft),
                            SettingsTab::Clipboard => draw_clipboard_tab(ui, draft),
                            SettingsTab::Notifications => draw_notifications_tab(ui, draft),
                            SettingsTab::Keybindings => draw_keybindings_tab(ui, draft),
                            SettingsTab::Language => draw_language_tab(ui, draft),
                        }
                    });
            }

            ui.separator();

            // Action buttons - collect actions first, apply after
            let mut do_save = false;
            let mut do_cancel = false;

            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(t("button.cancel")).clicked() {
                        do_cancel = true;
                    }
                    if ui.button(t("button.save")).clicked() {
                        do_save = true;
                    }
                });
            });

            if do_save {
                if let Some(draft) = &ui_state.draft {
                    *settings = draft.clone();
                }
                if let Err(e) = settings.save() {
                    tracing::error!("failed to save settings: {e}");
                }
                ui_state.draft = None;
                *open = false;
            }
            if do_cancel {
                ui_state.draft = None;
                *open = false;
            }
        });

    if !is_open {
        // Window was closed via X button
        ui_state.draft = None;
        *open = false;
    }
}

fn draw_general_tab(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.add_space(8.0);
    ui.heading(t("settings.general.heading"));
    ui.add_space(4.0);

    egui::Grid::new("general_grid")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label(t("settings.general.shell_label"));
            ui.text_edit_singleline(&mut settings.general.shell);
            ui.end_row();

            ui.label(t("settings.general.startup_command_label"));
            ui.text_edit_singleline(&mut settings.general.startup_command);
            ui.end_row();
        });
}

fn draw_appearance_tab(ui: &mut egui::Ui, settings: &mut Settings) {
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

            ui.label(t("settings.appearance.theme_label"));
            ui.horizontal(|ui| {
                ui.radio_value(&mut settings.appearance.theme, "dark".to_string(), t("settings.appearance.theme.dark"));
                ui.radio_value(&mut settings.appearance.theme, "light".to_string(), t("settings.appearance.theme.light"));
            });
            ui.end_row();

            ui.label(t("settings.appearance.background_opacity_label"));
            ui.add(egui::Slider::new(&mut settings.appearance.background_opacity, 0.0..=1.0));
            ui.end_row();

            ui.label(t("settings.appearance.sidebar_width_label"));
            ui.add(egui::DragValue::new(&mut settings.appearance.sidebar_width)
                .range(100.0..=400.0)
                .speed(1.0));
            ui.end_row();
        });
}

fn draw_clipboard_tab(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.add_space(8.0);
    ui.heading(t("settings.clipboard.heading"));
    ui.add_space(4.0);

    ui.checkbox(&mut settings.clipboard.macos_style, t("settings.clipboard.macos_style"));
    ui.checkbox(&mut settings.clipboard.linux_style, t("settings.clipboard.linux_style"));
    ui.checkbox(&mut settings.clipboard.windows_style, t("settings.clipboard.windows_style"));
}

fn draw_notifications_tab(ui: &mut egui::Ui, settings: &mut Settings) {
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

fn draw_keybindings_tab(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.add_space(8.0);
    ui.heading(t("settings.keybindings.heading"));
    ui.add_space(4.0);

    egui::Grid::new("keybindings_grid")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label(t("settings.keybindings.new_workspace_label"));
            ui.text_edit_singleline(&mut settings.keybindings.new_workspace);
            ui.end_row();

            ui.label(t("settings.keybindings.new_tab_label"));
            ui.text_edit_singleline(&mut settings.keybindings.new_tab);
            ui.end_row();

            ui.label(t("settings.keybindings.split_pane_vertical_label"));
            ui.text_edit_singleline(&mut settings.keybindings.split_pane_vertical);
            ui.end_row();

            ui.label(t("settings.keybindings.split_pane_horizontal_label"));
            ui.text_edit_singleline(&mut settings.keybindings.split_pane_horizontal);
            ui.end_row();

            ui.label(t("settings.keybindings.split_surface_vertical_label"));
            ui.text_edit_singleline(&mut settings.keybindings.split_surface_vertical);
            ui.end_row();

            ui.label(t("settings.keybindings.split_surface_horizontal_label"));
            ui.text_edit_singleline(&mut settings.keybindings.split_surface_horizontal);
            ui.end_row();
        });
}

fn draw_language_tab(ui: &mut egui::Ui, settings: &mut Settings) {
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
            .color(egui::Color32::from_rgb(200, 180, 100)),
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
