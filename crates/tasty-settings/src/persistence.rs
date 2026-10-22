//! Settings file protection, serialization and atomic replacement.
use crate::{Settings, SettingsOrigin};
use anyhow::Result;
use std::{collections::HashSet, fs, path::PathBuf};
use tasty_utils::path::tasty_home;

// ---- Settings file operations ----

impl Settings {
    /// tasty_home 아래 config.toml 경로. TASTY_HOME과 debug/release 구분을 따른다.
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
        Self::load_from_path(&path)
    }

    /// 설정 파일을 읽는다. 부재는 기본값으로 시작하고 다른 읽기 실패는 이후 저장을 막는다.
    pub(super) fn load_from_path(path: &std::path::Path) -> Self {
        let contents = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                tracing::info!("no settings file at {}, using defaults", path.display());
                return Self::default();
            }
            Err(e) => {
                tracing::error!(
                    "failed to read settings file {}: {e} — using defaults and refusing to \
                     overwrite it (fix permissions or move the file to start fresh)",
                    path.display()
                );
                return Self {
                    origin: SettingsOrigin::ProtectedUnreadable,
                    ..Self::default()
                };
            }
        };
        Self::parse_or_default(&contents, path)
    }

    /// 파싱 실패는 오류로 알리고 기본값을 반환한다. 원본 파일은 저장 전까지 그대로 둔다.
    fn parse_or_default(contents: &str, path: &std::path::Path) -> Self {
        match Self::parse_with_migration(contents) {
            Ok(settings) => {
                tracing::info!("loaded settings from {}", path.display());
                settings
            }
            Err(e) => {
                tracing::error!(
                    "failed to parse settings file {}: {e} — using defaults; the file is left \
                     as it is and will be moved aside to a .bak before anything overwrites it",
                    path.display()
                );
                Self {
                    origin: SettingsOrigin::Unparsable,
                    ..Self::default()
                }
            }
        }
    }

    /// TOML 파싱 + keybinding 충돌 제거 + 레거시 font override 마이그레이션. keybinding
    /// 충돌 판정에 "원본 파일에 실제로 있던 키" 목록이 필요해, TOML 을 `toml::Value` 로
    /// 먼저 훑어 `[keybindings]` 테이블의 키 이름을 뽑아둔다.
    pub(super) fn parse_with_migration(
        contents: &str,
    ) -> std::result::Result<Self, toml::de::Error> {
        let raw = toml::from_str::<toml::Value>(contents).ok();
        let existing_kb_keys: HashSet<String> = raw
            .as_ref()
            .and_then(|v| v.get("keybindings").and_then(|kb| kb.as_table().cloned()))
            .map(|t| t.keys().cloned().collect())
            .unwrap_or_default();
        if let Some(raw) = &raw {
            for (key, replacement) in Self::retired_keys_in(raw) {
                tracing::warn!(
                    "settings key {key} is no longer used and is ignored; {replacement} decides \
                     this value now, and the key is dropped on the next save"
                );
            }
        }

        let mut settings = toml::from_str::<Settings>(contents)?;
        settings.appearance.migrate_legacy_font_overrides();
        settings
            .keybindings
            .remove_conflicts_from_defaults(&existing_kb_keys);
        Ok(settings)
    }

    /// 더 이상 읽지 않는 설정 키(`[테이블, 키, 대신 정하는 설정]`). 남아 있어도 읽기는 실패하지
    /// 않으며 경고를 남기고 무시한다. 저장은 현재 필드만 쓰므로 다음 저장에서 빠진다.
    pub const RETIRED_KEYS: &'static [(&'static str, &'static str, &'static str)] =
        &[("appearance", "sidebar_width", "appearance.ui_scale")];

    /// 파일에 남아 있는 폐기 키를 `(테이블.키, 대신 정하는 설정)`으로 돌려준다.
    pub(super) fn retired_keys_in(raw: &toml::Value) -> Vec<(String, &'static str)> {
        Self::RETIRED_KEYS
            .iter()
            .filter(|(table, key, _)| raw.get(table).and_then(|t| t.get(key)).is_some())
            .map(|(table, key, replacement)| (format!("{table}.{key}"), *replacement))
            .collect()
    }

    /// 현재 파일을 다시 파싱할 수 없는지 확인한다. 없거나 읽지 못하면 false다.
    pub fn file_is_unparsable(path: &std::path::Path) -> bool {
        match fs::read_to_string(path) {
            Ok(contents) => Self::parse_with_migration(&contents).is_err(),
            Err(_) => false,
        }
    }

    /// 설정을 저장한다. 원본을 읽지 못한 ProtectedUnreadable 상태는 덮어쓰기를 거절한다.
    pub fn save(&self) -> Result<()> {
        Self::ensure_config_dir()?;
        let Some(path) = Self::config_path() else {
            anyhow::bail!("could not determine config path");
        };
        self.save_to_path(&path)
    }

    /// 확정된 경로에 쓴다. 덮어쓰기 전에 [`Self::protect_existing_file`] 로 기존 파일을
    /// 지키고, 같은 디렉터리의 임시 파일을 대상 이름으로 바꿔 교체한다. 중간에 실패하면 원본이 남는다.
    pub(super) fn save_to_path(&self, path: &std::path::Path) -> Result<()> {
        self.protect_existing_file(path)?;
        let contents = toml::to_string_pretty(self)?;
        tasty_utils::file_replace::replace(path, contents.as_bytes())?;
        tracing::info!("saved settings to {}", path.display());
        Ok(())
    }

    /// 파싱하지 못한 원본이 아직 있으면 백업한 뒤 저장한다. 읽기 실패 상태는 저장 자체를 거절한다.
    /// 다시 읽어 확인하므로 두 번째 저장이 이미 쓴 정상 파일을 다시 백업하지 않는다.
    fn protect_existing_file(&self, path: &std::path::Path) -> Result<()> {
        match self.origin {
            SettingsOrigin::Clean => return Ok(()),
            SettingsOrigin::ProtectedUnreadable => anyhow::bail!(
                "refusing to overwrite the settings file {}: it could not be read (see the \
                 earlier error); fix permissions or move it aside first",
                path.display()
            ),
            SettingsOrigin::Unparsable => {}
        }
        let still_unparsable = match fs::read_to_string(path) {
            Ok(contents) => Self::parse_with_migration(&contents).is_err(),
            // 이미 사라졌다면 지킬 것이 없다.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
            Err(e) => anyhow::bail!(
                "refusing to overwrite the settings file {}: it could not be re-read before \
                 being replaced ({e})",
                path.display()
            ),
        };
        if !still_unparsable {
            return Ok(());
        }
        match tasty_utils::path::preserve_corrupt_file(path) {
            Ok(Some(backup)) => {
                tracing::error!(
                    "the settings file {} could not be parsed at startup; it was moved to {} \
                     before being replaced",
                    path.display(),
                    backup.display()
                );
                Ok(())
            }
            // 다른 곳에서 이미 옮겼다 — 원본은 그 백업에 있고 이 자리는 비었다.
            Ok(None) => Ok(()),
            Err(e) => anyhow::bail!(
                "refusing to overwrite the settings file {}: the unparsable original could not \
                 be preserved ({e}); move it aside first",
                path.display()
            ),
        }
    }
}
