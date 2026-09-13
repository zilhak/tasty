//! 가져오기 미리보기의 자료 — 표의 행, 행 단위 적용, option 마이그레이션 행.
//!
//! 화면(`import_export.rs`)과 떼어 둔 이유는 적용 규칙을 창 없이 단정하기 위해서다 —
//! "선택한 행만 draft 에 들어가고, 번들에 없는 plugin override 는 남는다" 는 egui 가 없어도
//! 참이어야 한다.

use std::collections::{BTreeMap, BTreeSet};

use tasty_host_plugin::keybinding_bundle::PluginShortcutOverrides;
use tasty_host_plugin::keybinding_bundle::option_migration::{
    BindingSite, Resolution, ResolutionPlan,
};

use crate::plugin::registry_state::ShortcutOverride;
use crate::settings::{KeybindingSettings, SwitchAxis, SwitchStep};

/// 설정 창의 plugin override draft — 키 `(plugin_id, command_id)`, 값 `Some` = 설정,
/// `None` = override 제거(매니페스트 기본값 복귀).
pub(crate) type PluginShortcutDraft = BTreeMap<(String, String), Option<ShortcutOverride>>;

/// 표의 네 그룹 — 번들이 실어 나르는 네 자리.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Group {
    General,
    QuickSwitch,
    Scripts,
    Plugins,
}

impl Group {
    pub(crate) const ALL: [Group; 4] = [
        Group::General,
        Group::QuickSwitch,
        Group::Scripts,
        Group::Plugins,
    ];

    pub(crate) fn label_key(self) -> &'static str {
        match self {
            Group::General => "settings.keybindings.ie_group_general",
            Group::QuickSwitch => "settings.keybindings.ie_group_quickswitch",
            Group::Scripts => "settings.keybindings.ie_group_scripts",
            Group::Plugins => "settings.keybindings.ie_group_plugins",
        }
    }
}

/// 표의 한 행 — 적용의 단위이기도 하다.
///
/// quick-switch 는 **축마다 한 행**이다. 슬롯은 raw 키를 저장하고 조합은 표시 시점에
/// modifier 와 합성되므로, 슬롯 하나만 골라 적용하면 다른 modifier 아래에서 뜻이 바뀐다.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum RowKey {
    General(&'static str),
    Axis(SwitchAxis),
    Script(String),
    Plugin {
        plugin_id: String,
        command_id: String,
    },
}

impl RowKey {
    pub(crate) fn group(&self) -> Group {
        match self {
            RowKey::General(_) => Group::General,
            RowKey::Axis(_) => Group::QuickSwitch,
            RowKey::Script(_) => Group::Scripts,
            RowKey::Plugin { .. } => Group::Plugins,
        }
    }

    /// 마이그레이션 자리가 속한 행.
    pub(crate) fn of_site(site: &BindingSite) -> RowKey {
        match site {
            BindingSite::GeneralBinding { field_id, .. } => RowKey::General(field_id),
            BindingSite::AxisModifier { axis }
            | BindingSite::AxisSlot { axis, .. }
            | BindingSite::AxisStep { axis, .. } => RowKey::Axis(*axis),
            BindingSite::ScriptBinding { script_id } => RowKey::Script(script_id.clone()),
            BindingSite::PluginOverride {
                plugin_id,
                command_id,
                ..
            } => RowKey::Plugin {
                plugin_id: plugin_id.clone(),
                command_id: command_id.clone(),
            },
        }
    }
}

/// 마이그레이션 행 하나의 사용자 선택.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MigrationValue {
    Unset,
    Set(String),
    /// 이 환경에서 비워 둔다 — 해소로 센다.
    Unbound,
}

#[derive(Debug, Clone)]
pub(crate) struct MigrationRow {
    pub site: BindingSite,
    pub from: String,
    pub value: MigrationValue,
}

/// 지금 행들의 선택을 해소 계획으로 — `Unset` 은 계획에 안 들어간다.
pub(crate) fn plan_of(rows: &[MigrationRow]) -> ResolutionPlan {
    rows.iter()
        .filter_map(|r| match &r.value {
            MigrationValue::Unset => None,
            MigrationValue::Set(v) => Some((r.site.clone(), Resolution::Replace(v.clone()))),
            MigrationValue::Unbound => Some((r.site.clone(), Resolution::Unbind)),
        })
        .collect()
}

pub(crate) fn unresolved_count(rows: &[MigrationRow]) -> usize {
    rows.iter()
        .filter(|r| r.value == MigrationValue::Unset)
        .count()
}

