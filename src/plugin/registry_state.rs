//! `~/.tasty/plugins.toml` — plugin enabled/disabled 영속화.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

const FILE_NAME: &str = "plugins.toml";

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct PluginsConfig {
    #[serde(default)]
    pub disabled: PluginsDisabled,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct PluginsDisabled {
    #[serde(default)]
    pub ids: Vec<String>,
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
