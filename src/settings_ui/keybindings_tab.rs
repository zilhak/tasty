use crate::i18n::t;
use crate::plugin::host_actions;
use crate::plugin::manifest::BindingMode;
use crate::plugin::registry_state::ShortcutOverride;
use crate::settings::{KeybindingSettings, Settings};
use crate::settings_ui::{PluginShortcutRow, PluginShortcutSnapshot};

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
    Tab,
    Surface,
    Clipboard,
    Zoom,
    Image,
    Preset,
    Plugins,
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
    captured_winit_combo: &mut Option<KeyCapture>,
    plugin_shortcuts: &PluginShortcutSnapshot,
    plugin_shortcuts_selected: &mut Option<String>,
    plugin_shortcuts_draft: &mut std::collections::BTreeMap<
        (String, String),
        Option<ShortcutOverride>,
    >,
) {
    let th = crate::theme::theme();
    ui.add_space(8.0);

    let available_height = ui.available_height() - 8.0 - 14.0;

    ui.horizontal_top(|ui| {
        egui::Frame::new()
            .fill(th.crust.into())
            .stroke(egui::Stroke::new(1.0, th.surface0))
            .corner_radius(4.0)
            .inner_margin(egui::Margin::symmetric(6, 6))
            .show(ui, |ui| {
                ui.set_width(100.0);
                ui.set_min_height(available_height);

                ui.vertical(|ui| {
                    let sub_tabs = [
                        (
                            KeybindingsSubTab::General,
                            t("settings.keybindings.subtab.general"),
                        ),
                        (
                            KeybindingsSubTab::Workspace,
                            t("settings.keybindings.subtab.workspace"),
                        ),
                        (
                            KeybindingsSubTab::Pane,
                            t("settings.keybindings.subtab.pane"),
                        ),
                        (
                            KeybindingsSubTab::Tab,
                            t("settings.keybindings.subtab.tab"),
                        ),
                        (
                            KeybindingsSubTab::Surface,
                            t("settings.keybindings.subtab.surface"),
                        ),
                        (
                            KeybindingsSubTab::Clipboard,
                            t("settings.keybindings.subtab.clipboard"),
                        ),
                        (
                            KeybindingsSubTab::Zoom,
                            t("settings.keybindings.subtab.zoom"),
                        ),
                        (
                            KeybindingsSubTab::Image,
                            t("settings.keybindings.subtab.image"),
                        ),
                        (
                            KeybindingsSubTab::Preset,
                            t("settings.keybindings.subtab.preset"),
                        ),
                        (
                            KeybindingsSubTab::Plugins,
                            t("settings.keybindings.subtab.plugins"),
                        ),
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

            // winit에서 직접 캡처한 키 조합을 사용. double-tap이 우선.
            let captured = if recording_field.is_some() {
                if let Some(dt) = captured_double_tap.take() {
                    KeyCapture::Combo(dt)
                } else {
                    captured_winit_combo.take().unwrap_or(KeyCapture::None)
                }
            } else {
                KeyCapture::None
            };

            match *sub_tab {
                KeybindingsSubTab::General => {
                    draw_keybinding_entries(
                        ui,
                        &mut settings.keybindings,
                        recording_field,
                        pending_binding,
                        &captured,
                        &[
                            (
                                "toggle_settings",
                                "settings.keybindings.toggle_settings_label",
                            ),
                            (
                                "toggle_notifications",
                                "settings.keybindings.toggle_notifications_label",
                            ),
                            (
                                "toggle_clipboard_viewer",
                                "settings.keybindings.toggle_clipboard_viewer_label",
                            ),
                            (
                                "restore_closed",
                                "settings.keybindings.restore_closed_label",
                            ),
                            ("new_window", "settings.keybindings.new_window_label"),
                            ("quit", "settings.keybindings.quit_label"),
                            (
                                "quit_immediate",
                                "settings.keybindings.quit_immediate_label",
                            ),
                            ("quit_minimize", "settings.keybindings.quit_minimize_label"),
                        ],
                    );
                }
                KeybindingsSubTab::Workspace => {
                    draw_keybinding_entries(
                        ui,
                        &mut settings.keybindings,
                        recording_field,
                        pending_binding,
                        &captured,
                        &[
                            ("new_workspace", "settings.keybindings.new_workspace_label"),
                            (
                                "rename_workspace",
                                "settings.keybindings.rename_workspace_label",
                            ),
                            (
                                "rename_workspace_subtitle",
                                "settings.keybindings.rename_workspace_subtitle_label",
                            ),
                            (
                                "close_workspace",
                                "settings.keybindings.close_workspace_label",
                            ),
                        ],
                    );

                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(4.0);

                    egui::Grid::new("ws_modifier_grid")
                        .num_columns(2)
                        .spacing([12.0, 8.0])
                        .show(ui, |ui| {
                            ui.label(t("settings.keybindings.workspace_switch_modifier_label"));
                            egui::ComboBox::from_id_salt("workspace_switch_modifier")
                                .selected_text(modifier_display(
                                    &settings.keybindings.workspace_switch_modifier,
                                ))
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut settings.keybindings.workspace_switch_modifier,
                                        "ctrl".to_string(),
                                        "Ctrl",
                                    );
                                    ui.selectable_value(
                                        &mut settings.keybindings.workspace_switch_modifier,
                                        "alt".to_string(),
                                        "Alt",
                                    );
                                });
                            ui.end_row();
                        });
                }
                KeybindingsSubTab::Pane => {
                    draw_keybinding_entries(
                        ui,
                        &mut settings.keybindings,
                        recording_field,
                        pending_binding,
                        &captured,
                        &[
                            (
                                "split_pane_vertical",
                                "settings.keybindings.split_pane_vertical_label",
                            ),
                            (
                                "split_pane_horizontal",
                                "settings.keybindings.split_pane_horizontal_label",
                            ),
                            (
                                "focus_pane_next",
                                "settings.keybindings.focus_pane_next_label",
                            ),
                            (
                                "focus_pane_prev",
                                "settings.keybindings.focus_pane_prev_label",
                            ),
                            ("close_pane", "settings.keybindings.close_pane_label"),
                        ],
                    );
                }
                KeybindingsSubTab::Tab => {
                    draw_keybinding_entries(
                        ui,
                        &mut settings.keybindings,
                        recording_field,
                        pending_binding,
                        &captured,
                        &[
                            ("new_tab", "settings.keybindings.new_tab_label"),
                            ("open_markdown", "settings.keybindings.open_markdown_label"),
                            ("next_tab", "settings.keybindings.next_tab_label"),
                            ("prev_tab", "settings.keybindings.prev_tab_label"),
                            ("rename_tab", "settings.keybindings.rename_tab_label"),
                            ("close_active", "settings.keybindings.close_active_label"),
                        ],
                    );

                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(4.0);

                    egui::Grid::new("tab_modifier_grid")
                        .num_columns(2)
                        .spacing([12.0, 8.0])
                        .show(ui, |ui| {
                            ui.label(t("settings.keybindings.tab_switch_modifier_label"));
                            egui::ComboBox::from_id_salt("tab_switch_modifier")
                                .selected_text(modifier_display(
                                    &settings.keybindings.tab_switch_modifier,
                                ))
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut settings.keybindings.tab_switch_modifier,
                                        "ctrl".to_string(),
                                        "Ctrl",
                                    );
                                    ui.selectable_value(
                                        &mut settings.keybindings.tab_switch_modifier,
                                        "alt".to_string(),
                                        "Alt",
                                    );
                                });
                            ui.end_row();
                        });
                }
                KeybindingsSubTab::Surface => {
                    draw_keybinding_entries(
                        ui,
                        &mut settings.keybindings,
                        recording_field,
                        pending_binding,
                        &captured,
                        &[
                            (
                                "split_surface_vertical",
                                "settings.keybindings.split_surface_vertical_label",
                            ),
                            (
                                "split_surface_horizontal",
                                "settings.keybindings.split_surface_horizontal_label",
                            ),
                            (
                                "focus_surface_next",
                                "settings.keybindings.focus_surface_next_label",
                            ),
                            (
                                "focus_surface_prev",
                                "settings.keybindings.focus_surface_prev_label",
                            ),
                            (
                                "convert_surface",
                                "settings.keybindings.convert_surface_label",
                            ),
                            (
                                "convert_to_markdown",
                                "settings.keybindings.convert_to_markdown_label",
                            ),
                            ("close_surface", "settings.keybindings.close_surface_label"),
                        ],
                    );
                }
                KeybindingsSubTab::Clipboard => {
                    draw_keybinding_entries(
                        ui,
                        &mut settings.keybindings,
                        recording_field,
                        pending_binding,
                        &captured,
                        &[
                            ("copy", "settings.keybindings.copy_label"),
                            ("copy_path", "settings.keybindings.copy_path_label"),
                            ("cut", "settings.keybindings.cut_label"),
                            ("select_all", "settings.keybindings.select_all_label"),
                            ("paste", "settings.keybindings.paste_label"),
                        ],
                    );
                }
                KeybindingsSubTab::Zoom => {
                    draw_keybinding_entries(
                        ui,
                        &mut settings.keybindings,
                        recording_field,
                        pending_binding,
                        &captured,
                        &[
                            ("zoom_in", "settings.keybindings.zoom_in_label"),
                            ("zoom_out", "settings.keybindings.zoom_out_label"),
                            ("zoom_reset", "settings.keybindings.zoom_reset_label"),
                        ],
                    );
                }
                KeybindingsSubTab::Image => {
                    draw_keybinding_entries(
                        ui,
                        &mut settings.keybindings,
                        recording_field,
                        pending_binding,
                        &captured,
                        &[
                            ("image_undo", "settings.keybindings.image_undo_label"),
                            ("image_redo", "settings.keybindings.image_redo_label"),
                        ],
                    );
                }
                KeybindingsSubTab::Preset => {
                    draw_preset_subtab(ui, &mut settings.keybindings, selected_preset);
                }
                KeybindingsSubTab::Plugins => {
                    draw_plugins_subtab(
                        ui,
                        plugin_shortcuts,
                        plugin_shortcuts_selected,
                        plugin_shortcuts_draft,
                        &settings.keybindings,
                    );
                }
            }

            if !matches!(
                *sub_tab,
                KeybindingsSubTab::Preset | KeybindingsSubTab::Plugins
            ) {
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(t("settings.keybindings.hint_esc_to_clear"))
                        .small()
                        .color(th.overlay1),
                );
            }
        }); // end vertical
    }); // end horizontal_top

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
            .fill(th.mantle.into())
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
                .drag_to_scroll(false)
                .show(ui, |ui| {
                    egui::Grid::new("preset_preview_grid")
                        .num_columns(3)
                        .spacing([16.0, 6.0])
                        .striped(true)
                        .show(ui, |ui| {
                            let strong = |s: String| egui::RichText::new(s).color(th.text).strong();
                            let normal = |s: String| egui::RichText::new(s).color(th.text);

                            ui.label(strong(
                                t("settings.keybindings.preset_col_action").to_string(),
                            ));
                            ui.label(strong(
                                t("settings.keybindings.preset_col_before").to_string(),
                            ));
                            ui.label(strong(
                                t("settings.keybindings.preset_col_after").to_string(),
                            ));
                            ui.end_row();

                            for (field_id, label_key) in KeybindingSettings::GENERAL_BINDING_FIELDS
                            {
                                let before_raw = keybindings.get_bindings(field_id).unwrap_or(&[]);
                                let after_raw = preset.get_bindings(field_id).unwrap_or(&[]);
                                let changed = before_raw != after_raw;

                                let action_label =
                                    t(label_key).trim_end_matches(':').trim().to_string();
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

/// Plugin command 한 줄의 mode UI 상태.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RowMode {
    Inherit,
    Custom,
    None,
}

fn row_mode_of(ov: Option<&ShortcutOverride>, fallback: &BindingMode) -> RowMode {
    match ov {
        Some(ShortcutOverride::Inherit { .. }) => RowMode::Inherit,
        Some(ShortcutOverride::Key { .. }) => RowMode::Custom,
        Some(ShortcutOverride::None) => RowMode::None,
        None => match fallback {
            BindingMode::InheritHost(_) => RowMode::Inherit,
            BindingMode::Independent => RowMode::Custom,
        },
    }
}

/// override가 "row.current_override 또는 매니페스트 default와 동일"하면 draft에서
/// 제거 (clear). 그게 아니면 draft에 누적. 결과적으로 모달 close 시 main이
/// draft를 회수해 변경된 키만 plugins.toml에 반영.
fn commit_row_change(
    draft: &mut std::collections::BTreeMap<(String, String), Option<ShortcutOverride>>,
    row: &PluginShortcutRow,
    new_value: Option<ShortcutOverride>,
) {
    let key = (row.plugin_id.clone(), row.command_id.clone());
    // 기존 stored override와 동일하면 draft 항목 제거 (변경 없음).
    if shortcut_override_eq(new_value.as_ref(), row.current_override.as_ref()) {
        draft.remove(&key);
    } else {
        draft.insert(key, new_value);
    }
}

fn shortcut_override_eq(a: Option<&ShortcutOverride>, b: Option<&ShortcutOverride>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(ShortcutOverride::Key { value: v1 }), Some(ShortcutOverride::Key { value: v2 })) => {
            v1 == v2
        }
        (
            Some(ShortcutOverride::Inherit { source: s1 }),
            Some(ShortcutOverride::Inherit { source: s2 }),
        ) => s1 == s2,
        (Some(ShortcutOverride::None), Some(ShortcutOverride::None)) => true,
        _ => false,
    }
}

/// Plugins 서브탭 본문 (단계 E-c: 변경 가능 UI).
fn draw_plugins_subtab(
    ui: &mut egui::Ui,
    snapshot: &PluginShortcutSnapshot,
    selected: &mut Option<String>,
    draft: &mut std::collections::BTreeMap<(String, String), Option<ShortcutOverride>>,
    host_kb: &KeybindingSettings,
) {
    let th = crate::theme::theme();

    if snapshot.rows.is_empty() {
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(t("settings.keybindings.plugins.no_plugins_with_commands"))
                .color(th.subtext0),
        );
        return;
    }

    let mut plugin_ids: Vec<(&str, &str)> = Vec::new();
    for row in &snapshot.rows {
        if !plugin_ids.iter().any(|(id, _)| *id == row.plugin_id) {
            plugin_ids.push((row.plugin_id.as_str(), row.plugin_name.as_str()));
        }
    }
    plugin_ids.sort_by(|a, b| a.1.cmp(b.1));

    if selected
        .as_deref()
        .is_none_or(|s| !plugin_ids.iter().any(|(id, _)| *id == s))
    {
        *selected = plugin_ids.first().map(|(id, _)| id.to_string());
    }

    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.label(t("settings.keybindings.plugins.plugin_label"));
        let current_label = selected
            .as_deref()
            .and_then(|sel| plugin_ids.iter().find(|(id, _)| *id == sel).map(|(_, n)| *n))
            .unwrap_or("");
        egui::ComboBox::from_id_salt("plugin_shortcuts_combo")
            .selected_text(current_label)
            .show_ui(ui, |ui| {
                for (id, name) in &plugin_ids {
                    ui.selectable_value(selected, Some(id.to_string()), *name);
                }
            });
    });
    ui.add_space(8.0);

    let Some(active_id) = selected.clone() else {
        ui.label(t("settings.keybindings.plugins.none_selected"));
        return;
    };

    let rows: Vec<&PluginShortcutRow> = snapshot
        .rows
        .iter()
        .filter(|r| r.plugin_id == active_id)
        .collect();

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .drag_to_scroll(false)
        .show(ui, |ui| {
            for row in &rows {
                draw_plugin_command_row(ui, row, draft, host_kb);
                ui.separator();
            }
        });
}