/// 디스크의 override 위에 설정 창 draft 를 얹은 현재 값 — export 의 원본이자 미리보기의
/// "Current" 열이다. 스냅샷이 아니라 `PluginsConfig.keybindings` 전량에서 출발해야
/// 등록되지 않은 plugin 의 override 가 빠지지 않는다.
pub(crate) fn merged_overrides(
    base: &PluginShortcutOverrides,
    draft: &PluginShortcutDraft,
) -> PluginShortcutOverrides {
    let mut out = base.clone();
    for ((plugin_id, command_id), value) in draft {
        match value {
            Some(ov) => {
                out.entry(plugin_id.clone())
                    .or_default()
                    .insert(command_id.clone(), ov.clone());
            }
            None => {
                if let Some(commands) = out.get_mut(plugin_id) {
                    commands.remove(command_id);
                    if commands.is_empty() {
                        out.remove(plugin_id);
                    }
                }
            }
        }
    }
    out
}

/// 표에 오를 행 전량(그룹 순서대로).
///
/// - 일반: 모든 필드.
/// - quick-switch: 세 축.
/// - 스크립트: 현재와 번들의 **합집합** — 번들에 없는 현재 바인딩은 적용하면 사라지므로
///   그 변경도 보여야 한다.
/// - plugin: **번들에 있는 항목만**. 이 환경에만 있는 override 는 적용 대상이 아니다
///   (다른 환경은 그 plugin 을 몰랐을 수 있다 — 없음이 "지워라" 가 아니다).
pub(crate) fn row_keys(
    current: &KeybindingSettings,
    imported: &KeybindingSettings,
    imported_overrides: &PluginShortcutOverrides,
) -> Vec<RowKey> {
    let mut keys: Vec<RowKey> = KeybindingSettings::GENERAL_BINDING_FIELDS
        .iter()
        .map(|(id, _)| RowKey::General(id))
        .collect();
    keys.extend(SwitchAxis::ALL.into_iter().map(RowKey::Axis));
    let mut seen = BTreeSet::new();
    for b in current
        .script_bindings
        .iter()
        .chain(imported.script_bindings.iter())
    {
        if seen.insert(b.script_id.clone()) {
            keys.push(RowKey::Script(b.script_id.clone()));
        }
    }
    for (plugin_id, commands) in imported_overrides {
        for command_id in commands.keys() {
            keys.push(RowKey::Plugin {
                plugin_id: plugin_id.clone(),
                command_id: command_id.clone(),
            });
        }
    }
    keys
}

/// 현재와 가져올 값이 다른 행인가.
pub(crate) fn row_changed(
    key: &RowKey,
    current: &KeybindingSettings,
    current_overrides: &PluginShortcutOverrides,
    imported: &KeybindingSettings,
    imported_overrides: &PluginShortcutOverrides,
) -> bool {
    match key {
        RowKey::General(field) => current.get_bindings(field) != imported.get_bindings(field),
        RowKey::Axis(axis) => !axis_equal(*axis, current, imported),
        RowKey::Script(id) => current.script_binding_combo(id) != imported.script_binding_combo(id),
        RowKey::Plugin {
            plugin_id,
            command_id,
        } => {
            override_of(current_overrides, plugin_id, command_id)
                != override_of(imported_overrides, plugin_id, command_id)
        }
    }
}

pub(crate) fn override_of<'a>(
    overrides: &'a PluginShortcutOverrides,
    plugin_id: &str,
    command_id: &str,
) -> Option<&'a ShortcutOverride> {
    overrides.get(plugin_id).and_then(|m| m.get(command_id))
}

fn axis_equal(axis: SwitchAxis, a: &KeybindingSettings, b: &KeybindingSettings) -> bool {
    axis.modifier(a) == axis.modifier(b)
        && (0..axis.slot_count()).all(|i| axis.slot(a, i) == axis.slot(b, i))
        && SwitchStep::ALL
            .into_iter()
            .all(|s| axis.step(a, s) == axis.step(b, s))
}

