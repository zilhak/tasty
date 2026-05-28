mod appearance;
mod keybindings;
mod port;
mod port_impl;
mod types;

pub mod general;
pub mod testing;

use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tasty_utils::path::tasty_home;

pub use appearance::{AppearanceSettings, EffectiveFont, FontOverride, FontSettings, HexColor};
pub use general::{GeneralSettings, LinkModifier};
pub use keybindings::KeybindingSettings;
pub use port::SettingsStorage;
pub use port_impl::FileSettingsStorage;
pub use types::{
    AccessibilitySettings, ClipboardSettings, MemorySettings, NotificationSettings,
    PerformanceSettings,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct Settings {
    pub general: GeneralSettings,
    pub appearance: AppearanceSettings,
    pub clipboard: ClipboardSettings,
    pub notification: NotificationSettings,
    pub keybindings: KeybindingSettings,
    pub performance: PerformanceSettings,
    pub memory: MemorySettings,
    pub accessibility: AccessibilitySettings,
}

// ---- Settings file operations ----

impl Settings {
    /// Returns the config file path: ~/.tasty/config.toml
    pub fn config_path() -> Option<PathBuf> {
        tasty_home().map(|dir| dir.join("config.toml"))
    }

    /// Ensure the config directory exists.
    pub fn ensure_config_dir() -> Result<()> {
        if let Some(path) = Self::config_path()
            && let Some(parent) = path.parent()
        {
            fs::create_dir_all(parent)?;
        }
        Ok(())
    }

    /// Load settings from the config file. Falls back to defaults if not found or invalid.
    pub fn load() -> Self {
        let Some(path) = Self::config_path() else {
            tracing::info!("no config path available, using defaults");
            return Self::default();
        };

        match fs::read_to_string(&path) {
            Ok(contents) => {
                // TOML을 Value로 먼저 파싱하여 keybindings 키 목록 추출
                let existing_kb_keys: HashSet<String> = toml::from_str::<toml::Value>(&contents)
                    .ok()
                    .and_then(|v| v.get("keybindings").and_then(|kb| kb.as_table().cloned()))
                    .map(|t| t.keys().cloned().collect())
                    .unwrap_or_default();

                match toml::from_str::<Settings>(&contents) {
                    Ok(mut settings) => {
                        settings
                            .keybindings
                            .remove_conflicts_from_defaults(&existing_kb_keys);
                        tracing::info!("loaded settings from {}", path.display());
                        settings
                    }
                    Err(e) => {
                        tracing::warn!("failed to parse settings file: {e}, using defaults");
                        Self::default()
                    }
                }
            }
            Err(_) => {
                tracing::info!("no settings file at {}, using defaults", path.display());
                Self::default()
            }
        }
    }

    /// Save settings to the config file.
    pub fn save(&self) -> Result<()> {
        Self::ensure_config_dir()?;
        let Some(path) = Self::config_path() else {
            anyhow::bail!("could not determine config path");
        };
        let contents = toml::to_string_pretty(self)?;
        fs::write(&path, contents)?;
        tracing::info!("saved settings to {}", path.display());
        Ok(())
    }

    /// Validate enum-like string fields and replace invalid values with safe defaults.
    /// Returns a report so the caller can decide whether to save() and whether to
    /// surface a popup (theme only). Non-theme fields are fixed silently with a warn log.
    ///
    /// `general.language` is intentionally NOT normalized — users may install custom
    /// translation files at `~/.tasty/lang/{code}.toml`, so any code is potentially valid.
    pub fn normalize(&mut self) -> NormalizeReport {
        let mut report = NormalizeReport::default();

        // appearance.theme — legacy id 매핑.
        // 실제 valid 검증/fallback 은 부팅 흐름의 `tasty_themes::apply_theme()` 가 담당.
        // (settings 가 어떤 id 가 valid 인지 직접 알 필요 없음.)
        match self.appearance.theme.as_str() {
            "catppuccin-mocha" => {
                self.appearance.theme = "mocha".to_string();
                tracing::info!("migrated legacy theme id catppuccin-mocha → mocha");
                report.changed = true;
            }
            "catppuccin-latte" => {
                self.appearance.theme = "latte".to_string();
                tracing::info!("migrated legacy theme id catppuccin-latte → latte");
                report.changed = true;
            }
            _ => {}
        }

        // appearance.ui_scale
        normalize_choice(
            &mut self.appearance.ui_scale,
            &["small", "medium", "large"],
            "medium",
            "ui_scale",
            &mut report.changed,
        );

        // appearance.default_font.font_scale_mode
        normalize_choice(
            &mut self.appearance.default_font.font_scale_mode,
            &["auto", "fixed"],
            "fixed",
            "default_font.font_scale_mode",
            &mut report.changed,
        );

        // appearance.{terminal,markdown,explorer}_font.font_scale_mode (Option<String>)
        for (label, opt) in [
            (
                "terminal_font.font_scale_mode",
                &mut self.appearance.terminal_font.font_scale_mode,
            ),
            (
                "markdown_font.font_scale_mode",
                &mut self.appearance.markdown_font.font_scale_mode,
            ),
            (
                "explorer_font.font_scale_mode",
                &mut self.appearance.explorer_font.font_scale_mode,
            ),
        ] {
            if let Some(mode) = opt.as_ref()
                && !matches!(mode.as_str(), "auto" | "fixed")
            {
                let invalid = opt.take().unwrap();
                tracing::warn!("invalid {label} \"{invalid}\" → unset");
                report.changed = true;
            }
        }

        // general.shell_mode
        normalize_choice(
            &mut self.general.shell_mode,
            &["default", "tasty", "custom"],
            "default",
            "shell_mode",
            &mut report.changed,
        );

        // general.close_behavior
        normalize_choice(
            &mut self.general.close_behavior,
            &["ask", "minimize", "quit"],
            "ask",
            "close_behavior",
            &mut report.changed,
        );

        // general.link_click_modifier
        normalize_choice(
            &mut self.general.link_click_modifier,
            &["ctrl", "alt", "none"],
            "ctrl",
            "link_click_modifier",
            &mut report.changed,
        );

        report
    }
}

/// Result of [`Settings::normalize`].
#[derive(Debug, Default)]
pub struct NormalizeReport {
    /// Original (invalid) theme name if it was replaced. Caller may surface a popup.
    pub invalid_theme_name: Option<String>,
    /// True if any field was changed by normalize. Caller should `save()` to persist.
    pub changed: bool,
}

fn normalize_choice(
    field: &mut String,
    allowed: &[&str],
    fallback: &str,
    label: &str,
    changed: &mut bool,
) {
    if !allowed.contains(&field.as_str()) {
        let invalid = std::mem::take(field);
        *field = fallback.to_string();
        tracing::warn!("invalid {label} \"{invalid}\" → {fallback}");
        *changed = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_settings_valid() {
        let settings = Settings::default();
        assert!(!settings.general.shell.is_empty());
        assert!(settings.appearance.default_font.font_size > 0.0);
        assert!(settings.appearance.sidebar_width > tasty_type_geometry::length::LogicalPx(0.0));
    }

    #[test]
    fn settings_serialization_roundtrip() {
        let settings = Settings::default();
        let toml_str = toml::to_string_pretty(&settings).unwrap();
        let parsed: Settings = toml::from_str(&toml_str).unwrap();
        assert_eq!(
            parsed.appearance.default_font.font_size,
            settings.appearance.default_font.font_size
        );
        assert_eq!(parsed.general.shell, settings.general.shell);
        assert_eq!(
            parsed.notification.coalesce_ms,
            settings.notification.coalesce_ms
        );
    }

    #[test]
    fn settings_partial_toml_uses_defaults() {
        let partial = r#"
[appearance]
ui_scale = "large"
"#;
        let parsed: Settings = toml::from_str(partial).unwrap();
        assert_eq!(parsed.appearance.ui_scale, "large");
        // Other fields fall back to defaults.
        assert_eq!(parsed.appearance.default_font.font_size, 14.0);
        assert!(parsed.notification.enabled);
        assert!(!parsed.general.shell.is_empty());
    }

    #[test]
    fn settings_empty_toml_uses_all_defaults() {
        let parsed: Settings = toml::from_str("").unwrap();
        let defaults = Settings::default();
        assert_eq!(
            parsed.appearance.default_font.font_size,
            defaults.appearance.default_font.font_size
        );
        assert_eq!(
            parsed.notification.coalesce_ms,
            defaults.notification.coalesce_ms
        );
    }

    #[test]
    fn settings_font_family_default() {
        let settings = Settings::default();
        assert_eq!(settings.appearance.default_font.font_family, "");
    }

    #[test]
    fn settings_theme_default() {
        let settings = Settings::default();
        assert_eq!(settings.appearance.theme, "mocha");
    }

    #[test]
    fn settings_background_opacity_default() {
        let settings = Settings::default();
        assert_eq!(settings.appearance.background_opacity, 1.0);
    }

    #[test]
    fn settings_clipboard_history_defaults() {
        let settings = Settings::default();
        assert!(settings.clipboard.history_enabled);
        assert_eq!(settings.clipboard.history_max, 100);
    }

    #[test]
    fn settings_custom_appearance_roundtrip() {
        let mut settings = Settings::default();
        settings.appearance.default_font.font_family = "Fira Code".to_string();
        settings.appearance.default_font.font_size = 18.0;
        settings.appearance.terminal_font.font_size = Some(20.0);
        settings.appearance.theme = "light".to_string();
        settings.appearance.background_opacity = 0.8;
        let toml_str = toml::to_string_pretty(&settings).unwrap();
        let parsed: Settings = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.appearance.default_font.font_family, "Fira Code");
        assert_eq!(parsed.appearance.default_font.font_size, 18.0);
        assert_eq!(parsed.appearance.terminal_font.font_size, Some(20.0));
        assert_eq!(parsed.appearance.theme, "light");
        assert_eq!(parsed.appearance.background_opacity, 0.8);
    }

    #[test]
    fn settings_keybindings_default() {
        let settings = Settings::default();
        assert_eq!(
            settings.keybindings.get_field("new_workspace"),
            Some("alt+n")
        );
        assert_eq!(settings.keybindings.get_field("new_tab"), Some("alt+t"));
    }

    #[test]
    fn normalize_valid_settings_is_noop() {
        let mut settings = Settings::default();
        let report = settings.normalize();
        assert!(!report.changed);
        assert!(report.invalid_theme_name.is_none());
    }

    #[test]
    fn normalize_migrates_legacy_theme_ids() {
        for (legacy, expected) in [("catppuccin-mocha", "mocha"), ("catppuccin-latte", "latte")] {
            let mut settings = Settings::default();
            settings.appearance.theme = legacy.to_string();
            let report = settings.normalize();
            assert!(report.changed, "{legacy} should be migrated");
            assert_eq!(settings.appearance.theme, expected);
        }
    }

    #[test]
    fn normalize_does_not_touch_unknown_theme_ids() {
        // valid 검증은 부팅 흐름의 apply_theme 에서 처리. settings 는 단순 매핑만 한다.
        let mut settings = Settings::default();
        settings.appearance.theme = "my-custom-theme".to_string();
        let report = settings.normalize();
        // 마이그레이션 대상이 아니라 변경 없음 (다른 enum 필드들도 valid 였다고 가정).
        assert!(!report.changed);
        assert_eq!(settings.appearance.theme, "my-custom-theme");
    }

    #[test]
    fn normalize_current_ids_pass_through() {
        for theme in ["mocha", "latte"] {
            let mut settings = Settings::default();
            settings.appearance.theme = theme.to_string();
            let report = settings.normalize();
            assert!(!report.changed, "theme {theme} should be accepted as-is");
            assert_eq!(settings.appearance.theme, theme);
        }
    }

    #[test]
    fn normalize_silent_enum_fields_get_fixed() {
        let mut settings = Settings::default();
        settings.appearance.ui_scale = "huge".to_string();
        settings.appearance.default_font.font_scale_mode = "bogus".to_string();
        settings.appearance.terminal_font.font_scale_mode = Some("nope".to_string());
        settings.general.shell_mode = "weird".to_string();
        settings.general.close_behavior = "explode".to_string();
        settings.general.link_click_modifier = "meta".to_string();

        let report = settings.normalize();
        assert!(report.changed);
        assert!(report.invalid_theme_name.is_none());
        assert_eq!(settings.appearance.ui_scale, "medium");
        assert_eq!(settings.appearance.default_font.font_scale_mode, "fixed");
        assert!(settings.appearance.terminal_font.font_scale_mode.is_none());
        assert_eq!(settings.general.shell_mode, "default");
        assert_eq!(settings.general.close_behavior, "ask");
        assert_eq!(settings.general.link_click_modifier, "ctrl");
    }

    #[test]
    fn normalize_does_not_touch_language() {
        let mut settings = Settings::default();
        settings.general.language = "fr".to_string();
        let report = settings.normalize();
        assert!(!report.changed);
        assert_eq!(settings.general.language, "fr");
    }
}
