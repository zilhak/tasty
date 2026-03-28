use crate::i18n::t;
use crate::settings::{GeneralSettings, Settings};

/// Active tab in the settings window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingsTab {
    General,
    Appearance,
    Clipboard,
    Notifications,
    Keybindings,
    Language,
    Performance,
}

/// Sub-tab within the Keybindings tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeybindingsSubTab {
    General,
    Workspace,
    Pane,
    Surface,
}

/// Persistent state for the settings UI between frames.
pub struct SettingsUiState {
    active_tab: SettingsTab,
    /// Working copy of settings being edited.
    draft: Option<Settings>,
    /// Which keybinding field is currently recording input (None = not recording).
    recording_field: Option<String>,
    /// Active sub-tab within keybindings.
    keybindings_sub_tab: KeybindingsSubTab,
}

impl SettingsUiState {
    pub fn new() -> Self {
        Self {
            active_tab: SettingsTab::General,
            draft: None,
            recording_field: None,
            keybindings_sub_tab: KeybindingsSubTab::General,
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

    // Modal overlay: covers entire screen, blocks clicks from reaching panes behind
    let screen_rect = ctx.screen_rect();
    egui::Area::new(egui::Id::new("settings_modal_overlay"))
        .order(egui::Order::Middle)
        .fixed_pos(screen_rect.min)
        .show(ctx, |ui| {
            let response = ui.allocate_rect(screen_rect, egui::Sense::click());
            ui.painter().rect_filled(
                screen_rect,
                0.0,
                egui::Color32::from_black_alpha(120),
            );
            let _ = response;
        });

    let center = screen_rect.center();
    let window_size = egui::vec2(520.0, 420.0);
    let default_pos = egui::pos2(
        center.x - window_size.x / 2.0,
        center.y - window_size.y / 2.0,
    );

    egui::Window::new(t("settings.window.title"))
        .open(&mut is_open)
        .fixed_size(window_size)
        .collapsible(false)
        .default_pos(default_pos)
        .movable(true)
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
                    (SettingsTab::Performance, "Performance"),
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
                let mut draft = ui_state.draft.take().unwrap();
                let active_tab = ui_state.active_tab;

                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        match active_tab {
                            SettingsTab::General => draw_general_tab(ui, &mut draft),
                            SettingsTab::Appearance => draw_appearance_tab(ui, &mut draft),
                            SettingsTab::Clipboard => draw_clipboard_tab(ui, &mut draft),
                            SettingsTab::Notifications => draw_notifications_tab(ui, &mut draft),
                            SettingsTab::Keybindings => draw_keybindings_tab(ui, &mut draft, &mut ui_state.recording_field, &mut ui_state.keybindings_sub_tab),
                            SettingsTab::Language => draw_language_tab(ui, &mut draft),
                            SettingsTab::Performance => draw_performance_tab(ui, &mut draft),
                        }
                    });

                ui_state.draft = Some(draft);
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
                ui_state.recording_field = None;
                *open = false;
            }
            if do_cancel {
                ui_state.draft = None;
                ui_state.recording_field = None;
                *open = false;
            }
        });

    if !is_open {
        // Window was closed via X button
        ui_state.draft = None;
        ui_state.recording_field = None;
        *open = false;
    }
}

