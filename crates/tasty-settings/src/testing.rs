//! In-memory `SettingsStorage` — test 시 disk 우회.

use std::sync::Mutex;

use crate::Settings;
use crate::port::SettingsStorage;

#[derive(Debug, Default)]
pub struct InMemorySettingsStorage {
    inner: Mutex<Settings>,
}

impl InMemorySettingsStorage {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Settings::default()),
        }
    }

    pub fn with_settings(settings: Settings) -> Self {
        Self {
            inner: Mutex::new(settings),
        }
    }
}

impl SettingsStorage for InMemorySettingsStorage {
    fn load(&self) -> Settings {
        self.inner.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    fn save(&self, settings: &Settings) -> anyhow::Result<()> {
        *self.inner.lock().unwrap_or_else(|p| p.into_inner()) = settings.clone();
        Ok(())
    }
}
