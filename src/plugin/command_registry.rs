//! Plugin이 매니페스트 `[[contributes.commands]]`로 선언한 단축키 command를
//! 한 곳에 모은 registry.
//!
//! `PluginManager::discover_and_start` 시 모든 활성 plugin의 매니페스트에서
//! command를 흡수하고, plugin enable/disable/install/remove에 맞춰 갱신된다.
//!
//! 사용처:
//! - 키 매칭 (단계 F): focused surface가 plugin 소유일 때 후보 조회
//! - 설정 UI (단계 E): plugin 단축키 항목 목록 표시
//! - effective binding 계산 (단계 D): 매니페스트 기본값 + 사용자 override 합성

use std::collections::HashMap;

use super::host_actions;
use super::manifest::{BindingMode, CommandDecl, Manifest};

/// 한 plugin이 등록한 한 command의 메타데이터.
#[derive(Debug, Clone)]
pub struct PluginCommandEntry {
    pub plugin_id: String,
    pub command_id: String,
    pub title_i18n_key: String,
    /// 매니페스트 `default_keybinding`의 raw 문자열 (예: `"F5"`, `"ctrl+shift+r"`).
    /// 사용자가 별도 override를 지정하지 않았을 때의 시작값. None이면 기본 키 없음.
    pub manifest_default: Option<String>,
    pub binding_mode: BindingMode,
}

impl PluginCommandEntry {
    fn from_decl(plugin_id: &str, decl: &CommandDecl) -> Self {
        // inherit 대상이 화이트리스트에 없으면 Independent로 강등 (warn 로그)
        let mode = match &decl.binding_mode {
            BindingMode::InheritHost(action) if !host_actions::is_inheritable(action) => {
                tracing::warn!(
                    "plugin '{plugin_id}' command '{}' uses unknown inherit target '{action}' — \
                     downgrading to Independent",
                    decl.id
                );
                BindingMode::Independent
            }
            other => other.clone(),
        };
        Self {
            plugin_id: plugin_id.to_string(),
            command_id: decl.id.clone(),
            title_i18n_key: decl.title_i18n_key.clone(),
            manifest_default: decl.default_keybinding.clone(),
            binding_mode: mode,
        }
    }
}

/// Plugin command 일람. PluginManager가 보관하며, plugin enable/disable에 따라
/// `register_plugin` / `unregister_plugin`으로 갱신된다.
#[derive(Debug, Default, Clone)]
pub struct PluginCommandRegistry {
    /// (plugin_id, command_id) 기준 유일. plugin_id가 같은 entry는 vec에 모아서
    /// 설정 UI 그룹핑이 단순해지도록 한다.
    by_plugin: HashMap<String, Vec<PluginCommandEntry>>,
}

