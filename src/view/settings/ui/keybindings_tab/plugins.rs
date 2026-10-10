use crate::i18n::{t, t_fmt};
use crate::plugin::manifest::BindingMode;
use crate::plugin::registry_state::ShortcutOverride;
use crate::plugin_bridge::host_actions;
use crate::settings::{GeneralSettings, KeybindingSettings};
use crate::settings_ui::{PluginShortcutRow, PluginShortcutSnapshot};

use super::{FieldKind, KeyCapture, PendingBinding, RecordingSlot};

/// `RecordingSlot.field_id` 가 이 접두사면 plugin 명령의 단축키 슬롯이다. 뒤에
/// `<plugin id>/<command id>` 가 온다. 두 id 모두 매니페스트 검증상 `/` 를 쓸 수 없어
/// 첫 `/` 에서 나누면 되돌릴 수 있다. 설정 필드명·`script:`·가져오기 마이그레이션 id 와 겹치지 않는다.
const PLUGIN_SLOT_PREFIX: &str = "plugin:";

fn plugin_slot_id(plugin_id: &str, command_id: &str) -> String {
    format!("{PLUGIN_SLOT_PREFIX}{plugin_id}/{command_id}")
}

fn parse_plugin_slot_id(field_id: &str) -> Option<(&str, &str)> {
    field_id.strip_prefix(PLUGIN_SLOT_PREFIX)?.split_once('/')
}

/// 녹화 결과를 Custom 키 목록에 반영한다. 바뀌지 않으면 `None`.
/// 기존 슬롯의 조합은 교체하고 `idx == len` 이면 뒤에 붙인다. `Esc` 는 기존 슬롯이면 지우고
/// 새 슬롯이면 아무것도 바꾸지 않는다(녹화만 취소).
fn keys_after_capture(keys: &[String], idx: usize, captured: &KeyCapture) -> Option<Vec<String>> {
    let mut next = keys.to_vec();
    match captured {
        KeyCapture::Combo(combo) if idx < next.len() => next[idx] = combo.clone(),
        KeyCapture::Combo(combo) => next.push(combo.clone()),
        KeyCapture::Clear if idx < next.len() => {
            next.remove(idx);
        }
        KeyCapture::Clear | KeyCapture::None => return None,
    }
    Some(next)
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

/// 이 서브탭의 녹화 상태. 녹화 슬롯은 설정 창에 하나뿐이라 다른 서브탭과 함께 쓴다.
pub(super) struct PluginRecording<'a> {
    pub slot: &'a mut Option<RecordingSlot>,
    /// 다른 서브탭의 충돌 확인 popup. 떠 있는 동안은 녹화를 시작하지 않는다.
    pub pending: &'a Option<PendingBinding>,
    pub captured: &'a KeyCapture,
}

/// 녹화 중인 plugin 슬롯이 있으면 캡처를 그 명령의 Custom 초안에 쓰고 녹화를 끝낸다.
fn consume_capture(
    snapshot: &PluginShortcutSnapshot,
    draft: &mut std::collections::BTreeMap<(String, String), Option<ShortcutOverride>>,
    recording: &mut Option<RecordingSlot>,
    captured: &KeyCapture,
) {
    let Some(slot) = recording.as_ref() else {
        return;
    };
    let Some((plugin_id, command_id)) = parse_plugin_slot_id(&slot.field_id) else {
        return;
    };
    if matches!(captured, KeyCapture::None) {
        return;
    }
    let row = snapshot
        .rows
        .iter()
        .find(|r| r.plugin_id == plugin_id && r.command_id == command_id);
    if let Some(row) = row {
        let keys = custom_keys(row, draft);
        if let Some(next) = keys_after_capture(&keys, slot.idx, captured) {
            commit_row_change(draft, row, Some(ShortcutOverride::Key { value: next }));
        }
    }
    *recording = None;
}

/// 행의 현재 Custom 키 — 초안·저장값의 `Key`, 없으면 매니페스트 기본값.
fn custom_keys(
    row: &PluginShortcutRow,
    draft: &std::collections::BTreeMap<(String, String), Option<ShortcutOverride>>,
) -> Vec<String> {
    let key = (row.plugin_id.clone(), row.command_id.clone());
    let current = match draft.get(&key) {
        Some(o) => o.as_ref(),
        None => row.current_override.as_ref(),
    };
    match current {
        Some(ShortcutOverride::Key { value }) => value.clone(),
        _ => row
            .manifest_default
            .iter()
            .filter(|s| !s.is_empty())
            .cloned()
            .collect(),
    }
}

