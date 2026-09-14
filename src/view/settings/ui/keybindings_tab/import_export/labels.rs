//! 표시 문자열 — 번들의 자리·조합·override 를 사람이 읽는 이름으로 바꾼다.
//!
//! 경계: 설정·스냅샷을 **읽기만** 하는 문자열 계산이다. 그리기(`egui`)와 상태 전이는 여기 없다.

use std::collections::BTreeMap;

use tasty_host_plugin::keybinding_bundle::option_migration::BindingSite;

use crate::i18n::{t, t_fmt};
use crate::plugin::registry_state::ShortcutOverride;
use crate::settings::{GeneralSettings, KeybindingSettings, ScriptRegistry, SwitchStep};
use crate::settings_ui::PluginShortcutSnapshot;

/// 표시 문자열 출처 묶음.
pub(super) struct Labels<'a> {
    pub(super) general: &'a GeneralSettings,
    pub(super) scripts: &'a ScriptRegistry,
    pub(super) snapshot: &'a PluginShortcutSnapshot,
    pub(super) names: &'a BTreeMap<String, String>,
}

pub(super) fn trim_label(s: &str) -> String {
    s.trim_end_matches(':').trim().to_string()
}

impl Labels<'_> {
    pub(super) fn script_name(&self, id: &str) -> String {
        self.scripts
            .get(id)
            .map(|s| s.name.clone())
            .unwrap_or_else(|| id.to_string())
    }

    pub(super) fn plugin_name(&self, id: &str) -> String {
        self.names
            .get(id)
            .cloned()
            .unwrap_or_else(|| id.to_string())
    }

    pub(super) fn command_title(&self, plugin_id: &str, command_id: &str) -> String {
        self.snapshot
            .rows
            .iter()
            .find(|r| r.plugin_id == plugin_id && r.command_id == command_id)
            .map(|r| t(&r.title_i18n_key).to_string())
            .unwrap_or_else(|| command_id.to_string())
    }

    fn manifest_default(&self, plugin_id: &str, command_id: &str) -> Option<&str> {
        self.snapshot
            .rows
            .iter()
            .find(|r| r.plugin_id == plugin_id && r.command_id == command_id)
            .and_then(|r| r.manifest_default.as_deref())
    }

    /// 한 자리의 사람이 읽는 이름 — 충돌 상대 표시와 마이그레이션 행 라벨.
    pub(super) fn site(&self, site: &BindingSite) -> String {
        match site {
            BindingSite::GeneralBinding { field_id, .. } => {
                trim_label(KeybindingSettings::label_key_for(field_id).map_or(*field_id, t))
            }
            BindingSite::AxisModifier { axis } => trim_label(t(axis.modifier_label_key())),
            BindingSite::AxisSlot { axis, index } => {
                let key = match axis {
                    crate::settings::SwitchAxis::Tab => {
                        "settings.keybindings.tab_switch_slot_label"
                    }
                    crate::settings::SwitchAxis::Workspace => {
                        "settings.keybindings.workspace_switch_slot_label"
                    }
                    crate::settings::SwitchAxis::Category => {
                        "settings.keybindings.category_switch_slot_label"
                    }
                };
                trim_label(&t_fmt(key, &(index + 1).to_string()))
            }
            BindingSite::AxisStep { axis, step } => {
                let key = match (axis, step) {
                    (crate::settings::SwitchAxis::Tab, SwitchStep::Next) => {
                        "settings.keybindings.tab_switch_next_label"
                    }
                    (crate::settings::SwitchAxis::Tab, SwitchStep::Prev) => {
                        "settings.keybindings.tab_switch_prev_label"
                    }
                    (crate::settings::SwitchAxis::Workspace, SwitchStep::Next) => {
                        "settings.keybindings.workspace_switch_next_label"
                    }
                    (crate::settings::SwitchAxis::Workspace, SwitchStep::Prev) => {
                        "settings.keybindings.workspace_switch_prev_label"
                    }
                    (crate::settings::SwitchAxis::Category, SwitchStep::Next) => {
                        "settings.keybindings.category_switch_next_label"
                    }
                    (crate::settings::SwitchAxis::Category, SwitchStep::Prev) => {
                        "settings.keybindings.category_switch_prev_label"
                    }
                };
                trim_label(t(key))
            }
            BindingSite::ScriptBinding { script_id } => self.script_name(script_id),
            BindingSite::PluginOverride {
                plugin_id,
                command_id,
                ..
            } => self.command_title(plugin_id, command_id),
        }
    }

    pub(super) fn combo(&self, combo: &str) -> String {
        KeybindingSettings::format_display(combo, self.general)
    }

    pub(super) fn bindings(&self, v: &[String]) -> String {
        if v.is_empty() {
            t("settings.keybindings.hint_none").to_string()
        } else {
            v.iter()
                .map(|b| self.combo(b))
                .collect::<Vec<_>>()
                .join(", ")
        }
    }

    /// 축 한 행의 값 — 합성된 범위(`Alt+1…0`).
    pub(super) fn axis_summary(
        &self,
        axis: crate::settings::SwitchAxis,
        kb: &KeybindingSettings,
    ) -> String {
        let first = axis.slot(kb, 0).unwrap_or_default();
        let last = axis.slot(kb, axis.slot_count() - 1).unwrap_or_default();
        if first.is_empty() && last.is_empty() {
            return t("settings.keybindings.hint_none").to_string();
        }
        if axis.is_individual(kb) {
            format!("{}…{}", self.combo(first), self.combo(last))
        } else {
            let modifier = axis.modifier(kb);
            format!(
                "{}…{}",
                self.combo(&format!("{modifier}+{first}")),
                self.combo(last)
            )
        }
    }

    pub(super) fn plugin_value(
        &self,
        plugin_id: &str,
        command_id: &str,
        ov: Option<&ShortcutOverride>,
    ) -> String {
        match ov {
            Some(ShortcutOverride::Key { value }) => self.bindings(value),
            Some(ShortcutOverride::Inherit { source }) => format!("@{source}"),
            Some(ShortcutOverride::None) => t("settings.keybindings.hint_none").to_string(),
            None => match self.manifest_default(plugin_id, command_id) {
                Some(d) => self.combo(d),
                None => t("settings.keybindings.hint_none").to_string(),
            },
        }
    }
}
