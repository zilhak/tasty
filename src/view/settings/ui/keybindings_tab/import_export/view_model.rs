//! 미리보기와 현재 초안을 비교해 화면에 필요한 값과 문구를 만든다. 상태는 변경하지 않는다.

use tasty_host_plugin::keybinding_bundle::PluginShortcutOverrides;

use tasty_settings::keybindings::os_keys::ReservedOs;

use crate::i18n::{t, t_count, t_fmt, t_fmt2};
use crate::settings::KeybindingSettings;

use super::bundle_notices::BundleNotice;
use super::labels::{Labels, trim_label};
use super::model::{Group, RowKey, override_of, row_changed, row_keys};
use super::{ImportExportState, Preview};

/// 경고 한 줄의 문구. 알 수 없는 액션이 하나면 단수형 키를 사용한다.
fn notice_line(notice: &BundleNotice) -> String {
    match notice {
        BundleNotice::NewerSchema { found, known } => t_fmt2(
            "settings.keybindings.ie_notice_newer_schema",
            &found.to_string(),
            &known.to_string(),
        ),
        BundleNotice::UnknownActions(names) => t_count(
            "settings.keybindings.ie_notice_unknown_actions",
            names.len() as u64,
            &[&names.len().to_string(), &names.join(", ")],
        ),
        BundleNotice::OsReserved { os, combos } => {
            // 예약 목록은 option 조합뿐이고 Windows·Linux 의 option 표기는 표시 설정과 무관하다.
            let general = tasty_settings::GeneralSettings::default();
            let shown: Vec<String> = combos
                .iter()
                .map(|c| KeybindingSettings::format_display(c, &general))
                .collect();
            let key = match os {
                ReservedOs::Windows => "keys.os_reserved.windows",
                ReservedOs::Linux => "keys.os_reserved.linux",
            };
            t_fmt(key, &shown.join(", "))
        }
    }
}

pub(super) enum Sub {
    Plugin(String),
    Note(String),
}

pub(super) struct RowView {
    pub(super) key: RowKey,
    pub(super) action: String,
    pub(super) sub: Option<Sub>,
    pub(super) cur: String,
    pub(super) next: String,
    pub(super) changed: bool,
}

pub(super) struct GroupView {
    pub(super) group: Group,
    pub(super) rows: Vec<RowView>,
}

pub(super) struct ViewModel {
    pub(super) groups: Vec<GroupView>,
    pub(super) total: usize,
    pub(super) picked: usize,
    pub(super) intro: String,
    pub(super) dropped: Option<String>,
    /// 경고 블록의 줄(고정 순서).
    pub(super) notices: Vec<String>,
}

