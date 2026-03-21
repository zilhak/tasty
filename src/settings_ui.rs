use crate::settings::Settings;

/// Active tab in the settings window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingsTab {
    General,
    Appearance,
    Clipboard,
    Notifications,
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

    egui::Window::new("Settings")
        .open(&mut is_open)
        .default_width(520.0)
        .default_height(420.0)
        .resizable(true)
        .collapsible(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            // Tab bar
            ui.horizontal(|ui| {
                let tabs = [
                    (SettingsTab::General, "General"),
                    (SettingsTab::Appearance, "Appearance"),
                    (SettingsTab::Clipboard, "Clipboard"),
                    (SettingsTab::Notifications, "Notifications"),
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
                        }
                    });
            }

            ui.separator();

            // Action buttons - collect actions first, apply after
            let mut do_save = false;
            let mut do_cancel = false;

            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Cancel").clicked() {
                        do_cancel = true;
                    }
                    if ui.button("Save").clicked() {
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
    ui.heading("General");
    ui.add_space(4.0);

    egui::Grid::new("general_grid")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label("Shell:");
            ui.text_edit_singleline(&mut settings.general.shell);
            ui.end_row();

            ui.label("Startup command:");
            ui.text_edit_singleline(&mut settings.general.startup_command);
            ui.end_row();
        });
}

fn draw_appearance_tab(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.add_space(8.0);
    ui.heading("Appearance");
    ui.add_space(4.0);

    egui::Grid::new("appearance_grid")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label("Font family:");
            ui.text_edit_singleline(&mut settings.appearance.font_family);
            ui.end_row();

            ui.label("Font size:");
            ui.add(egui::DragValue::new(&mut settings.appearance.font_size)
                .range(6.0..=72.0)
                .speed(0.5));
            ui.end_row();

            ui.label("Theme:");
            ui.horizontal(|ui| {
                ui.radio_value(&mut settings.appearance.theme, "dark".to_string(), "Dark");
                ui.radio_value(&mut settings.appearance.theme, "light".to_string(), "Light");
            });
            ui.end_row();

            ui.label("Background opacity:");
            ui.add(egui::Slider::new(&mut settings.appearance.background_opacity, 0.0..=1.0));
            ui.end_row();

            ui.label("Sidebar width:");
            ui.add(egui::DragValue::new(&mut settings.appearance.sidebar_width)
                .range(100.0..=400.0)
                .speed(1.0));
            ui.end_row();
        });
}

fn draw_clipboard_tab(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.add_space(8.0);
    ui.heading("Clipboard");
    ui.add_space(4.0);

    ui.checkbox(&mut settings.clipboard.macos_style, "macOS style (Alt+C / Alt+V)");
    ui.checkbox(&mut settings.clipboard.linux_style, "Linux style (Ctrl+Shift+C / Ctrl+Shift+V)");
    ui.checkbox(&mut settings.clipboard.windows_style, "Windows style (Ctrl+C with selection / Ctrl+V)");
}

fn draw_notifications_tab(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.add_space(8.0);
    ui.heading("Notifications");
    ui.add_space(4.0);

    ui.checkbox(&mut settings.notification.enabled, "Notifications enabled");
    ui.checkbox(&mut settings.notification.system_notification, "System notifications (OS native)");
    ui.checkbox(&mut settings.notification.sound, "Sound");

    ui.add_space(8.0);
    egui::Grid::new("notification_grid")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label("Coalesce interval (ms):");
            ui.add(egui::DragValue::new(&mut settings.notification.coalesce_ms)
                .range(0..=5000)
                .speed(50));
            ui.end_row();
        });
}