/// 고른 행을 두 draft 에 쓴다 — 호스트 단축키는 `draft`, plugin override 는
/// `plugin_draft`. 한쪽만 쓰면 Save 후에도 절반만 반영된다.
pub(crate) fn apply_rows<'a>(
    keys: impl IntoIterator<Item = &'a RowKey>,
    draft: &mut KeybindingSettings,
    plugin_draft: &mut PluginShortcutDraft,
    imported: &KeybindingSettings,
    imported_overrides: &PluginShortcutOverrides,
) {
    for key in keys {
        match key {
            RowKey::General(field) => {
                draft.clear_field(field);
                for combo in imported.get_bindings(field).unwrap_or(&[]) {
                    draft.add_binding(field, combo.clone());
                }
            }
            RowKey::Axis(axis) => {
                axis.set_modifier(draft, axis.modifier(imported));
                for i in 0..axis.slot_count() {
                    axis.set_slot(draft, i, axis.slot(imported, i).unwrap_or_default());
                }
                for step in SwitchStep::ALL {
                    axis.set_step(draft, step, axis.step(imported, step));
                }
            }
            RowKey::Script(id) => match imported.script_binding_combo(id) {
                Some(combo) => draft.set_script_binding(id, combo.to_string()),
                None => {
                    draft.remove_script_binding(id);
                }
            },
            RowKey::Plugin {
                plugin_id,
                command_id,
            } => {
                if let Some(ov) = override_of(imported_overrides, plugin_id, command_id) {
                    plugin_draft.insert((plugin_id.clone(), command_id.clone()), Some(ov.clone()));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys_ov(plugin: &str, command: &str, combo: &str) -> PluginShortcutOverrides {
        let mut commands = BTreeMap::new();
        commands.insert(
            command.to_string(),
            ShortcutOverride::Key {
                value: vec![combo.to_string()],
            },
        );
        let mut out = PluginShortcutOverrides::new();
        out.insert(plugin.to_string(), commands);
        out
    }

    /// draft 의 `Some` 은 얹고 `None` 은 지운다 — 마지막 항목이 지워지면 plugin 키도 없다.
    #[test]
    fn merged_overrides_layers_the_draft_on_the_config() {
        let base = keys_ov("p", "a", "ctrl+a");
        let mut draft = PluginShortcutDraft::new();
        draft.insert(("p".into(), "a".into()), None);
        draft.insert(
            ("q".into(), "b".into()),
            Some(ShortcutOverride::Key {
                value: vec!["ctrl+b".into()],
            }),
        );
        let merged = merged_overrides(&base, &draft);
        assert!(!merged.contains_key("p"));
        assert!(override_of(&merged, "q", "b").is_some());
    }

    /// 고른 행만 들어가고, 안 고른 행과 번들에 없는 plugin override 는 그대로다.
    #[test]
    fn only_selected_rows_are_written_and_target_only_plugins_survive() {
        let mut draft = KeybindingSettings::preset_tasty();
        let mut imported = KeybindingSettings::preset_tasty();
        imported.new_tab = vec!["ctrl+alt+shift+t".into()];
        imported.new_workspace = vec!["ctrl+alt+shift+w".into()];
        imported.tab_switch_modifier = "ctrl+shift".into();
        imported.set_script_binding("s1", "ctrl+alt+shift+1".into());
        let imported_ov = keys_ov("p", "a", "ctrl+alt+shift+p");

        let mut plugin_draft = PluginShortcutDraft::new();
        let before_ws = draft.new_workspace.clone();
        let selected = [
            RowKey::General("new_tab"),
            RowKey::Axis(SwitchAxis::Tab),
            RowKey::Script("s1".into()),
            RowKey::Plugin {
                plugin_id: "p".into(),
                command_id: "a".into(),
            },
        ];
        apply_rows(
            &selected,
            &mut draft,
            &mut plugin_draft,
            &imported,
            &imported_ov,
        );

        assert_eq!(draft.new_tab, imported.new_tab);
        assert_eq!(draft.new_workspace, before_ws, "안 고른 행은 그대로다");
        assert_eq!(draft.tab_switch_modifier, "ctrl+shift");
        assert_eq!(draft.script_binding_combo("s1"), Some("ctrl+alt+shift+1"));
        assert_eq!(
            plugin_draft.len(),
            1,
            "번들에 있는 override 한 건만 draft 에 간다"
        );
    }

    /// 번들에 없는 현재 스크립트 바인딩도 행이 되고, 적용하면 사라진다.
    #[test]
    fn a_script_binding_missing_from_the_bundle_is_a_removal_row() {
        let mut current = KeybindingSettings::preset_tasty();
        current.set_script_binding("old", "ctrl+alt+shift+o".into());
        let imported = KeybindingSettings::preset_tasty();
        let keys = row_keys(&current, &imported, &PluginShortcutOverrides::new());
        let row = RowKey::Script("old".into());
        assert!(keys.contains(&row));
        let none = PluginShortcutOverrides::new();
        assert!(row_changed(&row, &current, &none, &imported, &none));
        apply_rows(
            [&row],
            &mut current,
            &mut PluginShortcutDraft::new(),
            &imported,
            &none,
        );
        assert_eq!(current.script_binding_combo("old"), None);
    }

    /// plugin 행은 번들 쪽에서만 온다.
    #[test]
    fn plugin_rows_come_from_the_bundle_only() {
        let kb = KeybindingSettings::preset_tasty();
        let keys = row_keys(&kb, &kb, &keys_ov("p", "a", "ctrl+a"));
        let plugin_rows = keys.iter().filter(|k| k.group() == Group::Plugins).count();
        assert_eq!(plugin_rows, 1);
    }

    /// Unset 은 계획에 없고, 미해결 수로만 센다.
    #[test]
    fn unset_rows_stay_out_of_the_plan() {
        let rows = vec![
            MigrationRow {
                site: BindingSite::GeneralBinding {
                    field_id: "new_tab",
                    index: 0,
                },
                from: "option+t".into(),
                value: MigrationValue::Unset,
            },
            MigrationRow {
                site: BindingSite::GeneralBinding {
                    field_id: "new_tab",
                    index: 1,
                },
                from: "option+y".into(),
                value: MigrationValue::Unbound,
            },
        ];
        assert_eq!(plan_of(&rows).len(), 1);
        assert_eq!(unresolved_count(&rows), 1);
    }
}
