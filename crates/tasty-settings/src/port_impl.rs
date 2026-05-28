//! Production / test `SettingsStorage` 구현.

use crate::Settings;
use crate::port::SettingsStorage;

/// Production: `~/.tasty/config.toml` 의 file 기반.
#[derive(Debug, Default)]
pub struct FileSettingsStorage;

impl SettingsStorage for FileSettingsStorage {
    fn load(&self) -> Settings {
        Settings::load()
    }

    fn save(&self, settings: &Settings) -> anyhow::Result<()> {
        settings.save()
    }
}