fn draw_plugin_command_row(
    ui: &mut egui::Ui,
    row: &PluginShortcutRow,
    draft: &mut std::collections::BTreeMap<(String, String), Option<ShortcutOverride>>,
    host_kb: &KeybindingSettings,
) {
    let key = (row.plugin_id.clone(), row.command_id.clone());
    // 현재 effective override: draft가 우선, 없으면 row.current_override
    let current_ov: Option<ShortcutOverride> = match draft.get(&key) {
        Some(o) => o.clone(),
        None => row.current_override.clone(),
    };
    let mode = row_mode_of(current_ov.as_ref(), &row.binding_mode);
    // 매니페스트가 inherit를 declare한 경우 source 후보를 매니페스트 default로
    // 시작. 그 외에는 화이트리스트 첫번째로 시작.
    let manifest_inherit_source: Option<&str> = match &row.binding_mode {
        BindingMode::InheritHost(s) => Some(s.as_str()),
        BindingMode::Independent => None,
    };

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(t(&row.title_i18n_key)).strong());
        ui.add_space(8.0);

        // mode ComboBox
        let mode_label = match mode {
            RowMode::Inherit => t("settings.keybindings.plugins.mode_inherit"),
            RowMode::Custom => t("settings.keybindings.plugins.mode_custom"),
            RowMode::None => t("settings.keybindings.plugins.mode_none_label"),
        };
        let combo_id = format!("plugin_mode::{}::{}", row.plugin_id, row.command_id);
        let mut new_mode = mode;
        egui::ComboBox::from_id_salt(combo_id)
            .selected_text(mode_label)
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut new_mode,
                    RowMode::Inherit,
                    t("settings.keybindings.plugins.mode_inherit"),
                );
                ui.selectable_value(
                    &mut new_mode,
                    RowMode::Custom,
                    t("settings.keybindings.plugins.mode_custom"),
                );
                ui.selectable_value(
                    &mut new_mode,
                    RowMode::None,
                    t("settings.keybindings.plugins.mode_none_label"),
                );
            });
        if new_mode != mode {
            apply_mode_change(row, draft, current_ov.as_ref(), new_mode, manifest_inherit_source);
        }
    });

    // mode별 부속 UI (실제로 표시할 mode는 draft 적용 직후의 값으로 재계산)
    let after_ov: Option<ShortcutOverride> = match draft.get(&key) {
        Some(o) => o.clone(),
        None => row.current_override.clone(),
    };
    let after_mode = row_mode_of(after_ov.as_ref(), &row.binding_mode);
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        match after_mode {
            RowMode::Inherit => {
                let active_source: String = match &after_ov {
                    Some(ShortcutOverride::Inherit { source }) => source.clone(),
                    _ => manifest_inherit_source.unwrap_or("clipboard.copy").to_string(),
                };
                let combo_id = format!(
                    "plugin_inherit_src::{}::{}",
                    row.plugin_id, row.command_id
                );
                let mut new_source = active_source.clone();
                egui::ComboBox::from_id_salt(combo_id)
                    .selected_text(&active_source)
                    .show_ui(ui, |ui| {
                        for src in host_actions::INHERITABLE_HOST_ACTIONS {
                            ui.selectable_value(&mut new_source, src.to_string(), *src);
                        }
                    });
                if new_source != active_source {
                    commit_row_change(
                        draft,
                        row,
                        Some(ShortcutOverride::Inherit { source: new_source.clone() }),
                    );
                }
                let resolved = host_actions::host_action_for(host_kb, &active_source)
                    .map(|v| {
                        v.iter()
                            .map(|s| KeybindingSettings::format_display(s))
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_default();
                ui.weak(format!(
                    "{} ({})",
                    t("settings.keybindings.plugins.inherit_source_prefix"),
                    if resolved.is_empty() {
                        t("settings.keybindings.hint_none").to_string()
                    } else {
                        resolved
                    }
                ));
            }
            RowMode::Custom => {
                let current_value: String = match &after_ov {
                    Some(ShortcutOverride::Key { value }) => value.join(", "),
                    _ => row.manifest_default.clone().unwrap_or_default(),
                };
                let edit_id = format!("plugin_key::{}::{}", row.plugin_id, row.command_id);
                let mut buf = current_value.clone();
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut buf)
                        .id_salt(edit_id)
                        .desired_width(180.0)
                        .hint_text("ctrl+f5"),
                );
                if resp.changed() {
                    let parsed: Vec<String> = buf
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                    let new_ov = if parsed.is_empty() {
                        Some(ShortcutOverride::Key { value: vec![] })
                    } else {
                        Some(ShortcutOverride::Key { value: parsed })
                    };
                    commit_row_change(draft, row, new_ov);
                }
            }
            RowMode::None => {
                ui.weak(t("settings.keybindings.plugins.mode_none_label"));
            }
        }

        ui.add_space(8.0);
        // 매니페스트 default로 복귀 버튼 — draft + current_override 모두 비움.
        if ui
            .small_button(t("settings.keybindings.plugins.reset_button"))
            .on_hover_text(t("settings.keybindings.plugins.reset_hint"))
            .clicked()
        {
            // None을 명시적으로 draft에 넣어 main이 clear_shortcut_override 호출하도록.
            // 단, current_override가 이미 None이면 변경 없음.
            if row.current_override.is_some() {
                draft.insert((row.plugin_id.clone(), row.command_id.clone()), None);
            } else {
                draft.remove(&(row.plugin_id.clone(), row.command_id.clone()));
            }
        }
    });
}

