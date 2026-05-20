//! `~/.tasty/plugins.toml` — plugin enabled/disabled + 권한 grant + 단축키 영속화.
//!
//! 형식:
//! ```toml
//! [disabled]
//! ids = ["com.example.broken"]
//!
//! [removed_builtins]
//! ids = ["com.tasty.explorer"]
//!
//! [grants."com.example.explorer"]
//! granted = ["fs.read", "surface.write"]
//!
//! [keybindings."com.example.explorer"."explorer.refresh"]
//! mode = "key"
//! value = ["F6"]
//!
//! [keybindings."com.example.explorer"."explorer.copy_paths"]
//! mode = "inherit"
//! source = "clipboard.copy"
//! ```

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

const FILE_NAME: &str = "plugins.toml";

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct PluginsConfig {
    #[serde(default)]
    pub disabled: PluginsDisabled,
    /// 사용자가 제거한 기본 제공 플러그인 id 목록. 다음 부팅 시 bundle에서
    /// 자동 재설치되지 않도록 차단한다.
    #[serde(default)]
    pub removed_builtins: PluginsDisabled,
    /// plugin id → grant entry. 키가 plugin id이므로 BTreeMap으로 정렬 보장.
    #[serde(default)]
    pub grants: BTreeMap<String, PluginGrants>,
    /// plugin id → command id → 사용자가 매니페스트 기본값을 덮어쓴 단축키 설정.
    /// 항목이 없으면 매니페스트의 `default_keybinding` + `binding_mode`가 그대로 적용.
    #[serde(default)]
    pub keybindings: BTreeMap<String, BTreeMap<String, ShortcutOverride>>,
}