fn draw_general_tab(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.add_space(8.0);
    ui.heading(t("settings.general.heading"));
    ui.add_space(4.0);

    // Show warning if shell is not valid
    if !settings.general.is_shell_valid() {
        ui.label(
            egui::RichText::new(t("settings.general.shell_not_found"))
                .color(egui::Color32::from_rgb(220, 160, 60)),
        );
        ui.add_space(4.0);
    }

    egui::Grid::new("general_grid")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label(t("settings.general.shell_label"));
            // Auto-detected bash path as hint, plus manual text input
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

fn draw_keybindings_tab(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    recording_field: &mut Option<String>,
    sub_tab: &mut KeybindingsSubTab,
) {
    ui.add_space(8.0);
    ui.heading(t("settings.keybindings.heading"));
    ui.add_space(4.0);

    // Sub-tab layout: left menu + right content
    let available_height = ui.available_height() - 8.0;

    ui.horizontal_top(|ui| {
        // Left menu with bordered frame, full height
        egui::Frame::new()
            .fill(egui::Color32::from_rgb(18, 18, 22))
            .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(60, 60, 70)))
            .corner_radius(4.0)
            .inner_margin(egui::Margin::symmetric(6, 6))
            .show(ui, |ui| {
                ui.set_width(88.0);
                ui.set_min_height(available_height);

                ui.vertical(|ui| {
                    let sub_tabs = [
                        (KeybindingsSubTab::General, t("settings.keybindings.subtab.general")),
                        (KeybindingsSubTab::Workspace, t("settings.keybindings.subtab.workspace")),
                        (KeybindingsSubTab::Pane, t("settings.keybindings.subtab.pane")),
                        (KeybindingsSubTab::Surface, t("settings.keybindings.subtab.surface")),
                    ];

                    for (tab, label) in &sub_tabs {
                        let selected = *sub_tab == *tab;
                        if ui.selectable_label(selected, *label).clicked() {
                            *sub_tab = *tab;
                            *recording_field = None;
                        }
                    }
                });
            });

        ui.add_space(8.0);

        // Right content area (vertical layout)
        ui.vertical(|ui| {

        // If recording, capture key events from egui input
        let captured = capture_key_combo(ui.ctx(), recording_field.is_some());

        match *sub_tab {
            KeybindingsSubTab::General => {
                draw_keybinding_entries(ui, recording_field, &captured, &mut [
                    ("toggle_settings", "settings.keybindings.toggle_settings_label", &mut settings.keybindings.toggle_settings),
                    ("toggle_notifications", "settings.keybindings.toggle_notifications_label", &mut settings.keybindings.toggle_notifications),
                ]);
            }
            KeybindingsSubTab::Workspace => {
                draw_keybinding_entries(ui, recording_field, &captured, &mut [
                    ("new_workspace", "settings.keybindings.new_workspace_label", &mut settings.keybindings.new_workspace),
                ]);

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);

                // Tab switch modifier: ComboBox
                egui::Grid::new("tab_ws_modifier_grid")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label(t("settings.keybindings.tab_switch_modifier_label"));
                        egui::ComboBox::from_id_salt("tab_switch_modifier")
                            .selected_text(modifier_display(&settings.keybindings.tab_switch_modifier))
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut settings.keybindings.tab_switch_modifier, "ctrl".to_string(), "Ctrl");
                                ui.selectable_value(&mut settings.keybindings.tab_switch_modifier, "alt".to_string(), "Alt");
                            });
                        ui.end_row();

                        ui.label(t("settings.keybindings.workspace_switch_modifier_label"));
                        egui::ComboBox::from_id_salt("workspace_switch_modifier")
                            .selected_text(modifier_display(&settings.keybindings.workspace_switch_modifier))
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut settings.keybindings.workspace_switch_modifier, "ctrl".to_string(), "Ctrl");
                                ui.selectable_value(&mut settings.keybindings.workspace_switch_modifier, "alt".to_string(), "Alt");
                            });
                        ui.end_row();
                    });
            }
            KeybindingsSubTab::Pane => {
                draw_keybinding_entries(ui, recording_field, &captured, &mut [
                    ("new_tab", "settings.keybindings.new_tab_label", &mut settings.keybindings.new_tab),
                    ("split_pane_vertical", "settings.keybindings.split_pane_vertical_label", &mut settings.keybindings.split_pane_vertical),
                    ("split_pane_horizontal", "settings.keybindings.split_pane_horizontal_label", &mut settings.keybindings.split_pane_horizontal),
                    ("focus_pane_next", "settings.keybindings.focus_pane_next_label", &mut settings.keybindings.focus_pane_next),
                    ("focus_pane_prev", "settings.keybindings.focus_pane_prev_label", &mut settings.keybindings.focus_pane_prev),
                    ("close_pane", "settings.keybindings.close_pane_label", &mut settings.keybindings.close_pane),
                ]);
            }
            KeybindingsSubTab::Surface => {
                draw_keybinding_entries(ui, recording_field, &captured, &mut [
                    ("split_surface_vertical", "settings.keybindings.split_surface_vertical_label", &mut settings.keybindings.split_surface_vertical),
                    ("split_surface_horizontal", "settings.keybindings.split_surface_horizontal_label", &mut settings.keybindings.split_surface_horizontal),
                    ("focus_surface_next", "settings.keybindings.focus_surface_next_label", &mut settings.keybindings.focus_surface_next),
                    ("focus_surface_prev", "settings.keybindings.focus_surface_prev_label", &mut settings.keybindings.focus_surface_prev),
                    ("close_surface", "settings.keybindings.close_surface_label", &mut settings.keybindings.close_surface),
                ]);
            }
        }

        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(t("settings.keybindings.hint_esc_to_clear"))
                .small()
                .color(egui::Color32::from_rgb(150, 150, 170)),
        );

        }); // end vertical
    }); // end horizontal_top
}

