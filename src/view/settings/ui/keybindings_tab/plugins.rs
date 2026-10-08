use crate::i18n::{t, t_fmt};
use crate::plugin::manifest::BindingMode;
use crate::plugin::registry_state::ShortcutOverride;
use crate::plugin_bridge::host_actions;
use crate::settings::{GeneralSettings, KeybindingSettings};
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

/// 저장된 override와 같으면 초안에서 지우고, 다르면 초안에 기록한다.
/// App은 Save로 닫았을 때만 이를 plugins.toml에 반영한다.
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

/// plugin 명령의 단축키 설정. 화면은 공용 view(`tasty_ui_widgets::kb_plugins_subtab`)가 그리고
/// 여기서는 초안·override 를 표시값으로 풀고 사용자가 바꾼 것을 초안에 반영한다.
pub(super) fn draw_plugins_subtab(
    ui: &mut egui::Ui,
    snapshot: &PluginShortcutSnapshot,
    selected: &mut Option<String>,
    draft: &mut std::collections::BTreeMap<(String, String), Option<ShortcutOverride>>,
    host_kb: &KeybindingSettings,
    general: &GeneralSettings,
) {
    let th = crate::theme::theme();

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
    let selected_index = selected
        .as_deref()
        .and_then(|sel| plugin_ids.iter().position(|(id, _)| *id == sel))
        .unwrap_or(0);
    let names: Vec<&str> = plugin_ids.iter().map(|(_, n)| *n).collect();

    let rows: Vec<&PluginShortcutRow> = match plugin_ids.get(selected_index) {
        Some((active, _)) => snapshot
            .rows
            .iter()
            .filter(|r| r.plugin_id == *active)
            .collect(),
        None => Vec::new(),
    };
    let states: Vec<RowState> = rows
        .iter()
        .map(|row| RowState::new(row, draft, host_kb, general))
        .collect();
    let mut bufs: Vec<String> = states.iter().map(|s| s.keys.clone()).collect();
    let sources: Vec<Vec<&str>> = states
        .iter()
        .map(|s| s.sources.iter().map(String::as_str).collect())
        .collect();

    let views = rows
        .iter()
        .zip(&states)
        .zip(bufs.iter_mut())
        .zip(&sources)
        .map(
            |(((row, state), buf), sources)| tasty_ui_widgets::KbPluginRowView {
                id: egui::Id::new(("plugin_shortcut", &row.plugin_id, &row.command_id)),
                title: t(&row.title_i18n_key),
                dirty: state.dirty,
                overridden: state.overridden,
                slot: match state.mode {
                    RowMode::Inherit => tasty_ui_widgets::KbPluginSlot::Inherit {
                        sources,
                        selected: state.source_index,
                        caption: &state.caption,
                    },
                    RowMode::Custom => tasty_ui_widgets::KbPluginSlot::Custom {
                        keys: buf,
                        error: state.error.as_deref(),
                    },
                    RowMode::None => tasty_ui_widgets::KbPluginSlot::Unassigned,
                },
            },
        )
        .collect();

    let out = tasty_ui_widgets::kb_plugins_subtab(
        ui,
        &th,
        "plugin_shortcuts",
        tasty_ui_widgets::KbPluginsView {
            plugins: &names,
            selected: selected_index,
            rows: views,
            labels: tasty_ui_widgets::KbPluginLabels {
                plugin: t("settings.keybindings.plugins.plugin_label"),
                empty: t("settings.keybindings.plugins.no_plugins_with_commands"),
                modes: [
                    t("settings.keybindings.plugins.mode_inherit"),
                    t("settings.keybindings.plugins.mode_custom"),
                    t("settings.keybindings.plugins.mode_none"),
                ],
                unassigned: t("settings.keybindings.plugins.mode_none_label"),
                reset: t("settings.keybindings.plugins.reset_button"),
                reset_hint: t("settings.keybindings.plugins.reset_hint"),
                key_placeholder: "ctrl+f5",
                draft_hint: t("settings.keybindings.plugins.draft_hint"),
                unrecognized: t("settings.keybindings.plugins.unrecognized_key"),
                // 녹화 버튼 slot 은 본체가 쓰지 않는다(갤러리 대안 견본).
                record: "",
            },
        },
    );

    if let Some(i) = out.plugin
        && let Some((id, _)) = plugin_ids.get(i)
    {
        *selected = Some(id.to_string());
    }
    for (((row, state), buf), o) in rows.iter().zip(&states).zip(&bufs).zip(&out.rows) {
        apply_row_output(row, state, buf, o, draft);
    }
}