/// mode가 바뀌면 합리적인 시작값으로 override를 작성해 draft에 push.
fn apply_mode_change(
    row: &PluginShortcutRow,
    draft: &mut std::collections::BTreeMap<(String, String), Option<ShortcutOverride>>,
    current: Option<&ShortcutOverride>,
    new_mode: RowMode,
    manifest_inherit_source: Option<&str>,
) {
    let new_ov = match new_mode {
        RowMode::Inherit => {
            // 매니페스트가 inherit를 제공하면 그 source, 아니면 화이트리스트 첫번째.
            let source = manifest_inherit_source
                .unwrap_or_else(|| host_actions::INHERITABLE_HOST_ACTIONS[0])
                .to_string();
            Some(ShortcutOverride::Inherit { source })
        }
        RowMode::Custom => {
            // 기존 Custom 값이 있으면 유지, 아니면 매니페스트 default를 시작값으로.
            let value = match current {
                Some(ShortcutOverride::Key { value }) if !value.is_empty() => value.clone(),
                _ => match &row.manifest_default {
                    Some(s) if !s.is_empty() => vec![s.clone()],
                    _ => vec![],
                },
            };
            Some(ShortcutOverride::Key { value })
        }
        RowMode::None => Some(ShortcutOverride::None),
    };
    commit_row_change(draft, row, new_ov);
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

                    let bg_color = if is_recording {
                        th.surface1
                    } else {
                        th.surface0
                    };
                    let text_color = if is_recording { th.overlay1 } else { th.text };

                    let button = egui::Button::new(
                        egui::RichText::new(&display_text)
                            .color(text_color)
                            .monospace(),
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
                let add_btn =
                    egui::Button::new(egui::RichText::new(&add_label).color(add_fg).monospace())
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

/// winit KeyEvent + ModifiersState에서 키 조합 문자열을 생성한다.
/// egui를 거치지 않으므로 Cmd+C 등 egui가 시맨틱 커맨드로 소비하는
/// 조합도 정상 캡처된다.
pub fn capture_winit_key_combo(
    event: &winit::event::KeyEvent,
    modifiers: winit::keyboard::ModifiersState,
) -> KeyCapture {
    use winit::event::ElementState;
    use winit::keyboard::{Key, KeyCode, NamedKey, PhysicalKey};

    if event.state != ElementState::Pressed {
        return KeyCapture::None;
    }

    // Escape → clear
    if event.logical_key == Key::Named(NamedKey::Escape) {
        return KeyCapture::Clear;
    }

    // modifier-only 키는 무시
    if let Key::Named(n) = &event.logical_key {
        if matches!(
            n,
            NamedKey::Control
                | NamedKey::Shift
                | NamedKey::Alt
                | NamedKey::Super
                | NamedKey::Meta
                | NamedKey::Hyper
                | NamedKey::Fn
                | NamedKey::FnLock
                | NamedKey::CapsLock
                | NamedKey::NumLock
                | NamedKey::ScrollLock
                | NamedKey::Symbol
                | NamedKey::SymbolLock
        ) {
            return KeyCapture::None;
        }
    }

    // 물리 키에서 키 이름 결정 (IME/Option 변환에 영향받지 않도록)
    let key_name = physical_key_to_name(&event.physical_key)
        .or_else(|| named_key_to_name(&event.logical_key));
    let Some(key_name) = key_name else {
        return KeyCapture::None;
    };

    // modifier 조합
    let mut parts = Vec::new();
    if modifiers.control_key() {
        parts.push("ctrl");
    }
    // macOS: Cmd(⌘) = "alt" (물리적 위치가 Win/Linux Alt와 동일)
    #[cfg(target_os = "macos")]
    if modifiers.super_key() {
        parts.push("alt");
    }
    #[cfg(not(target_os = "macos"))]
    if modifiers.alt_key() {
        parts.push("alt");
    }
    // macOS: Option 키 = "option"
    #[cfg(target_os = "macos")]
    if modifiers.alt_key() {
        parts.push("option");
    }
    if modifiers.shift_key() {
        parts.push("shift");
    }

    // modifier 없는 타이핑 키는 단축키로 등록 불가
    let is_typing_key = matches!(
        event.physical_key,
        PhysicalKey::Code(
            KeyCode::KeyA
                | KeyCode::KeyB
                | KeyCode::KeyC
                | KeyCode::KeyD
                | KeyCode::KeyE
                | KeyCode::KeyF
                | KeyCode::KeyG
                | KeyCode::KeyH
                | KeyCode::KeyI
                | KeyCode::KeyJ
                | KeyCode::KeyK
                | KeyCode::KeyL
                | KeyCode::KeyM
                | KeyCode::KeyN
                | KeyCode::KeyO
                | KeyCode::KeyP
                | KeyCode::KeyQ
                | KeyCode::KeyR
                | KeyCode::KeyS
                | KeyCode::KeyT
                | KeyCode::KeyU
                | KeyCode::KeyV
                | KeyCode::KeyW
                | KeyCode::KeyX
                | KeyCode::KeyY
                | KeyCode::KeyZ
                | KeyCode::Digit0
                | KeyCode::Digit1
                | KeyCode::Digit2
                | KeyCode::Digit3
                | KeyCode::Digit4
                | KeyCode::Digit5
                | KeyCode::Digit6
                | KeyCode::Digit7
                | KeyCode::Digit8
                | KeyCode::Digit9
                | KeyCode::Space
                | KeyCode::Minus
                | KeyCode::Equal
        )
    );
    if is_typing_key && parts.is_empty() {
        return KeyCapture::None;
    }

    parts.push(key_name);
    KeyCapture::Combo(parts.join("+"))
}

fn physical_key_to_name(physical: &winit::keyboard::PhysicalKey) -> Option<&'static str> {
    use winit::keyboard::{KeyCode, PhysicalKey};
    let code = match physical {
        PhysicalKey::Code(c) => c,
        _ => return None,
    };
    Some(match code {
        KeyCode::KeyA => "a",
        KeyCode::KeyB => "b",
        KeyCode::KeyC => "c",
        KeyCode::KeyD => "d",
        KeyCode::KeyE => "e",
        KeyCode::KeyF => "f",
        KeyCode::KeyG => "g",
        KeyCode::KeyH => "h",
        KeyCode::KeyI => "i",
        KeyCode::KeyJ => "j",
        KeyCode::KeyK => "k",
        KeyCode::KeyL => "l",
        KeyCode::KeyM => "m",
        KeyCode::KeyN => "n",
        KeyCode::KeyO => "o",
        KeyCode::KeyP => "p",
        KeyCode::KeyQ => "q",
        KeyCode::KeyR => "r",
        KeyCode::KeyS => "s",
        KeyCode::KeyT => "t",
        KeyCode::KeyU => "u",
        KeyCode::KeyV => "v",
        KeyCode::KeyW => "w",
        KeyCode::KeyX => "x",
        KeyCode::KeyY => "y",
        KeyCode::KeyZ => "z",
        KeyCode::Digit0 => "0",
        KeyCode::Digit1 => "1",
        KeyCode::Digit2 => "2",
        KeyCode::Digit3 => "3",
        KeyCode::Digit4 => "4",
        KeyCode::Digit5 => "5",
        KeyCode::Digit6 => "6",
        KeyCode::Digit7 => "7",
        KeyCode::Digit8 => "8",
        KeyCode::Digit9 => "9",
        KeyCode::Tab => "tab",
        KeyCode::Space => "space",
        KeyCode::Enter => "enter",
        KeyCode::Backspace => "backspace",
        KeyCode::Delete => "delete",
        KeyCode::Insert => "insert",
        KeyCode::Home => "home",
        KeyCode::End => "end",
        KeyCode::PageUp => "pageup",
        KeyCode::PageDown => "pagedown",
        KeyCode::ArrowUp => "up",
        KeyCode::ArrowDown => "down",
        KeyCode::ArrowLeft => "left",
        KeyCode::ArrowRight => "right",
        KeyCode::F1 => "f1",
        KeyCode::F2 => "f2",
        KeyCode::F3 => "f3",
        KeyCode::F4 => "f4",
        KeyCode::F5 => "f5",
        KeyCode::F6 => "f6",
        KeyCode::F7 => "f7",
        KeyCode::F8 => "f8",
        KeyCode::F9 => "f9",
        KeyCode::F10 => "f10",
        KeyCode::F11 => "f11",
        KeyCode::F12 => "f12",
        KeyCode::Minus => "minus",
        KeyCode::Equal => "=",
        KeyCode::Comma => ",",
        KeyCode::Period => ".",
        KeyCode::Semicolon => ";",
        KeyCode::BracketLeft => "[",
        KeyCode::BracketRight => "]",
        KeyCode::Backslash => "\\",
        KeyCode::Backquote => "`",
        KeyCode::Slash => "/",
        _ => return None,
    })
}

fn named_key_to_name(key: &winit::keyboard::Key) -> Option<&'static str> {
    use winit::keyboard::NamedKey;
    if let winit::keyboard::Key::Named(n) = key {
        Some(match n {
            NamedKey::Tab => "tab",
            NamedKey::Space => "space",
            NamedKey::Enter => "enter",
            NamedKey::Backspace => "backspace",
            NamedKey::Delete => "delete",
            NamedKey::Insert => "insert",
            NamedKey::Home => "home",
            NamedKey::End => "end",
            NamedKey::PageUp => "pageup",
            NamedKey::PageDown => "pagedown",
            NamedKey::ArrowUp => "up",
            NamedKey::ArrowDown => "down",
            NamedKey::ArrowLeft => "left",
            NamedKey::ArrowRight => "right",
            NamedKey::F1 => "f1",
            NamedKey::F2 => "f2",
            NamedKey::F3 => "f3",
            NamedKey::F4 => "f4",
            NamedKey::F5 => "f5",
            NamedKey::F6 => "f6",
            NamedKey::F7 => "f7",
            NamedKey::F8 => "f8",
            NamedKey::F9 => "f9",
            NamedKey::F10 => "f10",
            NamedKey::F11 => "f11",
            NamedKey::F12 => "f12",
            _ => return None,
        })
    } else {
        None
    }
}