/// Display modifier name for ComboBox.
fn modifier_display(modifier: &str) -> &str {
    match modifier.to_lowercase().as_str() {
        "alt" => "Alt",
        _ => "Ctrl",
    }
}

/// Draw a list of keybinding capture entries in a grid.
fn draw_keybinding_entries(
    ui: &mut egui::Ui,
    recording_field: &mut Option<String>,
    captured: &KeyCapture,
    bindings: &mut [(&str, &str, &mut String)],
) {
    egui::Grid::new("keybindings_grid")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            for (field_id, label_key, value) in bindings.iter_mut() {
                ui.label(t(label_key));

                let is_recording = recording_field.as_deref() == Some(*field_id);

                if is_recording {
                    match captured {
                        KeyCapture::Combo(combo) => {
                            **value = combo.clone();
                            *recording_field = None;
                        }
                        KeyCapture::Clear => {
                            value.clear();
                            *recording_field = None;
                        }
                        KeyCapture::None => {}
                    }
                }

                let display_text = if is_recording {
                    t("settings.keybindings.hint_press_key").to_string()
                } else if value.is_empty() {
                    t("settings.keybindings.hint_none").to_string()
                } else {
                    (**value).clone()
                };

                let bg_color = if is_recording {
                    egui::Color32::from_rgb(60, 80, 120)
                } else {
                    egui::Color32::from_rgb(45, 45, 55)
                };
                let text_color = if is_recording || value.is_empty() {
                    egui::Color32::from_rgb(160, 160, 180)
                } else {
                    egui::Color32::from_rgb(220, 220, 230)
                };

                let button = egui::Button::new(
                    egui::RichText::new(&display_text).color(text_color).monospace()
                )
                    .fill(bg_color)
                    .min_size(egui::vec2(200.0, 24.0));

                if ui.add(button).clicked() {
                    *recording_field = Some(field_id.to_string());
                }
                ui.end_row();
            }
        });
}

/// Result of key capture attempt.
enum KeyCapture {
    /// No key pressed yet.
    None,
    /// User pressed Escape — clear the binding.
    Clear,
    /// A valid key combination was captured.
    Combo(String),
}

/// Read egui input events and build a key combo string like "ctrl+shift+n".
fn capture_key_combo(ctx: &egui::Context, active: bool) -> KeyCapture {
    if !active {
        return KeyCapture::None;
    }

    ctx.input(|input| {
        for event in &input.events {
            if let egui::Event::Key { key, pressed, modifiers, .. } = event {
                if !pressed {
                    continue;
                }

                // Escape clears the binding
                if *key == egui::Key::Escape {
                    return KeyCapture::Clear;
                }

                // Ignore modifier-only keys
                if matches!(key,
                    egui::Key::Tab // allow Tab as a valid key
                    ) || !is_modifier_only_key(key)
                {
                    let mut parts = Vec::new();
                    if modifiers.ctrl {
                        parts.push("ctrl");
                    }
                    if modifiers.alt {
                        parts.push("alt");
                    }
                    if modifiers.shift {
                        parts.push("shift");
                    }

                    let key_name = egui_key_to_string(key);
                    if !key_name.is_empty() {
                        parts.push(&key_name);
                        return KeyCapture::Combo(parts.join("+"));
                    }
                }
            }
        }
        KeyCapture::None
    })
}