/// 명령 한 행의 표시값. 초안이 있으면 저장된 override보다 우선한다.
struct RowState {
    mode: RowMode,
    current: Option<ShortcutOverride>,
    /// 매니페스트가 inherit를 선언했으면 그 source.
    manifest_inherit_source: Option<String>,
    dirty: bool,
    overridden: bool,
    /// 상속 소스 후보. 현재 source 가 화이트리스트에 없으면 뒤에 붙인다.
    sources: Vec<String>,
    source_index: usize,
    caption: String,
    keys: String,
    /// 해석하지 못한 첫 키.
    error: Option<String>,
}

impl RowState {
    fn new(
        row: &PluginShortcutRow,
        draft: &std::collections::BTreeMap<(String, String), Option<ShortcutOverride>>,
        host_kb: &KeybindingSettings,
        general: &GeneralSettings,
    ) -> Self {
        let key = (row.plugin_id.clone(), row.command_id.clone());
        let current: Option<ShortcutOverride> = match draft.get(&key) {
            Some(o) => o.clone(),
            None => row.current_override.clone(),
        };
        let mode = row_mode_of(current.as_ref(), &row.binding_mode);
        let manifest_inherit_source = match &row.binding_mode {
            BindingMode::InheritHost(s) => Some(s.clone()),
            BindingMode::Independent => None,
        };

        let active_source: String = match &current {
            Some(ShortcutOverride::Inherit { source }) => source.clone(),
            _ => manifest_inherit_source
                .clone()
                .unwrap_or_else(|| host_actions::INHERITABLE_HOST_ACTIONS[0].to_string()),
        };
        let mut sources: Vec<String> = host_actions::INHERITABLE_HOST_ACTIONS
            .iter()
            .map(|s| s.to_string())
            .collect();
        let source_index = match sources.iter().position(|s| *s == active_source) {
            Some(i) => i,
            None => {
                sources.push(active_source.clone());
                sources.len() - 1
            }
        };
        let resolved = host_actions::host_action_for(host_kb, &active_source)
            .map(|v| {
                v.iter()
                    .map(|s| KeybindingSettings::format_display(s, general))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        let caption = if resolved.is_empty() {
            t("settings.keybindings.hint_none").to_string()
        } else {
            t_fmt("settings.keybindings.plugins.inherited", &resolved)
        };

        let keys = match &current {
            Some(ShortcutOverride::Key { value }) => value.join(", "),
            _ => row.manifest_default.clone().unwrap_or_default(),
        };
        let error = parse_keys(&keys)
            .into_iter()
            .find(|k| !tasty_key_match::binding_key_recognized(k));

        Self {
            mode,
            dirty: draft.contains_key(&key),
            overridden: current.is_some(),
            current,
            manifest_inherit_source,
            sources,
            source_index,
            caption,
            keys,
            error,
        }
    }
}

/// 쉼표로 여러 키를 받고 앞뒤 공백과 빈 칸은 버린다.
fn parse_keys(buf: &str) -> Vec<String> {
    buf.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn apply_row_output(
    row: &PluginShortcutRow,
    state: &RowState,
    buf: &str,
    out: &tasty_ui_widgets::KbPluginRowOutput,
    draft: &mut std::collections::BTreeMap<(String, String), Option<ShortcutOverride>>,
) {
    if let Some(m) = out.mode {
        let new_mode = match m {
            tasty_ui_widgets::KB_PLUGIN_MODE_INHERIT => RowMode::Inherit,
            tasty_ui_widgets::KB_PLUGIN_MODE_CUSTOM => RowMode::Custom,
            _ => RowMode::None,
        };
        apply_mode_change(
            row,
            draft,
            state.current.as_ref(),
            new_mode,
            state.manifest_inherit_source.as_deref(),
        );
    }
    if let Some(i) = out.source
        && let Some(source) = state.sources.get(i)
    {
        commit_row_change(
            draft,
            row,
            Some(ShortcutOverride::Inherit {
                source: source.clone(),
            }),
        );
    }
    if out.keys_changed {
        commit_row_change(
            draft,
            row,
            Some(ShortcutOverride::Key {
                value: parse_keys(buf),
            }),
        );
    }
    // 매니페스트 default로 복귀 — 저장된 override가 있으면 None을 초안에 넣어
    // main이 clear_shortcut_override를 호출하게 하고, 없으면 초안만 지운다.
    if out.reset {
        let key = (row.plugin_id.clone(), row.command_id.clone());
        if row.current_override.is_some() {
            draft.insert(key, None);
        } else {
            draft.remove(&key);
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn row(binding_mode: BindingMode, current: Option<ShortcutOverride>) -> PluginShortcutRow {
        PluginShortcutRow {
            plugin_id: "com.example.clip".into(),
            plugin_name: "Clipboard Viewer".into(),
            command_id: "open".into(),
            title_i18n_key: "open".into(),
            binding_mode,
            manifest_default: Some("ctrl+shift+h".into()),
            current_override: current,
        }
    }

    fn state(
        r: &PluginShortcutRow,
        draft: &std::collections::BTreeMap<(String, String), Option<ShortcutOverride>>,
    ) -> RowState {
        RowState::new(
            r,
            draft,
            &KeybindingSettings::default(),
            &GeneralSettings::default(),
        )
    }

    fn key(value: &[&str]) -> Option<ShortcutOverride> {
        Some(ShortcutOverride::Key {
            value: value.iter().map(|s| s.to_string()).collect(),
        })
    }

    /// 매니페스트 기본값만 있으면 Reset 은 disabled, 초안 점도 없다.
    #[test]
    fn manifest_default_is_neither_overridden_nor_dirty() {
        let r = row(BindingMode::Independent, None);
        let s = state(&r, &Default::default());
        assert_eq!(s.mode, RowMode::Custom);
        assert_eq!(s.keys, "ctrl+shift+h");
        assert!(!s.overridden && !s.dirty && s.error.is_none());
    }

    /// 저장된 override 는 Reset 을 켜지만 초안 점은 저장값과 다를 때만 선다.
    #[test]
    fn saved_override_enables_reset_and_a_draft_adds_the_dot() {
        let r = row(BindingMode::Independent, key(&["ctrl+alt+x"]));
        let s = state(&r, &Default::default());
        assert!(s.overridden && !s.dirty);

        let mut draft = std::collections::BTreeMap::new();
        commit_row_change(&mut draft, &r, Some(ShortcutOverride::None));
        let s = state(&r, &draft);
        assert_eq!(s.mode, RowMode::None);
        assert!(s.overridden && s.dirty);

        // 저장값으로 되돌리면 초안 항목이 사라진다.
        commit_row_change(&mut draft, &r, key(&["ctrl+alt+x"]));
        assert!(!state(&r, &draft).dirty);
    }

    /// 해석하지 못한 첫 키를 caption 에 싣는다. 쉼표 목록의 나머지는 보지 않는다.
    #[test]
    fn the_first_unrecognized_key_becomes_the_error() {
        let r = row(
            BindingMode::Independent,
            key(&["ctrl+f5", "ctrl+shft+h", "cmd+k"]),
        );
        assert_eq!(
            state(&r, &Default::default()).error.as_deref(),
            Some("ctrl+shft+h")
        );
        let r = row(BindingMode::Independent, key(&["ctrl+f5", "alt+escape"]));
        assert!(state(&r, &Default::default()).error.is_none());
        // 비운 입력은 오류가 아니다.
        let r = row(BindingMode::Independent, key(&[]));
        assert!(state(&r, &Default::default()).error.is_none());
    }

    /// 상속 source 는 화이트리스트에서 고르고, 목록에 없는 값이면 뒤에 붙여 그대로 보인다.
    #[test]
    fn inherit_source_is_selected_or_appended() {
        let r = row(BindingMode::InheritHost("clipboard.paste".into()), None);
        let s = state(&r, &Default::default());
        assert_eq!(s.mode, RowMode::Inherit);
        assert_eq!(s.sources[s.source_index], "clipboard.paste");
        assert_eq!(
            s.sources.len(),
            host_actions::INHERITABLE_HOST_ACTIONS.len()
        );

        let r = row(
            BindingMode::Independent,
            Some(ShortcutOverride::Inherit {
                source: "not.listed".into(),
            }),
        );
        let s = state(&r, &Default::default());
        assert_eq!(s.sources[s.source_index], "not.listed");
    }

    fn output() -> tasty_ui_widgets::KbPluginRowOutput {
        tasty_ui_widgets::KbPluginRowOutput::default()
    }

    fn entry(
        draft: &std::collections::BTreeMap<(String, String), Option<ShortcutOverride>>,
        r: &PluginShortcutRow,
    ) -> Option<Option<ShortcutOverride>> {
        draft
            .get(&(r.plugin_id.clone(), r.command_id.clone()))
            .cloned()
    }

    /// mode 출력은 시작값을 고른 override 로, source 출력은 고른 소스의 Inherit 로 초안에 쓴다.
    #[test]
    fn mode_and_source_outputs_write_the_draft() {
        let r = row(BindingMode::Independent, None);
        let mut draft = std::collections::BTreeMap::new();
        let s = state(&r, &draft);
        let o = tasty_ui_widgets::KbPluginRowOutput {
            mode: Some(tasty_ui_widgets::KB_PLUGIN_MODE_INHERIT),
            ..output()
        };
        apply_row_output(&r, &s, &s.keys, &o, &mut draft);
        assert_eq!(
            entry(&draft, &r),
            Some(Some(ShortcutOverride::Inherit {
                source: host_actions::INHERITABLE_HOST_ACTIONS[0].to_string()
            }))
        );

        let s = state(&r, &draft);
        let o = tasty_ui_widgets::KbPluginRowOutput {
            source: Some(1),
            ..output()
        };
        apply_row_output(&r, &s, &s.keys, &o, &mut draft);
        assert_eq!(
            entry(&draft, &r),
            Some(Some(ShortcutOverride::Inherit {
                source: s.sources[1].clone()
            }))
        );

        let s = state(&r, &draft);
        let o = tasty_ui_widgets::KbPluginRowOutput {
            mode: Some(tasty_ui_widgets::KB_PLUGIN_MODE_NONE),
            ..output()
        };
        apply_row_output(&r, &s, &s.keys, &o, &mut draft);
        assert_eq!(entry(&draft, &r), Some(Some(ShortcutOverride::None)));
    }

    /// 입력칸 변경은 쉼표로 나눈 키를 초안에 쓰고, 저장값과 같아지면 초안 항목을 지운다.
    #[test]
    fn typed_keys_write_the_draft_and_matching_the_saved_value_clears_it() {
        let r = row(BindingMode::Independent, key(&["ctrl+alt+x"]));
        let mut draft = std::collections::BTreeMap::new();
        let s = state(&r, &draft);
        let o = tasty_ui_widgets::KbPluginRowOutput {
            keys_changed: true,
            ..output()
        };
        apply_row_output(&r, &s, " ctrl+a, ,ctrl+b ", &o, &mut draft);
        assert_eq!(entry(&draft, &r), Some(key(&["ctrl+a", "ctrl+b"])));

        apply_row_output(&r, &s, "ctrl+alt+x", &o, &mut draft);
        assert_eq!(entry(&draft, &r), None);

        // 다 지우면 빈 Key override 다(매니페스트 기본값으로 되돌리는 것은 Reset 이다).
        apply_row_output(&r, &s, "", &o, &mut draft);
        assert_eq!(entry(&draft, &r), Some(key(&[])));
    }

    /// Reset 은 저장된 override 가 있으면 지움(None)을 초안에 넣고, 없으면 초안만 지운다.
    #[test]
    fn reset_clears_a_saved_override_or_drops_the_draft() {
        let o = tasty_ui_widgets::KbPluginRowOutput {
            reset: true,
            ..output()
        };

        let saved = row(BindingMode::Independent, key(&["ctrl+alt+x"]));
        let mut draft = std::collections::BTreeMap::new();
        let s = state(&saved, &draft);
        apply_row_output(&saved, &s, &s.keys, &o, &mut draft);
        assert_eq!(entry(&draft, &saved), Some(None));

        let unsaved = row(BindingMode::Independent, None);
        let mut draft = std::collections::BTreeMap::new();
        commit_row_change(&mut draft, &unsaved, key(&["ctrl+q"]));
        let s = state(&unsaved, &draft);
        apply_row_output(&unsaved, &s, &s.keys, &o, &mut draft);
        assert_eq!(entry(&draft, &unsaved), None);
    }
}