impl PluginCommandRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// plugin의 모든 command를 (재)등록. 기존 항목은 모두 교체된다.
    /// 이미 같은 plugin이 등록된 상태라면 새 매니페스트로 통째로 갱신.
    pub fn register_plugin(&mut self, manifest: &Manifest) {
        let entries: Vec<PluginCommandEntry> = manifest
            .contributes
            .commands
            .iter()
            .map(|d| PluginCommandEntry::from_decl(&manifest.id, d))
            .collect();
        if entries.is_empty() {
            self.by_plugin.remove(&manifest.id);
        } else {
            self.by_plugin.insert(manifest.id.clone(), entries);
        }
    }

    /// plugin의 command를 모두 제거.
    pub fn unregister_plugin(&mut self, plugin_id: &str) {
        self.by_plugin.remove(plugin_id);
    }

    /// 특정 plugin의 command 목록.
    pub fn commands_for(&self, plugin_id: &str) -> &[PluginCommandEntry] {
        self.by_plugin
            .get(plugin_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// plugin id 목록 (command가 1개 이상 등록된 plugin만). 정렬은 등록 순서가
    /// 아닌 알파벳 순서로 안정화.
    pub fn plugins_with_commands(&self) -> Vec<String> {
        let mut v: Vec<String> = self.by_plugin.keys().cloned().collect();
        v.sort();
        v
    }

    /// (plugin_id, command_id)로 entry 직접 조회.
    pub fn find(&self, plugin_id: &str, command_id: &str) -> Option<&PluginCommandEntry> {
        self.by_plugin
            .get(plugin_id)
            .and_then(|v| v.iter().find(|e| e.command_id == command_id))
    }

    pub fn is_empty(&self) -> bool {
        self.by_plugin.is_empty()
    }

    pub fn len(&self) -> usize {
        self.by_plugin.values().map(|v| v.len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::manifest::{Contributes, Entry};

    fn manifest_with_commands(id: &str, cmds: Vec<CommandDecl>) -> Manifest {
        Manifest {
            manifest_version: 1,
            id: id.to_string(),
            name: id.to_string(),
            version: "0.1".to_string(),
            authors: vec![],
            description: String::new(),
            homepage: String::new(),
            api_version: "1".to_string(),
            entry: Entry::Process {
                command: "x".to_string(),
                args: vec![],
            },
            surface_kinds: vec![],
            permissions: vec![],
            contributes: Contributes {
                commands: cmds,
                menu_items: vec![],
            },
            lang_dir: "lang".to_string(),
        }
    }

    fn cmd(id: &str, mode: BindingMode) -> CommandDecl {
        CommandDecl {
            id: id.to_string(),
            title_i18n_key: format!("{id}.title"),
            default_keybinding: Some("F5".to_string()),
            binding_mode: mode,
        }
    }

    #[test]
    fn register_and_lookup() {
        let mut reg = PluginCommandRegistry::new();
        let m = manifest_with_commands(
            "com.example.x",
            vec![
                cmd("x.refresh", BindingMode::Independent),
                cmd("x.copy", BindingMode::InheritHost("clipboard.copy".into())),
            ],
        );
        reg.register_plugin(&m);
        assert_eq!(reg.len(), 2);
        assert_eq!(reg.commands_for("com.example.x").len(), 2);
        assert_eq!(reg.find("com.example.x", "x.refresh").unwrap().command_id, "x.refresh");
    }

    #[test]
    fn unregister_removes_all() {
        let mut reg = PluginCommandRegistry::new();
        reg.register_plugin(&manifest_with_commands(
            "com.example.x",
            vec![cmd("x.a", BindingMode::Independent)],
        ));
        reg.unregister_plugin("com.example.x");
        assert!(reg.is_empty());
    }

    #[test]
    fn re_register_replaces_entries() {
        let mut reg = PluginCommandRegistry::new();
        reg.register_plugin(&manifest_with_commands(
            "com.example.x",
            vec![
                cmd("x.a", BindingMode::Independent),
                cmd("x.b", BindingMode::Independent),
            ],
        ));
        // 같은 plugin을 더 적은 command로 다시 등록 → 기존 entry 모두 교체
        reg.register_plugin(&manifest_with_commands(
            "com.example.x",
            vec![cmd("x.c", BindingMode::Independent)],
        ));
        assert_eq!(reg.commands_for("com.example.x").len(), 1);
        assert!(reg.find("com.example.x", "x.a").is_none());
        assert!(reg.find("com.example.x", "x.c").is_some());
    }

    #[test]
    fn unknown_inherit_target_downgraded_to_independent() {
        let mut reg = PluginCommandRegistry::new();
        reg.register_plugin(&manifest_with_commands(
            "com.example.x",
            vec![cmd("x.weird", BindingMode::InheritHost("tab.new".into()))],
        ));
        let entry = reg.find("com.example.x", "x.weird").unwrap();
        assert_eq!(entry.binding_mode, BindingMode::Independent);
    }

    #[test]
    fn empty_command_list_purges_plugin_entry() {
        let mut reg = PluginCommandRegistry::new();
        reg.register_plugin(&manifest_with_commands(
            "com.example.x",
            vec![cmd("x.a", BindingMode::Independent)],
        ));
        // 매니페스트가 commands를 비워서 다시 들어오면 plugin 항목 자체 제거
        reg.register_plugin(&manifest_with_commands("com.example.x", vec![]));
        assert!(reg.is_empty());
    }

    #[test]
    fn plugins_with_commands_sorted() {
        let mut reg = PluginCommandRegistry::new();
        reg.register_plugin(&manifest_with_commands(
            "com.example.zebra",
            vec![cmd("z.a", BindingMode::Independent)],
        ));
        reg.register_plugin(&manifest_with_commands(
            "com.example.alpha",
            vec![cmd("a.a", BindingMode::Independent)],
        ));
        let plugins = reg.plugins_with_commands();
        assert_eq!(plugins, vec!["com.example.alpha", "com.example.zebra"]);
    }
}