/// 사용자가 plugin 단축키를 어떻게 덮어쓰는지 표현.
///
/// - `Key { value }`: plugin 매니페스트가 inherit를 선언했더라도 사용자가
///   독립 키로 떼어낸 경우 또는 매니페스트가 independent였던 경우.
/// - `Inherit { source }`: plugin이 inherit를 선언한 command를 사용자가
///   그대로 두거나, plugin이 inherit 가능한 host action으로 명시 변경한 경우.
/// - `None`: 사용자가 의도적으로 단축키를 비워둠. 매니페스트 기본값보다 우선.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum ShortcutOverride {
    Key { value: Vec<String> },
    Inherit { source: String },
    None,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct PluginsDisabled {
    #[serde(default)]
    pub ids: Vec<String>,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct PluginGrants {
    /// 사용자가 grant한 권한 토큰들 (예: "fs.read", "surface.write").
    /// 매니페스트의 `permissions`에 선언된 토큰의 부분집합이어야 함.
    #[serde(default)]
    pub granted: Vec<String>,
}

impl PluginsConfig {
    fn path() -> Option<PathBuf> {
        tasty_core::paths::tasty_home().map(|d| d.join(FILE_NAME))
    }

    pub fn load() -> Self {
        let path = match Self::path() {
            Some(p) => p,
            None => return Self::default(),
        };
        if !path.exists() {
            return Self::default();
        }
        match std::fs::read_to_string(&path) {
            Ok(s) => match toml::from_str::<PluginsConfig>(&s) {
                Ok(cfg) => cfg,
                Err(e) => {
                    tracing::warn!("plugins.toml parse error: {} — using defaults", e);
                    Self::default()
                }
            },
            Err(e) => {
                tracing::warn!("plugins.toml read error: {} — using defaults", e);
                Self::default()
            }
        }
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::path().ok_or_else(|| anyhow::anyhow!("no tasty home directory"))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let s = toml::to_string_pretty(self)?;
        std::fs::write(&path, s)?;
        Ok(())
    }

    pub fn is_disabled(&self, id: &str) -> bool {
        self.disabled.ids.iter().any(|x| x == id)
    }

    pub fn is_builtin_removed(&self, id: &str) -> bool {
        self.removed_builtins.ids.iter().any(|x| x == id)
    }

    pub fn mark_builtin_removed(&mut self, id: &str) -> bool {
        if self.is_builtin_removed(id) {
            false
        } else {
            self.removed_builtins.ids.push(id.to_string());
            true
        }
    }

    pub fn enable(&mut self, id: &str) -> bool {
        let before = self.disabled.ids.len();
        self.disabled.ids.retain(|x| x != id);
        before != self.disabled.ids.len()
    }

    pub fn disable(&mut self, id: &str) -> bool {
        if self.is_disabled(id) {
            false
        } else {
            self.disabled.ids.push(id.to_string());
            true
        }
    }

    /// plugin에 grant된 권한 토큰 set. 미등록 plugin은 빈 set.
    pub fn granted_permissions(&self, id: &str) -> HashSet<String> {
        self.grants
            .get(id)
            .map(|g| g.granted.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// plugin grant entry를 매니페스트 권한으로 초기화. 첫 설치/재설치 시 호출.
    pub fn set_granted(&mut self, id: &str, tokens: Vec<String>) {
        self.grants
            .insert(id.to_string(), PluginGrants { granted: tokens });
    }

    /// 단일 권한 추가 (없으면). 권한이 새로 추가됐는지 반환.
    pub fn grant(&mut self, id: &str, token: &str) -> bool {
        let entry = self.grants.entry(id.to_string()).or_default();
        if entry.granted.iter().any(|t| t == token) {
            false
        } else {
            entry.granted.push(token.to_string());
            true
        }
    }

    /// 단일 권한 제거. 실제로 제거됐는지 반환.
    pub fn revoke(&mut self, id: &str, token: &str) -> bool {
        let Some(entry) = self.grants.get_mut(id) else {
            return false;
        };
        let before = entry.granted.len();
        entry.granted.retain(|t| t != token);
        before != entry.granted.len()
    }

    /// 사용자가 plugin command 단축키에 적용한 override를 조회. 없으면 None.
    pub fn shortcut_override(
        &self,
        plugin_id: &str,
        command_id: &str,
    ) -> Option<&ShortcutOverride> {
        self.keybindings
            .get(plugin_id)
            .and_then(|m| m.get(command_id))
    }

    /// override를 설정. 같은 키가 있으면 덮어씀.
    pub fn set_shortcut_override(
        &mut self,
        plugin_id: &str,
        command_id: &str,
        ov: ShortcutOverride,
    ) {
        self.keybindings
            .entry(plugin_id.to_string())
            .or_default()
            .insert(command_id.to_string(), ov);
    }

    /// override를 제거 (매니페스트 기본값으로 되돌림). 실제 제거됐는지 반환.
    pub fn clear_shortcut_override(&mut self, plugin_id: &str, command_id: &str) -> bool {
        let Some(map) = self.keybindings.get_mut(plugin_id) else {
            return false;
        };
        let removed = map.remove(command_id).is_some();
        if map.is_empty() {
            self.keybindings.remove(plugin_id);
        }
        removed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enable_disable_round_trip() {
        let mut cfg = PluginsConfig::default();
        assert!(!cfg.is_disabled("com.example.x"));
        assert!(cfg.disable("com.example.x"));
        assert!(cfg.is_disabled("com.example.x"));
        assert!(!cfg.disable("com.example.x")); // 두 번째 disable은 false
        assert!(cfg.enable("com.example.x"));
        assert!(!cfg.is_disabled("com.example.x"));
        assert!(!cfg.enable("com.example.x")); // 이미 enabled
    }

    #[test]
    fn grant_revoke_round_trip() {
        let mut cfg = PluginsConfig::default();
        assert!(cfg.granted_permissions("com.example.x").is_empty());
        assert!(cfg.grant("com.example.x", "fs.read"));
        assert!(cfg.grant("com.example.x", "surface.write"));
        assert!(!cfg.grant("com.example.x", "fs.read")); // 중복은 false
        let granted = cfg.granted_permissions("com.example.x");
        assert!(granted.contains("fs.read"));
        assert!(granted.contains("surface.write"));
        assert!(cfg.revoke("com.example.x", "fs.read"));
        assert!(!cfg.revoke("com.example.x", "fs.read")); // 두 번째는 false
        let granted = cfg.granted_permissions("com.example.x");
        assert!(!granted.contains("fs.read"));
        assert!(granted.contains("surface.write"));
    }

    #[test]
    fn set_granted_replaces_entire_list() {
        let mut cfg = PluginsConfig::default();
        cfg.set_granted(
            "com.example.x",
            vec!["fs.read".into(), "surface.read".into()],
        );
        let g = cfg.granted_permissions("com.example.x");
        assert_eq!(g.len(), 2);
        cfg.set_granted("com.example.x", vec!["fs.write".into()]);
        let g = cfg.granted_permissions("com.example.x");
        assert_eq!(g.len(), 1);
        assert!(g.contains("fs.write"));
    }

    #[test]
    fn parses_grants_section() {
        let s = r#"
            [grants."com.example.x"]
            granted = ["fs.read", "surface.write"]
        "#;
        let cfg: PluginsConfig = toml::from_str(s).unwrap();
        let g = cfg.granted_permissions("com.example.x");
        assert!(g.contains("fs.read"));
        assert!(g.contains("surface.write"));
    }

    #[test]
    fn parses_disabled_list() {
        let s = r#"
            [disabled]
            ids = ["com.example.broken", "com.foo.bar"]
        "#;
        let cfg: PluginsConfig = toml::from_str(s).unwrap();
        assert!(cfg.is_disabled("com.example.broken"));
        assert!(cfg.is_disabled("com.foo.bar"));
        assert!(!cfg.is_disabled("com.example.good"));
    }

    #[test]
    fn shortcut_override_round_trip() {
        let mut cfg = PluginsConfig::default();
        assert!(
            cfg.shortcut_override("com.example.x", "x.refresh")
                .is_none()
        );
        cfg.set_shortcut_override(
            "com.example.x",
            "x.refresh",
            ShortcutOverride::Key {
                value: vec!["F6".into()],
            },
        );
        match cfg.shortcut_override("com.example.x", "x.refresh") {
            Some(ShortcutOverride::Key { value }) => assert_eq!(value, &vec!["F6".to_string()]),
            other => panic!("unexpected: {other:?}"),
        }
        assert!(cfg.clear_shortcut_override("com.example.x", "x.refresh"));
        assert!(
            cfg.shortcut_override("com.example.x", "x.refresh")
                .is_none()
        );
        // plugin 항목이 비면 자체 제거됨
        assert!(!cfg.keybindings.contains_key("com.example.x"));
    }

    #[test]
    fn shortcut_override_serialization() {
        let mut cfg = PluginsConfig::default();
        cfg.set_shortcut_override(
            "com.example.x",
            "x.refresh",
            ShortcutOverride::Key {
                value: vec!["F6".into()],
            },
        );
        cfg.set_shortcut_override(
            "com.example.x",
            "x.copy",
            ShortcutOverride::Inherit {
                source: "clipboard.copy".into(),
            },
        );
        cfg.set_shortcut_override("com.example.x", "x.paste", ShortcutOverride::None);
        let s = toml::to_string_pretty(&cfg).unwrap();
        let restored: PluginsConfig = toml::from_str(&s).unwrap();
        assert!(matches!(
            restored.shortcut_override("com.example.x", "x.refresh"),
            Some(ShortcutOverride::Key { .. })
        ));
        assert!(matches!(
            restored.shortcut_override("com.example.x", "x.copy"),
            Some(ShortcutOverride::Inherit { .. })
        ));
        assert!(matches!(
            restored.shortcut_override("com.example.x", "x.paste"),
            Some(ShortcutOverride::None)
        ));
    }
}