/// plugin 명령의 단축키 설정. 화면은 공용 view(`tasty_ui_widgets::kb_plugins_subtab`)가 그리고
/// 여기서는 초안·override 를 표시값으로 풀고 사용자가 바꾼 것을 초안에 반영한다.
/// Custom 키는 다른 서브탭과 같은 녹화 슬롯으로 받는다.
pub(super) fn draw_plugins_subtab(
    ui: &mut egui::Ui,
    snapshot: &PluginShortcutSnapshot,
    selected: &mut Option<String>,
    draft: &mut std::collections::BTreeMap<(String, String), Option<ShortcutOverride>>,
    host_kb: &KeybindingSettings,
    general: &GeneralSettings,
    recording: PluginRecording<'_>,
) {
    let th = crate::theme::theme();
    let PluginRecording {
        slot: recording,
        pending,
        captured,
    } = recording;
    consume_capture(snapshot, draft, recording, captured);
    let can_record = pending.is_none();

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
    let displays: Vec<Vec<String>> = states
        .iter()
        .map(|s| {
            s.keys
                .iter()
                .map(|k| KeybindingSettings::format_display(k, general))
                .collect()
        })
        .collect();
    let display_refs: Vec<Vec<&str>> = displays
        .iter()
        .map(|d| d.iter().map(String::as_str).collect())
        .collect();
    let recording_idx: Vec<Option<usize>> = rows
        .iter()
        .map(|row| {
            recording.as_ref().and_then(|slot| {
                (parse_plugin_slot_id(&slot.field_id)
                    == Some((row.plugin_id.as_str(), row.command_id.as_str())))
                .then_some(slot.idx)
            })
        })
        .collect();
    let sources: Vec<Vec<&str>> = states
        .iter()
        .map(|s| s.sources.iter().map(String::as_str).collect())
        .collect();

    let views = rows
        .iter()
        .zip(&states)
        .zip(&display_refs)
        .zip(&sources)
        .zip(&recording_idx)
        .map(
            |((((row, state), keys), sources), rec)| tasty_ui_widgets::KbPluginRowView {
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
                        keys,
                        recording: *rec,
                        can_record,
                        problem: state.problem(),
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
                draft_hint: t("settings.keybindings.plugins.draft_hint"),
                unrecognized: t("settings.keybindings.plugins.unrecognized_key"),
                os_key_name: t("keys.os_key_name"),
                press_key: t("settings.keybindings.hint_press_key"),
                no_key: t("settings.keybindings.hint_none"),
                add_hint: t("settings.keybindings.add_binding_button"),
            },
        },
    );

    if let Some(i) = out.plugin
        && let Some((id, _)) = plugin_ids.get(i)
    {
        *selected = Some(id.to_string());
    }
    for (row, (state, o)) in rows.iter().zip(states.iter().zip(&out.rows)) {
        apply_row_output(row, state, o, draft);
        if let Some(idx) = o.record {
            *recording = Some(RecordingSlot {
                field_id: plugin_slot_id(&row.plugin_id, &row.command_id),
                idx,
                field_kind: FieldKind::Combo,
            });
        }
    }
    if cancels_recording(&out) {
        cancel_plugin_recording(recording);
    }
}

/// 플러그인·mode·Reset 이 바뀌면 녹화 중인 슬롯이 사라지거나 다른 값을 가리키게 된다.
/// 그대로 두면 다음 캡처가 보이지 않는 행에 들어간다.
fn cancels_recording(out: &tasty_ui_widgets::KbPluginsOutput) -> bool {
    out.plugin.is_some() || out.rows.iter().any(|o| o.mode.is_some() || o.reset)
}

