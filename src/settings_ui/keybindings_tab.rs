use crate::i18n::t;
use crate::settings::Settings;

/// Sub-tab within the Keybindings tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeybindingsSubTab {
    General,
    Workspace,
    Pane,
    Surface,
    Preset,
}

/// Result of key capture attempt.
pub enum KeyCapture {
    /// No key pressed yet.
    None,
    /// User pressed Escape — clear the binding.
    Clear,
    /// A valid key combination was captured.
    Combo(String),
}

#[allow(clippy::too_many_arguments)]
pub fn draw_keybindings_tab(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    recording_field: &mut Option<String>,
    sub_tab: &mut KeybindingsSubTab,
    preset_confirm: &mut Option<String>,
) {
    let th = crate::theme::theme();
    ui.add_space(8.0);
    ui.heading(t("settings.keybindings.heading"));
    ui.add_space(4.0);

    let available_height = ui.available_height() - 8.0;

    ui.horizontal_top(|ui| {
        egui::Frame::new()
            .fill(th.crust)
            .stroke(egui::Stroke::new(1.0, th.surface0))
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
                        (KeybindingsSubTab::Preset, "Preset"),
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

        ui.vertical(|ui| {

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
                    ("close_workspace", "settings.keybindings.close_workspace_label", &mut settings.keybindings.close_workspace),
                ]);

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);

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
            KeybindingsSubTab::Preset => {
                ui.add_space(4.0);
                ui.label("Select a preset to overwrite all keybindings:");
                ui.add_space(8.0);

                for name in crate::settings::KeybindingSettings::preset_names() {
                    if ui.button(*name).clicked() {
                        *preset_confirm = Some(name.to_string());
                    }
                }
            }
        }

        if *sub_tab != KeybindingsSubTab::Preset {
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(t("settings.keybindings.hint_esc_to_clear"))
                    .small()
                    .color(th.overlay1),
            );
        }

        }); // end vertical
    }); // end horizontal_top

    // Preset confirmation modal
    if let Some(name) = preset_confirm.clone() {
        egui::Window::new("Apply Preset")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ui.ctx(), |ui| {
                ui.label(format!(
                    "Are you sure you want to apply the \"{}\" preset?\nThis will overwrite all current keybindings.",
                    name
                ));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        *preset_confirm = None;
                    }
                    if ui.button("Apply").clicked() {
                        settings.keybindings.apply_preset(&name);
                        *preset_confirm = None;
                    }
                });
            });
    }
}

fn modifier_display(modifier: &str) -> &str {
    match modifier.to_lowercase().as_str() {
        "alt" => "Alt",
        _ => "Ctrl",
    }
}

fn draw_keybinding_entries(
    ui: &mut egui::Ui,
    recording_field: &mut Option<String>,
    captured: &KeyCapture,
    bindings: &mut [(&str, &str, &mut String)],
) {
    let th = crate::theme::theme();
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
                    th.surface1
                } else {
                    th.surface0
                };
                let text_color = if is_recording || value.is_empty() {
                    th.overlay1
                } else {
                    th.text
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

                if *key == egui::Key::Escape {
                    return KeyCapture::Clear;
                }

                if is_modifier_only_key(key) {
                    continue;
                }

                {
                    let mut parts = Vec::new();
                    if modifiers.ctrl {
                        parts.push("ctrl");
                    }
                    #[cfg(target_os = "macos")]
                    if modifiers.mac_cmd {
                        parts.push("alt");
                    }
                    #[cfg(not(target_os = "macos"))]
                    if modifiers.alt {
                        parts.push("alt");
                    }
                    if modifiers.shift {
                        parts.push("shift");
                    }

                    let key_name = egui_key_to_string(key);
                    if key_name.is_empty() {
                        continue;
                    }

                    let is_typing_key = matches!(key,
                        egui::Key::A | egui::Key::B | egui::Key::C | egui::Key::D |
                        egui::Key::E | egui::Key::F | egui::Key::G | egui::Key::H |
                        egui::Key::I | egui::Key::J | egui::Key::K | egui::Key::L |
                        egui::Key::M | egui::Key::N | egui::Key::O | egui::Key::P |
                        egui::Key::Q | egui::Key::R | egui::Key::S | egui::Key::T |
                        egui::Key::U | egui::Key::V | egui::Key::W | egui::Key::X |
                        egui::Key::Y | egui::Key::Z |
                        egui::Key::Num0 | egui::Key::Num1 | egui::Key::Num2 |
                        egui::Key::Num3 | egui::Key::Num4 | egui::Key::Num5 |
                        egui::Key::Num6 | egui::Key::Num7 | egui::Key::Num8 |
                        egui::Key::Num9 |
                        egui::Key::Space | egui::Key::Minus | egui::Key::Plus
                    );
                    if is_typing_key && parts.is_empty() {
                        continue;
                    }

                    parts.push(&key_name);
                    return KeyCapture::Combo(parts.join("+"));
                }
            }
        }
        KeyCapture::None
    })
}

fn is_modifier_only_key(_key: &egui::Key) -> bool {
    false
}

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
