use crate::i18n::t;
use crate::plugin::manifest::BindingMode;
use crate::plugin::registry_state::ShortcutOverride;
use crate::plugin_bridge::host_actions;
use crate::settings::KeybindingSettings;
use crate::settings_ui::{PluginShortcutRow, PluginShortcutSnapshot};

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
pub(super) fn draw_plugins_subtab(
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
            .and_then(|sel| {
                plugin_ids
                    .iter()
                    .find(|(id, _)| *id == sel)
                    .map(|(_, n)| *n)
            })
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
            apply_mode_change(
                row,
                draft,
                current_ov.as_ref(),
                new_mode,
                manifest_inherit_source,
            );
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
                    _ => manifest_inherit_source
                        .unwrap_or("clipboard.copy")
                        .to_string(),
                };
                let combo_id = format!("plugin_inherit_src::{}::{}", row.plugin_id, row.command_id);
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
                        Some(ShortcutOverride::Inherit {
                            source: new_source.clone(),
                        }),
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
                        .hint_text(crate::theme_bridge::hint_text("ctrl+f5")),
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