fn cancel_plugin_recording(recording: &mut Option<RecordingSlot>) {
    if recording
        .as_ref()
        .is_some_and(|s| parse_plugin_slot_id(&s.field_id).is_some())
    {
        *recording = None;
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
    /// Custom 키. 초안·저장값의 `Key`, 없으면 매니페스트 기본값.
    keys: Vec<String>,
    /// 해석하지 못하는 첫 키의 위치와 이유.
    error: Option<(usize, tasty_key_match::TextKeyProblem)>,
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

        let keys = custom_keys(row, draft);
        let error = tasty_key_match::first_text_key_problem(&keys);

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

impl RowState {
    /// 위젯에 넘길 키 문제. 슬롯은 표시 문자열 대신 설정 원문을 보인다.
    fn problem(&self) -> Option<tasty_ui_widgets::KbPluginKeyProblem<'_>> {
        use tasty_key_match::TextKeyProblem;
        use tasty_ui_widgets::KbPluginKeyProblem;
        let (slot, why) = self.error?;
        let raw = self.keys.get(slot)?.as_str();
        Some(match why {
            TextKeyProblem::OsKeyName => KbPluginKeyProblem::OsKeyName { slot, raw },
            TextKeyProblem::Unrecognized => KbPluginKeyProblem::Unrecognized { slot, raw },
        })
    }
}

fn apply_row_output(
    row: &PluginShortcutRow,
    state: &RowState,
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
        assert_eq!(s.keys, ["ctrl+shift+h"]);
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

    /// 해석하지 못한 첫 키를 caption 에 싣는다. 뒤의 키는 보지 않는다.
    /// OS 키 이름을 적은 키는 일반 해석 실패와 다른 caption 을 고른다.
    #[test]
    fn the_first_unrecognized_key_becomes_the_error() {
        use tasty_ui_widgets::KbPluginKeyProblem;
        let r = row(
            BindingMode::Independent,
            key(&["ctrl+f5", "ctrl+shft+h", "cmd+k"]),
        );
        assert_eq!(
            state(&r, &Default::default()).problem(),
            Some(KbPluginKeyProblem::Unrecognized {
                slot: 1,
                raw: "ctrl+shft+h"
            })
        );
        let r = row(BindingMode::Independent, key(&["ctrl+f5", "cmd+k"]));
        assert_eq!(
            state(&r, &Default::default()).problem(),
            Some(KbPluginKeyProblem::OsKeyName {
                slot: 1,
                raw: "cmd+k"
            })
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
        apply_row_output(&r, &s, &o, &mut draft);
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
        apply_row_output(&r, &s, &o, &mut draft);
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
        apply_row_output(&r, &s, &o, &mut draft);
        assert_eq!(entry(&draft, &r), Some(Some(ShortcutOverride::None)));
    }

    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    /// 기존 슬롯의 캡처는 그 키를 바꾸고, 키 수와 같은 슬롯의 캡처는 뒤에 붙인다.
    #[test]
    fn a_combo_replaces_its_slot_or_appends_at_the_end() {
        let keys = strings(&["ctrl+shift+h"]);
        let combo = KeyCapture::Combo("ctrl+alt+j".into());
        assert_eq!(
            keys_after_capture(&keys, 0, &combo),
            Some(strings(&["ctrl+alt+j"]))
        );
        assert_eq!(
            keys_after_capture(&keys, 1, &combo),
            Some(strings(&["ctrl+shift+h", "ctrl+alt+j"]))
        );
        assert_eq!(keys_after_capture(&keys, 0, &KeyCapture::None), None);
    }

    /// `Esc` 는 기존 슬롯이면 지우고 새 슬롯이면 아무것도 바꾸지 않는다.
    #[test]
    fn escape_removes_an_existing_slot_and_cancels_a_new_one() {
        let keys = strings(&["ctrl+a", "ctrl+b"]);
        assert_eq!(
            keys_after_capture(&keys, 0, &KeyCapture::Clear),
            Some(strings(&["ctrl+b"]))
        );
        assert_eq!(keys_after_capture(&keys, 2, &KeyCapture::Clear), None);
        // 마지막 키를 지우면 빈 Key 다 — 그 명령은 단축키가 없다.
        assert_eq!(
            keys_after_capture(&strings(&["ctrl+a"]), 0, &KeyCapture::Clear),
            Some(Vec::new())
        );
    }

    /// 슬롯 id 는 plugin id 와 명령 id 로 되돌아오고 다른 녹화 id 와 겹치지 않는다.
    #[test]
    fn plugin_slot_id_round_trips_and_stays_apart_from_other_slots() {
        let id = plugin_slot_id("com.tasty.clipboard-viewer", "clipboard.open_viewer");
        assert_eq!(
            parse_plugin_slot_id(&id),
            Some(("com.tasty.clipboard-viewer", "clipboard.open_viewer"))
        );
        for other in [
            "toggle_sidebar",
            "script:abc",
            "__keybindings_import_migration",
        ] {
            assert_eq!(parse_plugin_slot_id(other), None, "{other}");
        }
    }

    fn snapshot(rows: Vec<PluginShortcutRow>) -> PluginShortcutSnapshot {
        PluginShortcutSnapshot { rows }
    }

    fn recording_of(r: &PluginShortcutRow, idx: usize) -> Option<RecordingSlot> {
        Some(RecordingSlot {
            field_id: plugin_slot_id(&r.plugin_id, &r.command_id),
            idx,
            field_kind: FieldKind::Combo,
        })
    }

    /// 녹화한 조합은 그 명령의 Custom 초안에 들어가고 녹화가 끝난다. 다른 명령은 그대로다.
    #[test]
    fn a_capture_writes_only_the_recorded_command() {
        let r = row(BindingMode::Independent, None);
        let mut other = row(BindingMode::Independent, None);
        other.command_id = "other".into();
        let snap = snapshot(vec![r.clone(), other.clone()]);
        let mut draft = std::collections::BTreeMap::new();
        let mut rec = recording_of(&r, 0);
        consume_capture(
            &snap,
            &mut draft,
            &mut rec,
            &KeyCapture::Combo("ctrl+alt+j".into()),
        );
        assert!(rec.is_none());
        assert_eq!(entry(&draft, &r), Some(key(&["ctrl+alt+j"])));
        assert_eq!(entry(&draft, &other), None);

        // 아직 아무 키도 누르지 않았으면 녹화가 이어진다.
        let mut rec = recording_of(&r, 0);
        consume_capture(&snap, &mut draft, &mut rec, &KeyCapture::None);
        assert!(rec.is_some());
    }

    /// 다른 서브탭의 녹화 슬롯 캡처는 건드리지 않는다.
    #[test]
    fn a_capture_for_another_slot_is_left_alone() {
        let r = row(BindingMode::Independent, None);
        let snap = snapshot(vec![r.clone()]);
        let mut draft = std::collections::BTreeMap::new();
        let mut rec = Some(RecordingSlot {
            field_id: "toggle_sidebar".into(),
            idx: 0,
            field_kind: FieldKind::Combo,
        });
        consume_capture(
            &snap,
            &mut draft,
            &mut rec,
            &KeyCapture::Combo("ctrl+alt+j".into()),
        );
        assert!(rec.is_some() && draft.is_empty());
    }

    /// 플러그인·mode·Reset 변경은 진행 중인 plugin 녹화만 취소한다.
    #[test]
    fn changing_plugin_mode_or_reset_cancels_plugin_recording() {
        let r = row(BindingMode::Independent, None);
        let changed = |o: tasty_ui_widgets::KbPluginRowOutput, plugin: Option<usize>| {
            tasty_ui_widgets::KbPluginsOutput {
                plugin,
                rows: vec![o],
                picker: None,
            }
        };
        let cases = [
            changed(output(), Some(1)),
            changed(
                tasty_ui_widgets::KbPluginRowOutput {
                    mode: Some(tasty_ui_widgets::KB_PLUGIN_MODE_NONE),
                    ..output()
                },
                None,
            ),
            changed(
                tasty_ui_widgets::KbPluginRowOutput {
                    reset: true,
                    ..output()
                },
                None,
            ),
        ];
        for out in &cases {
            assert!(cancels_recording(out));
            let mut rec = recording_of(&r, 0);
            cancel_plugin_recording(&mut rec);
            assert!(rec.is_none());
        }
        assert!(!cancels_recording(&changed(output(), None)));
        // 다른 서브탭의 녹화는 남는다.
        let mut rec = Some(RecordingSlot {
            field_id: "script:abc".into(),
            idx: 0,
            field_kind: FieldKind::Combo,
        });
        cancel_plugin_recording(&mut rec);
        assert!(rec.is_some());
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
        apply_row_output(&saved, &s, &o, &mut draft);
        assert_eq!(entry(&draft, &saved), Some(None));

        let unsaved = row(BindingMode::Independent, None);
        let mut draft = std::collections::BTreeMap::new();
        commit_row_change(&mut draft, &unsaved, key(&["ctrl+q"]));
        let s = state(&unsaved, &draft);
        apply_row_output(&unsaved, &s, &o, &mut draft);
        assert_eq!(entry(&draft, &unsaved), None);
    }
}
