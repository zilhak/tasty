//! `~/.tasty/plugins.toml` — plugin enabled/disabled + 권한 grant 영속화.
//!
//! 형식:
//! ```toml
//! [disabled]
//! ids = ["com.example.broken"]
//!
//! [grants."com.example.explorer"]
//! granted = ["fs.read", "surface.write"]
//! ```

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

const FILE_NAME: &str = "plugins.toml";

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct PluginsConfig {
    #[serde(default)]
    pub disabled: PluginsDisabled,
    /// plugin id → grant entry. 키가 plugin id이므로 BTreeMap으로 정렬 보장.
    #[serde(default)]
    pub grants: BTreeMap<String, PluginGrants>,
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
        self.grants.insert(
            id.to_string(),
            PluginGrants { granted: tokens },
        );
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
        cfg.set_granted("com.example.x", vec!["fs.read".into(), "surface.read".into()]);
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
}
