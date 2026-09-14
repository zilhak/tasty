//! 한 프레임의 표시값 — 미리보기와 현재 draft 를 견줘 표 · 마이그레이션 카드 · 안내문이 그릴
//! 문자열과 판정을 미리 만든다.
//!
//! 경계: 그리기 함수들이 **읽기만** 하는 계산 결과다. `egui` 를 부르지 않고 상태를 바꾸지 않는다.

use std::collections::BTreeSet;

use tasty_host_plugin::keybinding_bundle::PluginShortcutOverrides;
use tasty_host_plugin::keybinding_bundle::option_migration::{
    BindingConflict, BindingSite, ReplacementKind, introduced_conflicts, preview_resolution,
};

use crate::i18n::{t, t_args, t_fmt, t_fmt2};
use crate::settings::KeybindingSettings;

use super::bundle_notices::BundleNotice;
use super::labels::{Labels, trim_label};
use super::model::{
    Group, MigrationValue, RowKey, override_of, plan_of, row_changed, row_keys, unresolved_count,
};
use super::{ImportExportState, Preview};

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
    pub(super) blocked: bool,
}

pub(super) struct GroupView {
    pub(super) group: Group,
    pub(super) rows: Vec<RowView>,
}

pub(super) struct MigrationView {
    pub(super) action: String,
    pub(super) from: String,
    pub(super) kind: ReplacementKind,
    pub(super) conflict: Option<String>,
    pub(super) fanout: Option<String>,
}

pub(super) struct ViewModel {
    pub(super) groups: Vec<GroupView>,
    pub(super) total: usize,
    pub(super) picked: usize,
    pub(super) unresolved: usize,
    pub(super) migration: Vec<MigrationView>,
    pub(super) intro: String,
    pub(super) dropped: Option<String>,
    /// 경고 블록의 줄(고정 순서).
    pub(super) notices: Vec<String>,
    /// 충돌 부제가 붙은 마이그레이션 행 수 — 카드의 개수 줄이 쓴다.
    pub(super) conflicts: usize,
}

pub(super) fn build_view_model(
    preview: &Preview,
    state: &ImportExportState,
    current: &KeybindingSettings,
    current_overrides: &PluginShortcutOverrides,
    labels: &Labels<'_>,
) -> ViewModel {
    let plan = plan_of(&preview.migration);
    let (imported, imported_ov) = if preview.migration.is_empty() {
        (preview.keybindings.clone(), preview.overrides.clone())
    } else {
        preview_resolution(&preview.keybindings, &preview.overrides, &plan)
    };
    let blocked: BTreeSet<RowKey> = preview
        .migration
        .iter()
        .filter(|r| r.value == MigrationValue::Unset)
        .map(|r| RowKey::of_site(&r.site))
        .collect();

    let mut groups: Vec<GroupView> = Group::ALL
        .into_iter()
        .map(|group| GroupView {
            group,
            rows: Vec::new(),
        })
        .collect();
    for key in row_keys(current, &imported, &imported_ov) {
        let changed = row_changed(&key, current, current_overrides, &imported, &imported_ov);
        let is_blocked = blocked.contains(&key);
        let (action, sub, cur, next) = match &key {
            RowKey::General(field) => (
                trim_label(KeybindingSettings::label_key_for(field).map_or(*field, t)),
                None,
                labels.bindings(current.get_bindings(field).unwrap_or(&[])),
                labels.bindings(imported.get_bindings(field).unwrap_or(&[])),
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
                let note_key = if is_blocked {
                    "settings.keybindings.ie_axis_slots_blocked"
                } else {
                    "settings.keybindings.ie_axis_slots"
                };
                (
                    t(action_key).to_string(),
                    Some(Sub::Note(t_fmt(note_key, &axis.slot_count().to_string()))),
                    labels.axis_summary(*axis, current),
                    labels.axis_summary(*axis, &imported),
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
                    override_of(&imported_ov, plugin_id, command_id),
                ),
            ),
        };
        let next = if is_blocked {
            t("settings.keybindings.ie_unresolved_value").to_string()
        } else {
            next
        };
        let group = key.group();
        if let Some(g) = groups.iter_mut().find(|g| g.group == group) {
            g.rows.push(RowView {
                key,
                action,
                sub,
                cur,
                next,
                changed: changed || is_blocked,
                blocked: is_blocked,
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

    let conflicts: Vec<BindingConflict> = if preview.migration.is_empty() {
        Vec::new()
    } else {
        introduced_conflicts(&preview.keybindings, &preview.overrides, &plan)
    };
    let migration: Vec<MigrationView> = preview
        .migration
        .iter()
        .map(|r| {
            let conflict = if matches!(r.value, MigrationValue::Set(_)) {
                conflicts
                    .iter()
                    .find_map(|c| {
                        if c.a == r.site {
                            Some(&c.b)
                        } else if c.b == r.site {
                            Some(&c.a)
                        } else {
                            None
                        }
                    })
                    .map(|other| {
                        t_fmt(
                            "settings.keybindings.ie_migrate_conflict",
                            &labels.site(other),
                        )
                    })
            } else {
                None
            };
            let fanout = match &r.site {
                BindingSite::AxisModifier { axis } => Some(t_fmt(
                    "settings.keybindings.ie_axis_fanout",
                    &axis.slot_count().to_string(),
                )),
                _ => None,
            };
            MigrationView {
                action: labels.site(&r.site),
                from: labels.combo(&r.from),
                kind: r.site.replacement_kind(),
                conflict,
                fanout,
            }
        })
        .collect();

    let mut intro = t_args(
        "settings.keybindings.ie_preview_intro",
        &[
            &preview.file_name,
            &changed.to_string(),
            &total.to_string(),
            &picked.to_string(),
        ],
    );
    if preview.migration.is_empty() {
        intro.push_str(t("settings.keybindings.ie_no_migration"));
    }
    let dropped = (!preview.dropped.is_empty()).then(|| {
        let count: usize = preview.dropped.iter().map(|(_, n)| n).sum();
        let list = preview
            .dropped
            .iter()
            .map(|(id, _)| id.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        t_fmt2("settings.keybindings.ie_dropped", &count.to_string(), &list)
    });

    let notices = preview
        .notices
        .iter()
        .map(|n| match n {
            BundleNotice::NewerSchema { found, known } => t_fmt2(
                "settings.keybindings.ie_notice_newer_schema",
                &found.to_string(),
                &known.to_string(),
            ),
            BundleNotice::UnknownActions(names) => t_fmt2(
                "settings.keybindings.ie_notice_unknown_actions",
                &names.len().to_string(),
                &names.join(", "),
            ),
        })
        .collect();
    let conflicts = migration.iter().filter(|m| m.conflict.is_some()).count();

    ViewModel {
        groups,
        total,
        picked,
        unresolved: unresolved_count(&preview.migration),
        migration,
        intro,
        dropped,
        notices,
        conflicts,
    }
}