/// Returns true if the key is a modifier key only (no actual key).
fn is_modifier_only_key(_key: &egui::Key) -> bool {
    false
}

/// Convert an egui::Key to a lowercase string representation.
fn egui_key_to_string(key: &egui::Key) -> String {
    match key {
        egui::Key::A => "a".into(),
        egui::Key::B => "b".into(),
        egui::Key::C => "c".into(),
        egui::Key::D => "d".into(),
        egui::Key::E => "e".into(),
        egui::Key::F => "f".into(),
        egui::Key::G => "g".into(),
        egui::Key::H => "h".into(),
        egui::Key::I => "i".into(),
        egui::Key::J => "j".into(),
        egui::Key::K => "k".into(),
        egui::Key::L => "l".into(),
        egui::Key::M => "m".into(),
        egui::Key::N => "n".into(),
        egui::Key::O => "o".into(),
        egui::Key::P => "p".into(),
        egui::Key::Q => "q".into(),
        egui::Key::R => "r".into(),
        egui::Key::S => "s".into(),
        egui::Key::T => "t".into(),
        egui::Key::U => "u".into(),
        egui::Key::V => "v".into(),
        egui::Key::W => "w".into(),
        egui::Key::X => "x".into(),
        egui::Key::Y => "y".into(),
        egui::Key::Z => "z".into(),
        egui::Key::Num0 => "0".into(),
        egui::Key::Num1 => "1".into(),
        egui::Key::Num2 => "2".into(),
        egui::Key::Num3 => "3".into(),
        egui::Key::Num4 => "4".into(),
        egui::Key::Num5 => "5".into(),
        egui::Key::Num6 => "6".into(),
        egui::Key::Num7 => "7".into(),
        egui::Key::Num8 => "8".into(),
        egui::Key::Num9 => "9".into(),
        egui::Key::Tab => "tab".into(),
        egui::Key::Space => "space".into(),
        egui::Key::Enter => "enter".into(),
        egui::Key::Backspace => "backspace".into(),
        egui::Key::Delete => "delete".into(),
        egui::Key::Insert => "insert".into(),
        egui::Key::Home => "home".into(),
        egui::Key::End => "end".into(),
        egui::Key::PageUp => "pageup".into(),
        egui::Key::PageDown => "pagedown".into(),
        egui::Key::ArrowUp => "up".into(),
        egui::Key::ArrowDown => "down".into(),
        egui::Key::ArrowLeft => "left".into(),
        egui::Key::ArrowRight => "right".into(),
        egui::Key::F1 => "f1".into(),
        egui::Key::F2 => "f2".into(),
        egui::Key::F3 => "f3".into(),
        egui::Key::F4 => "f4".into(),
        egui::Key::F5 => "f5".into(),
        egui::Key::F6 => "f6".into(),
        egui::Key::F7 => "f7".into(),
        egui::Key::F8 => "f8".into(),
        egui::Key::F9 => "f9".into(),
        egui::Key::F10 => "f10".into(),
        egui::Key::F11 => "f11".into(),
        egui::Key::F12 => "f12".into(),
        egui::Key::Minus => "minus".into(),
        egui::Key::Plus => "plus".into(),
        egui::Key::Comma => ",".into(),
        egui::Key::Period => ".".into(),
        egui::Key::Semicolon => ";".into(),
        egui::Key::Colon => ":".into(),
        egui::Key::Pipe => "|".into(),
        egui::Key::Questionmark => "?".into(),
        egui::Key::OpenBracket => "[".into(),
        egui::Key::CloseBracket => "]".into(),
        egui::Key::Backslash => "\\".into(),
        egui::Key::Backtick => "`".into(),
        egui::Key::Equals => "=".into(),
        _ => String::new(),
    }
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

fn draw_performance_tab(ui: &mut egui::Ui, settings: &mut crate::settings::Settings) {
    ui.heading("Performance");
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new("Changes require restart to take effect.")
            .small()
            .color(egui::Color32::from_rgb(249, 226, 175)), // Yellow warning
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
