use std::fs;
use std::path::PathBuf;

use anyhow::Result;
use directories::BaseDirs;
use serde::{Deserialize, Serialize};

/// Returns the Tasty home directory: ~/.tasty/
/// Consistent across all platforms for easy AI/agent access.
pub fn tasty_home() -> Option<PathBuf> {
    BaseDirs::new().map(|dirs| dirs.home_dir().join(".tasty"))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub general: GeneralSettings,
    pub appearance: AppearanceSettings,
    pub clipboard: ClipboardSettings,
    pub notification: NotificationSettings,
    pub keybindings: KeybindingSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralSettings {
    pub shell: String,
    /// Shell startup mode: "default", "fast", or "custom".
    pub shell_mode: String,
    /// Extra arguments passed to the shell (used when shell_mode is "custom").
    pub shell_args: String,
    pub startup_command: String,
    pub language: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppearanceSettings {
    pub font_family: String,
    pub font_size: f32,
    pub theme: String,
    pub background_opacity: f32,
    pub sidebar_width: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ClipboardSettings {
    pub macos_style: bool,
    pub linux_style: bool,
    pub windows_style: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NotificationSettings {
    pub enabled: bool,
    pub system_notification: bool,
    pub sound: bool,
    pub coalesce_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct KeybindingSettings {
    pub new_workspace: String,
    pub new_tab: String,
    pub split_pane_vertical: String,
    pub split_pane_horizontal: String,
    pub split_surface_vertical: String,
    pub split_surface_horizontal: String,
}

// ---- Default implementations ----

impl Default for Settings {
    fn default() -> Self {
        Self {
            general: GeneralSettings::default(),
            appearance: AppearanceSettings::default(),
            clipboard: ClipboardSettings::default(),
            notification: NotificationSettings::default(),
            keybindings: KeybindingSettings::default(),
        }
    }
}

impl Default for GeneralSettings {
    fn default() -> Self {
        let shell = Self::detect_shell();
        Self {
            shell,
            shell_mode: "default".to_string(),
            shell_args: String::new(),
            startup_command: String::new(),
            language: "en".to_string(),
        }
    }
}

impl GeneralSettings {
    /// Detect bash (Git Bash on Windows, system bash on Unix).
    /// Returns the path if found, or an empty string if not.
    pub fn detect_shell() -> String {
        Self::detect_bash().unwrap_or_default()
    }

    /// Try to find bash. On Windows this means Git Bash.
    pub fn detect_bash() -> Option<String> {
        #[cfg(windows)]
        {
            let candidates = [
                std::env::var("ProgramFiles")
                    .map(|p| format!("{}/Git/bin/bash.exe", p))
                    .unwrap_or_default(),
                "C:/Program Files/Git/bin/bash.exe".to_string(),
                "C:/Program Files (x86)/Git/bin/bash.exe".to_string(),
            ];
            for path in &candidates {
                if !path.is_empty() && std::path::Path::new(path).exists() {
                    return Some(path.clone());
                }
            }
            None
        }
        #[cfg(not(windows))]
        {
            // Prefer $SHELL, then common paths
            if let Ok(shell) = std::env::var("SHELL") {
                return Some(shell);
            }
            for path in &["/bin/zsh", "/bin/bash", "/bin/sh"] {
                if std::path::Path::new(path).exists() {
                    return Some(path.to_string());
                }
            }
            None
        }
    }

    /// Returns true if the configured shell path points to an existing bash-compatible shell.
    /// On Windows, the filename must contain "bash" (e.g. bash.exe).
    /// On Unix, any existing shell is accepted (zsh, bash, fish, sh).
    pub fn is_shell_valid(&self) -> bool {
        if self.shell.is_empty() {
            return false;
        }
        let path = std::path::Path::new(&self.shell);
        if !path.exists() {
            return false;
        }
        #[cfg(windows)]
        {
            // On Windows, only accept bash-compatible shells
            let filename = path.file_name()
                .and_then(|f| f.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            filename.contains("bash") || filename.contains("zsh")
        }
        #[cfg(not(windows))]
        {
            true
        }
    }

    /// Resolve effective shell arguments based on shell_mode.
    pub fn effective_shell_args(&self) -> Vec<String> {
        match self.shell_mode.as_str() {
            "fast" => {
                Self::ensure_tasty_bashrc(&Self::tasty_bashrc_path());
                vec![
                    "--norc".to_string(),
                    "--noprofile".to_string(),
                ]
            }
            "custom" => self
                .shell_args
                .split_whitespace()
                .map(|s| s.to_string())
                .collect(),
            _ => vec![], // "default"
        }
    }

    /// Returns a command to source the Tasty bashrc (sent to PTY after shell starts).
    /// Returns None if not in fast mode.
    pub fn fast_mode_init_command(&self) -> Option<String> {
        if self.shell_mode == "fast" {
            let rcfile = Self::tasty_bashrc_path();
            // Use printf to avoid issues with echo interpretation
            Some(format!(". '{}'\n", rcfile.replace('\\', "/")))
        } else {
            None
        }
    }

    /// Path to Tasty's lightweight bashrc.
    fn tasty_bashrc_path() -> String {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .map(std::path::PathBuf::from)
            .unwrap_or_default();
        home.join(".tasty").join("bashrc").to_string_lossy().to_string()
    }

    /// Create ~/.tasty/bashrc if it doesn't exist.
    fn ensure_tasty_bashrc(path: &str) {
        let p = std::path::Path::new(path);
        if p.exists() {
            return;
        }
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let contents = r#"# Tasty fast-start bashrc
# Minimal shell configuration for fast pane/tab creation.
# Edit this file to customize. Tasty will not overwrite it.

# UTF-8
export LANG=en_US.UTF-8
export LC_ALL=en_US.UTF-8

# Inherit Windows PATH
ORIGINAL_PATH="${ORIGINAL_PATH:-${PATH}}"
export PATH="/usr/local/bin:/usr/bin:/bin:${ORIGINAL_PATH}"

# Prompt
PS1='\[\033[32m\]\u@\h\[\033[0m\] \[\033[33m\]\w\[\033[0m\]\n\$ '

# Common aliases
alias ls='ls --color=auto'
alias ll='ls -la'
alias grep='grep --color=auto'
"#;
        let _ = std::fs::write(p, contents);
    }
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            font_family: String::new(),
            font_size: 14.0,
            theme: "dark".to_string(),
            background_opacity: 1.0,
            sidebar_width: 180.0,
        }
    }
}

impl Default for ClipboardSettings {
    fn default() -> Self {
        #[cfg(target_os = "macos")]
        {
            Self {
                macos_style: true,
                linux_style: false,
                windows_style: false,
            }
        }
        #[cfg(target_os = "linux")]
        {
            Self {
                macos_style: false,
                linux_style: true,
                windows_style: false,
            }
        }
        #[cfg(target_os = "windows")]
        {
            Self {
                macos_style: false,
                linux_style: false,
                windows_style: true,
            }
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
        {
            Self {
                macos_style: false,
                linux_style: false,
                windows_style: true,
            }
        }
    }
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            system_notification: true,
            sound: false,
            coalesce_ms: 500,
        }
    }
}

impl Default for KeybindingSettings {
    fn default() -> Self {
        Self {
            new_workspace: "ctrl+shift+n".to_string(),
            new_tab: "ctrl+shift+t".to_string(),
            split_pane_vertical: "ctrl+shift+e".to_string(),
            split_pane_horizontal: "ctrl+shift+o".to_string(),
            split_surface_vertical: "ctrl+shift+d".to_string(),
            split_surface_horizontal: "ctrl+shift+j".to_string(),
        }
    }
}

// ---- Settings file operations ----

impl Settings {
    /// Returns the config file path: ~/.tasty/config.toml
    pub fn config_path() -> Option<PathBuf> {
        tasty_home().map(|dir| dir.join("config.toml"))
    }

    /// Ensure the config directory exists.
    pub fn ensure_config_dir() -> Result<()> {
        if let Some(path) = Self::config_path() {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
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
            Ok(contents) => match toml::from_str::<Settings>(&contents) {
                Ok(settings) => {
                    tracing::info!("loaded settings from {}", path.display());
                    settings
                }
                Err(e) => {
                    tracing::warn!("failed to parse settings file: {e}, using defaults");
                    Self::default()
                }
            },
            Err(_) => {
                tracing::info!(
                    "no settings file at {}, using defaults",
                    path.display()
                );
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_settings_valid() {
        let settings = Settings::default();
        assert!(!settings.general.shell.is_empty());
        assert!(settings.appearance.font_size > 0.0);
        assert!(settings.appearance.sidebar_width > 0.0);
    }

    #[test]
    fn settings_serialization_roundtrip() {
        let settings = Settings::default();
        let toml_str = toml::to_string_pretty(&settings).unwrap();
        let parsed: Settings = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.appearance.font_size, settings.appearance.font_size);
        assert_eq!(parsed.general.shell, settings.general.shell);
        assert_eq!(parsed.notification.coalesce_ms, settings.notification.coalesce_ms);
    }

    #[test]
    fn settings_partial_toml_uses_defaults() {
        let partial = r#"
[appearance]
font_size = 18.0
"#;
        let parsed: Settings = toml::from_str(partial).unwrap();
        assert_eq!(parsed.appearance.font_size, 18.0);
        // Other fields should be defaults
        assert!(parsed.notification.enabled);
        assert!(!parsed.general.shell.is_empty());
    }

    #[test]
    fn settings_empty_toml_uses_all_defaults() {
        let parsed: Settings = toml::from_str("").unwrap();
        let defaults = Settings::default();
        assert_eq!(parsed.appearance.font_size, defaults.appearance.font_size);
        assert_eq!(parsed.notification.coalesce_ms, defaults.notification.coalesce_ms);
    }

    #[test]
    fn settings_font_family_default() {
        let settings = Settings::default();
        assert_eq!(settings.appearance.font_family, "");
    }

    #[test]
    fn settings_theme_default() {
        let settings = Settings::default();
        assert_eq!(settings.appearance.theme, "dark");
    }

    #[test]
    fn settings_background_opacity_default() {
        let settings = Settings::default();
        assert_eq!(settings.appearance.background_opacity, 1.0);
    }

    #[test]
    fn settings_clipboard_platform_defaults() {
        let settings = Settings::default();
        #[cfg(target_os = "windows")]
        assert!(settings.clipboard.windows_style);
        #[cfg(target_os = "macos")]
        assert!(settings.clipboard.macos_style);
        #[cfg(target_os = "linux")]
        assert!(settings.clipboard.linux_style);
    }

    #[test]
    fn settings_custom_appearance_roundtrip() {
        let mut settings = Settings::default();
        settings.appearance.font_family = "Fira Code".to_string();
        settings.appearance.font_size = 18.0;
        settings.appearance.theme = "light".to_string();
        settings.appearance.background_opacity = 0.8;
        let toml_str = toml::to_string_pretty(&settings).unwrap();
        let parsed: Settings = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.appearance.font_family, "Fira Code");
        assert_eq!(parsed.appearance.font_size, 18.0);
        assert_eq!(parsed.appearance.theme, "light");
        assert_eq!(parsed.appearance.background_opacity, 0.8);
    }

    #[test]
    fn settings_keybindings_default() {
        let settings = Settings::default();
        assert_eq!(settings.keybindings.new_workspace, "ctrl+shift+n");
        assert_eq!(settings.keybindings.new_tab, "ctrl+shift+t");
    }
}
