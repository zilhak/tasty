use crate::i18n::t;
use crate::settings::{KeybindingSettings, Settings};

/// 녹화 완료 시 발견된 단축키 충돌의 확인 대기 상태.
#[derive(Debug, Clone)]
pub struct PendingBinding {
    pub target_field: String,
    /// 교체할 (또는 새로 추가할) 대상 인덱스. len()이면 새 추가.
    pub target_idx: usize,
    pub combo: String,
    pub conflicting_field: String,
    pub conflicting_idx: usize,
}

/// Sub-tab within the Keybindings tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeybindingsSubTab {
    General,
    Workspace,
    Pane,
    Surface,
    Clipboard,
    Zoom,
    Preset,
}

/// 녹화 중인 필드 식별자 — 어떤 필드의 어느 슬롯을 기록 중인지.
#[derive(Debug, Clone)]
pub struct RecordingSlot {
    pub field_id: String,
    /// 기존 바인딩 교체 시 인덱스, 새 바인딩 추가 시 `bindings.len()`.
    pub idx: usize,
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
    recording_field: &mut Option<RecordingSlot>,
    sub_tab: &mut KeybindingsSubTab,
    selected_preset: &mut Option<String>,
    pending_binding: &mut Option<PendingBinding>,
    captured_double_tap: &mut Option<String>,
) {
    let th = crate::theme::theme();
    ui.add_space(8.0);

    let available_height = ui.available_height() - 8.0 - 14.0;

    ui.horizontal_top(|ui| {
        egui::Frame::new()
            .fill(th.crust)
            .stroke(egui::Stroke::new(1.0, th.surface0))
            .corner_radius(4.0)
            .inner_margin(egui::Margin::symmetric(6, 6))
            .show(ui, |ui| {
                ui.set_width(100.0);
                ui.set_min_height(available_height);

                ui.vertical(|ui| {
                    let sub_tabs = [
                        (KeybindingsSubTab::General, t("settings.keybindings.subtab.general")),
                        (KeybindingsSubTab::Workspace, t("settings.keybindings.subtab.workspace")),
                        (KeybindingsSubTab::Pane, t("settings.keybindings.subtab.pane")),
                        (KeybindingsSubTab::Surface, t("settings.keybindings.subtab.surface")),
                        (KeybindingsSubTab::Clipboard, t("settings.keybindings.subtab.clipboard")),
                        (KeybindingsSubTab::Zoom, t("settings.keybindings.subtab.zoom")),
                        (KeybindingsSubTab::Preset, t("settings.keybindings.subtab.preset")),
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
        ui.set_max_height(available_height);

        let mut captured = capture_key_combo(ui.ctx(), recording_field.is_some());

        // Check for double-tap modifier captured from winit events
        if recording_field.is_some() {
            if let Some(dt) = captured_double_tap.take() {
                captured = KeyCapture::Combo(dt);
            }
        }

        match *sub_tab {
            KeybindingsSubTab::General => {
                draw_keybinding_entries(ui, &mut settings.keybindings, recording_field, pending_binding, &captured, &[
                    ("toggle_settings", "settings.keybindings.toggle_settings_label"),
                    ("toggle_notifications", "settings.keybindings.toggle_notifications_label"),
                    ("toggle_clipboard_viewer", "settings.keybindings.toggle_clipboard_viewer_label"),
                    ("restore_closed", "settings.keybindings.restore_closed_label"),
                    ("new_window", "settings.keybindings.new_window_label"),
                    ("quit", "settings.keybindings.quit_label"),
                    ("quit_immediate", "settings.keybindings.quit_immediate_label"),
                    ("quit_minimize", "settings.keybindings.quit_minimize_label"),
                ]);
            }
            KeybindingsSubTab::Workspace => {
                draw_keybinding_entries(ui, &mut settings.keybindings, recording_field, pending_binding, &captured, &[
                    ("new_workspace", "settings.keybindings.new_workspace_label"),
                    ("close_workspace", "settings.keybindings.close_workspace_label"),
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
                draw_keybinding_entries(ui, &mut settings.keybindings, recording_field, pending_binding, &captured, &[
                    ("new_tab", "settings.keybindings.new_tab_label"),
                    ("next_tab", "settings.keybindings.next_tab_label"),
                    ("prev_tab", "settings.keybindings.prev_tab_label"),
                    ("split_pane_vertical", "settings.keybindings.split_pane_vertical_label"),
                    ("split_pane_horizontal", "settings.keybindings.split_pane_horizontal_label"),
                    ("focus_pane_next", "settings.keybindings.focus_pane_next_label"),
                    ("focus_pane_prev", "settings.keybindings.focus_pane_prev_label"),
                    ("close_active", "settings.keybindings.close_active_label"),
                    ("close_pane", "settings.keybindings.close_pane_label"),
                    ("open_markdown", "settings.keybindings.open_markdown_label"),
                    ("open_explorer", "settings.keybindings.open_explorer_label"),
                ]);
            }
            KeybindingsSubTab::Surface => {
                draw_keybinding_entries(ui, &mut settings.keybindings, recording_field, pending_binding, &captured, &[
                    ("split_surface_vertical", "settings.keybindings.split_surface_vertical_label"),
                    ("split_surface_horizontal", "settings.keybindings.split_surface_horizontal_label"),
                    ("focus_surface_next", "settings.keybindings.focus_surface_next_label"),
                    ("focus_surface_prev", "settings.keybindings.focus_surface_prev_label"),
                    ("close_surface", "settings.keybindings.close_surface_label"),
                    ("convert_surface", "settings.keybindings.convert_surface_label"),
                    ("convert_to_markdown", "settings.keybindings.convert_to_markdown_label"),
                    ("convert_to_explorer", "settings.keybindings.convert_to_explorer_label"),
                ]);
            }
            KeybindingsSubTab::Clipboard => {
                draw_keybinding_entries(ui, &mut settings.keybindings, recording_field, pending_binding, &captured, &[
                    ("copy", "settings.keybindings.copy_label"),
                    ("paste", "settings.keybindings.paste_label"),
                ]);
            }
            KeybindingsSubTab::Zoom => {
                draw_keybinding_entries(ui, &mut settings.keybindings, recording_field, pending_binding, &captured, &[
                    ("zoom_in", "settings.keybindings.zoom_in_label"),
                    ("zoom_out", "settings.keybindings.zoom_out_label"),
                    ("zoom_reset", "settings.keybindings.zoom_reset_label"),
                ]);
            }
            KeybindingsSubTab::Preset => {
                draw_preset_subtab(ui, &mut settings.keybindings, selected_preset);
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

    // Keybinding conflict confirmation modal
    if let Some(pending) = pending_binding.clone() {
        let conflict_label_raw = KeybindingSettings::label_key_for(&pending.conflicting_field)
            .map(t)
            .unwrap_or(pending.conflicting_field.as_str());
        let conflict_label = conflict_label_raw
            .trim_end_matches(':')
            .trim()
            .to_string();
        let combo_display = KeybindingSettings::format_display(&pending.combo);

        let mut accept = false;
        let mut cancel = false;
        ui.ctx().input(|i| {
            if i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Y) {
                accept = true;
            }
            if i.key_pressed(egui::Key::Escape) || i.key_pressed(egui::Key::N) {
                cancel = true;
            }
        });

        egui::Window::new(t("settings.keybindings.conflict_title"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ui.ctx(), |ui| {
                ui.label(crate::i18n::t_fmt2(
                    "settings.keybindings.conflict_message",
                    &combo_display,
                    &conflict_label,
                ));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button(t("button.cancel")).clicked() {
                        cancel = true;
                    }
                    if ui.button(t("settings.keybindings.conflict_apply")).clicked() {
                        accept = true;
                    }
                });
            });

        if accept {
            // 충돌하는 쪽의 해당 바인딩만 제거 후, 대상 필드에 새 바인딩을 기록한다.
            settings.keybindings.remove_binding(&pending.conflicting_field, pending.conflicting_idx);
            settings.keybindings.replace_binding_at(
                &pending.target_field,
                pending.target_idx,
                pending.combo.clone(),
            );
            *pending_binding = None;
        } else if cancel {
            *pending_binding = None;
        }
    }
}

fn modifier_display(modifier: &str) -> &str {
    match modifier.to_lowercase().as_str() {
        "alt" => "Alt",
        _ => "Ctrl",
    }
}

/// Preset 서브탭: 좌측 프리셋 목록, 우측 미리보기 테이블 + 적용 버튼.
fn draw_preset_subtab(
    ui: &mut egui::Ui,
    keybindings: &mut KeybindingSettings,
    selected_preset: &mut Option<String>,
) {
    let th = crate::theme::theme();
    ui.add_space(4.0);

    ui.horizontal_top(|ui| {
        // 좌측: 프리셋 목록
        egui::Frame::new()
            .fill(th.mantle)
            .stroke(egui::Stroke::new(1.0, th.surface0))
            .corner_radius(4.0)
            .inner_margin(egui::Margin::symmetric(8, 8))
            .show(ui, |ui| {
                ui.set_width(120.0);
                ui.vertical(|ui| {
                    for name in KeybindingSettings::preset_names() {
                        let is_selected = selected_preset.as_deref() == Some(*name);
                        if ui.selectable_label(is_selected, *name).clicked() {
                            *selected_preset = Some(name.to_string());
                        }
                    }
                });
            });

        ui.add_space(8.0);

        // 우측: 미리보기 패널
        ui.vertical(|ui| {
            let Some(name) = selected_preset.clone() else {
                ui.label(t("settings.keybindings.select_preset_label"));
                return;
            };
            let Some(preset) = KeybindingSettings::preset_by_name(&name) else {
                ui.label(t("settings.keybindings.select_preset_label"));
                return;
            };

            ui.heading(&name);
            ui.add_space(4.0);

            let is_identical = KeybindingSettings::GENERAL_BINDING_FIELDS
                .iter()
                .all(|(id, _)| keybindings.get_bindings(id) == preset.get_bindings(id));

            egui::ScrollArea::vertical()
                .max_height(ui.available_height() - 40.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    egui::Grid::new("preset_preview_grid")
                        .num_columns(3)
                        .spacing([16.0, 6.0])
                        .striped(true)
                        .show(ui, |ui| {
                            let strong = |s: String| {
                                egui::RichText::new(s).color(th.text).strong()
                            };
                            let normal = |s: String| egui::RichText::new(s).color(th.text);

                            ui.label(strong(t("settings.keybindings.preset_col_action").to_string()));
                            ui.label(strong(t("settings.keybindings.preset_col_before").to_string()));
                            ui.label(strong(t("settings.keybindings.preset_col_after").to_string()));
                            ui.end_row();

                            for (field_id, label_key) in KeybindingSettings::GENERAL_BINDING_FIELDS {
                                let before_raw = keybindings.get_bindings(field_id).unwrap_or(&[]);
                                let after_raw = preset.get_bindings(field_id).unwrap_or(&[]);
                                let changed = before_raw != after_raw;

                                let action_label = t(label_key)
                                    .trim_end_matches(':')
                                    .trim()
                                    .to_string();
                                let fmt_list = |v: &[String]| -> String {
                                    if v.is_empty() {
                                        t("settings.keybindings.hint_none").to_string()
                                    } else {
                                        v.iter()
                                            .map(|b| KeybindingSettings::format_display(b))
                                            .collect::<Vec<_>>()
                                            .join(", ")
                                    }
                                };

                                let make = |s: String| if changed { strong(s) } else { normal(s) };
                                ui.label(make(action_label));
                                ui.label(make(fmt_list(before_raw)));
                                ui.label(make(fmt_list(after_raw)));
                                ui.end_row();
                            }
                        });
                });

            ui.add_space(4.0);
            ui.separator();
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let apply_btn = egui::Button::new(t("settings.keybindings.apply_button"));
                if ui.add_enabled(!is_identical, apply_btn).clicked() {
                    keybindings.apply_preset(&name);
                }
            });
        });
    });
}

fn draw_keybinding_entries(
    ui: &mut egui::Ui,
    keybindings: &mut KeybindingSettings,
    recording_field: &mut Option<RecordingSlot>,
    pending_binding: &mut Option<PendingBinding>,
    captured: &KeyCapture,
    entries: &[(&str, &str)],
) {
    let th = crate::theme::theme();
    // 충돌 팝업이 떠 있는 동안은 녹화 버튼을 눌러도 녹화 상태로 진입하지 않도록 가드.
    let can_record = pending_binding.is_none();

    // 녹화된 combo 처리: 녹화 슬롯이 정해져 있을 때만 적용.
    if let Some(slot) = recording_field.clone() {
        match captured {
            KeyCapture::Combo(combo) => {
                match keybindings.find_conflict(&slot.field_id, combo) {
                    Some((conflicting, conflicting_idx)) => {
                        *pending_binding = Some(PendingBinding {
                            target_field: slot.field_id.clone(),
                            target_idx: slot.idx,
                            combo: combo.clone(),
                            conflicting_field: conflicting.to_string(),
                            conflicting_idx,
                        });
                    }
                    None => {
                        keybindings.replace_binding_at(&slot.field_id, slot.idx, combo.clone());
                    }
                }
                *recording_field = None;
            }
            KeyCapture::Clear => {
                // Escape — 녹화 중인 슬롯이 기존 엔트리면 제거, 새 슬롯이면 그냥 취소.
                let current_len = keybindings
                    .get_bindings(&slot.field_id)
                    .map(|v| v.len())
                    .unwrap_or(0);
                if slot.idx < current_len {
                    keybindings.remove_binding(&slot.field_id, slot.idx);
                }
                *recording_field = None;
            }
            KeyCapture::None => {}
        }
    }

    // 버튼/간격 치수. 4px 그리드 준수.
    const BUTTON_HEIGHT: f32 = 24.0;
    const BUTTON_WIDTH: f32 = 140.0;
    const ADD_BUTTON_WIDTH: f32 = 32.0;
    const LABEL_GAP: f32 = 12.0;
    const ROW_GAP: f32 = 4.0;

    // 라벨 컬럼 폭을 이 탭에 표시되는 모든 엔트리 중 가장 긴 라벨 기준으로 계산.
    // 라벨 영역과 버튼 영역이 명확히 분리되고 모든 행에서 정렬되도록 한다.
    let label_col_width = {
        let font_id = egui::TextStyle::Body.resolve(ui.style());
        entries
            .iter()
            .map(|(_, label_key)| {
                let text = t(label_key).to_string();
                ui.ctx().fonts(|f| {
                    f.layout_no_wrap(text, font_id.clone(), egui::Color32::WHITE)
                        .size()
                        .x
                })
            })
            .fold(0.0_f32, f32::max)
    };

    for (field_id, label_key) in entries.iter() {
        ui.horizontal_top(|ui| {
            // 라벨 컬럼: 고정 폭, 우측 정렬(콜론이 항상 버튼 영역 바로 앞).
            ui.allocate_ui_with_layout(
                egui::vec2(label_col_width, BUTTON_HEIGHT),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    ui.label(t(label_key));
                },
            );
            ui.add_space(LABEL_GAP);

            // 버튼 영역: 남은 폭을 모두 사용. 폭을 초과하면 자동 줄바꿈.
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(ROW_GAP, ROW_GAP);

                let bindings_len = keybindings
                    .get_bindings(field_id)
                    .map(|v| v.len())
                    .unwrap_or(0);

                // 기존 바인딩 각각을 버튼으로 표시.
                for idx in 0..bindings_len {
                    let is_recording = matches!(
                        recording_field,
                        Some(slot) if slot.field_id == *field_id && slot.idx == idx
                    );
                    let current = keybindings
                        .get_bindings(field_id)
                        .and_then(|v| v.get(idx))
                        .cloned()
                        .unwrap_or_default();

                    let display_text = if is_recording {
                        t("settings.keybindings.hint_press_key").to_string()
                    } else {
                        KeybindingSettings::format_display(&current)
                    };

                    let bg_color = if is_recording { th.surface1 } else { th.surface0 };
                    let text_color = if is_recording { th.overlay1 } else { th.text };

                    let button = egui::Button::new(
                        egui::RichText::new(&display_text).color(text_color).monospace(),
                    )
                    .fill(bg_color)
                    .min_size(egui::vec2(BUTTON_WIDTH, BUTTON_HEIGHT));

                    if ui.add_enabled(can_record, button).clicked() {
                        *recording_field = Some(RecordingSlot {
                            field_id: field_id.to_string(),
                            idx,
                        });
                    }
                }

                // 새 바인딩 추가 버튼. 바인딩이 없을 때는 "없음" 플레이스홀더.
                let adding = matches!(
                    recording_field,
                    Some(slot) if slot.field_id == *field_id && slot.idx == bindings_len
                );
                let add_label = if adding {
                    t("settings.keybindings.hint_press_key").to_string()
                } else if bindings_len == 0 {
                    t("settings.keybindings.hint_none").to_string()
                } else {
                    "+".to_string()
                };
                let add_bg = if adding { th.surface1 } else { th.surface0 };
                let add_fg = if adding { th.overlay1 } else { th.subtext0 };
                let add_width = if bindings_len == 0 {
                    BUTTON_WIDTH
                } else {
                    ADD_BUTTON_WIDTH
                };
                let add_btn = egui::Button::new(
                    egui::RichText::new(&add_label).color(add_fg).monospace(),
                )
                .fill(add_bg)
                .min_size(egui::vec2(add_width, BUTTON_HEIGHT));
                if ui
                    .add_enabled(can_record, add_btn)
                    .on_hover_text(t("settings.keybindings.add_binding_button"))
                    .clicked()
                {
                    *recording_field = Some(RecordingSlot {
                        field_id: field_id.to_string(),
                        idx: bindings_len,
                    });
                }
            });
        });
        ui.add_space(ROW_GAP);
    }
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