pub(super) fn build_view_model(
    preview: &Preview,
    state: &ImportExportState,
    current: &KeybindingSettings,
    current_overrides: &PluginShortcutOverrides,
    labels: &Labels<'_>,
) -> ViewModel {
    let (imported, imported_ov) = (&preview.keybindings, &preview.overrides);

    let mut groups: Vec<GroupView> = Group::ALL
        .into_iter()
        .map(|group| GroupView {
            group,
            rows: Vec::new(),
        })
        .collect();
    for key in row_keys(current, imported, imported_ov) {
        let changed = row_changed(&key, current, current_overrides, imported, imported_ov);
        let (action, sub, cur, next) = match &key {
            RowKey::General(field) => (
                trim_label(KeybindingSettings::label_key_for(field).map_or(*field, t)),
                None,
                labels.bindings(current.get_bindings(field).unwrap_or(&[])),
                labels.bindings(imported.get_bindings(field).unwrap_or(&[])),
            ),
            RowKey::DragFlip => (
                trim_label(t("settings.keybindings.explorer_drag_flip_modifier_label")),
                None,
                labels.combo(&current.explorer_drag_flip_modifier),
                labels.combo(&imported.explorer_drag_flip_modifier),
            ),
            RowKey::Axis(axis) => {
                let action_key = match axis {
                    crate::settings::SwitchAxis::Tab => "settings.keybindings.ie_axis_tab",
                    crate::settings::SwitchAxis::Workspace => {
                        "settings.keybindings.ie_axis_workspace"
                    }
                    crate::settings::SwitchAxis::Category => {
                        "settings.keybindings.ie_axis_category"
                    }
                };
                (
                    t(action_key).to_string(),
                    Some(Sub::Note(t_count(
                        "settings.keybindings.ie_axis_slots",
                        axis.slot_count() as u64,
                        &[&axis.slot_count().to_string()],
                    ))),
                    labels.axis_summary(*axis, current),
                    labels.axis_summary(*axis, imported),
                )
            }
            RowKey::Script(id) => (
                labels.script_name(id),
                None,
                labels.bindings(
                    &current
                        .script_binding_combo(id)
                        .map(|c| vec![c.to_string()])
                        .unwrap_or_default(),
                ),
                labels.bindings(
                    &imported
                        .script_binding_combo(id)
                        .map(|c| vec![c.to_string()])
                        .unwrap_or_default(),
                ),
            ),
            RowKey::Plugin {
                plugin_id,
                command_id,
            } => (
                labels.command_title(plugin_id, command_id),
                Some(Sub::Plugin(labels.plugin_name(plugin_id))),
                labels.plugin_value(
                    plugin_id,
                    command_id,
                    override_of(current_overrides, plugin_id, command_id),
                ),
                labels.plugin_value(
                    plugin_id,
                    command_id,
                    override_of(imported_ov, plugin_id, command_id),
                ),
            ),
        };
        let group = key.group();
        if let Some(g) = groups.iter_mut().find(|g| g.group == group) {
            g.rows.push(RowView {
                key,
                action,
                sub,
                cur,
                next,
                changed,
            });
        }
    }
    let total: usize = groups.iter().map(|g| g.rows.len()).sum();
    let changed = groups
        .iter()
        .flat_map(|g| &g.rows)
        .filter(|r| r.changed)
        .count();
    let picked = groups
        .iter()
        .flat_map(|g| &g.rows)
        .filter(|r| !state.deselected.contains(&r.key))
        .count();

    let intro = t_count(
        "settings.keybindings.ie_preview_intro",
        total as u64,
        &[
            &preview.file_name,
            &changed.to_string(),
            &total.to_string(),
            &picked.to_string(),
        ],
    );
    let dropped = (!preview.dropped.is_empty()).then(|| {
        let count: usize = preview.dropped.iter().map(|(_, n)| n).sum();
        let list = preview
            .dropped
            .iter()
            .map(|(id, _)| id.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        t_count(
            "settings.keybindings.ie_dropped",
            count as u64,
            &[&count.to_string(), &list],
        )
    });

    let notices = preview.notices.iter().map(notice_line).collect();

    ViewModel {
        groups,
        total,
        picked,
        intro,
        dropped,
        notices,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 액션 수에 맞는 단수·복수 키를 고르는지 확인한다.
    /// 번역과 placeholder 일치는 tests/i18n_key_parity.rs에서 확인한다.
    #[test]
    fn only_exactly_one_unknown_action_takes_the_singular_line() {
        let none = notice_line(&BundleNotice::UnknownActions(Vec::new()));
        let one = notice_line(&BundleNotice::UnknownActions(vec!["tab.pin".into()]));
        let two = notice_line(&BundleNotice::UnknownActions(vec![
            "tab.pin".into(),
            "pane.zoom_cycle".into(),
        ]));
        assert_ne!(one, two, "하나와 둘이 같은 문구를 쓴다");
        assert_ne!(one, none, "빈 목록에 단수형 문구를 사용했다");
    }

    /// OS 예약 줄은 OS 마다 다른 문구를 고르고 조합을 사용자 표기로 채운다.
    #[test]
    fn the_os_reserved_line_names_the_os_and_shows_the_combo() {
        let line = |os| {
            notice_line(&BundleNotice::OsReserved {
                os,
                combos: vec!["option+l".into()],
            })
        };
        let (win, linux) = (line(ReservedOs::Windows), line(ReservedOs::Linux));
        assert_ne!(win, linux);
        for l in [&win, &linux] {
            assert!(l.contains("+L") && !l.contains("{}"), "{l}");
        }
    }
}
