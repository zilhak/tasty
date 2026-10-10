//! 번들의 설정 항목과 키 조합을 표시 문자열로 바꾼다. 상태는 변경하지 않는다.

use std::collections::BTreeMap;

use crate::i18n::t;
use crate::plugin::registry_state::ShortcutOverride;
use crate::settings::{GeneralSettings, KeybindingSettings, ScriptRegistry};
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

    /// 빠른 전환 키 범위의 표시값. 예: Alt+1…0.
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
