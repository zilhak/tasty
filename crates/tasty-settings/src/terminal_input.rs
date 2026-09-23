//! Per-application terminal key encoding. These rules never transform pasted text.
use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TerminalInputSettings {
    pub rules: Vec<TerminalInputRule>,
    /// Remember first-time plugin defaults even after the user removes a rule.
    pub initialized_defaults: BTreeSet<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalInputRule {
    pub app: String,
    pub shift_enter_newline: bool,
}

/// Match executable basenames, ignoring ASCII case and the Windows suffix.
pub fn normalize_app(app: &str) -> String {
    let name = app
        .trim()
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    name.strip_suffix(".exe").unwrap_or(&name).to_owned()
}

impl TerminalInputSettings {
    pub fn shift_enter_newline(&self, foreground: Option<&str>) -> bool {
        let Some(app) = foreground.map(normalize_app).filter(|s| !s.is_empty()) else {
            return false;
        };
        self.rules
            .iter()
            .find(|rule| normalize_app(&rule.app) == app)
            .is_some_and(|rule| rule.shift_enter_newline)
    }

    pub fn set_rule(&mut self, app: &str, newline: bool) -> Result<(), String> {
        let app = normalize_app(app);
        if app.is_empty() || app.chars().any(char::is_control) {
            return Err("app must be a nonempty executable name without control characters".into());
        }
        if let Some(rule) = self.rules.iter_mut().find(|r| normalize_app(&r.app) == app) {
            rule.shift_enter_newline = newline;
        } else {
            self.rules.push(TerminalInputRule {
                app,
                shift_enter_newline: newline,
            });
        }
        Ok(())
    }

    pub fn remove_rule(&mut self, app: &str) {
        let app = normalize_app(app);
        self.rules.retain(|rule| normalize_app(&rule.app) != app);
    }

    /// Install once per plugin/application, preserving any existing user choice.
    pub fn initialize_rule(
        &mut self,
        owner: &str,
        app: &str,
        newline: bool,
    ) -> Result<bool, String> {
        let app = normalize_app(app);
        // Validate before remembering the registration.
        let mut candidate = Self::default();
        candidate.set_rule(&app, newline)?;
        let key = format!("{owner}/{app}");
        if self.initialized_defaults.contains(&key) {
            return Ok(false);
        }
        if !self
            .rules
            .iter()
            .any(|rule| normalize_app(&rule.app) == app)
        {
            self.set_rule(&app, newline)?;
        }
        self.initialized_defaults.insert(key);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executable_matching_does_not_guess_launchers_or_substrings() {
        let mut input = TerminalInputSettings::default();
        input.set_rule("claude", true).unwrap();
        assert!(input.shift_enter_newline(Some(r"C:\tools\CLAUDE.EXE")));
        for name in [
            None,
            Some("node"),
            Some("bash"),
            Some("claude-helper"),
            Some("codex"),
        ] {
            assert!(!input.shift_enter_newline(name));
        }
        input.set_rule("CLAUDE.exe", false).unwrap();
        assert_eq!(input.rules.len(), 1);
        assert!(!input.shift_enter_newline(Some("claude")));
    }

    #[test]
    fn plugin_default_preserves_edits_deletion_and_restart() {
        let mut input = TerminalInputSettings::default();
        assert!(input.initialize_rule("plugin", "claude", true).unwrap());
        input.set_rule("claude", false).unwrap();
        assert!(!input.initialize_rule("plugin", "claude", true).unwrap());
        assert!(!input.shift_enter_newline(Some("claude")));
        input.remove_rule("claude");
        let saved = toml::to_string(&input).unwrap();
        let mut restored: TerminalInputSettings = toml::from_str(&saved).unwrap();
        assert!(!restored.initialize_rule("plugin", "claude", true).unwrap());
        assert!(restored.rules.is_empty());
    }

    #[test]
    fn existing_user_rule_wins_over_first_install() {
        let mut input = TerminalInputSettings::default();
        input.set_rule("claude", false).unwrap();
        input.initialize_rule("plugin", "claude", true).unwrap();
        assert!(!input.shift_enter_newline(Some("claude")));
        assert!(input.initialize_rule("plugin", "", true).is_err());
    }

    #[test]
    fn legacy_config_defaults_to_platform_encoding() {
        let settings: crate::Settings = toml::from_str("[general]\n").unwrap();
        assert!(settings.terminal_input.rules.is_empty());
        assert!(!settings.terminal_input.shift_enter_newline(Some("claude")));
    }
}
