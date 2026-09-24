//! In-memory `SettingsStorage` — test 시 disk 우회.

use std::sync::Mutex;
use std::sync::atomic::AtomicBool;

use tasty_utils::poison::recover_mutex;

use crate::Settings;
use crate::port::SettingsStorage;

/// 메모리 자료구조 락의 poison은 복구하고 처음 한 번 보고한다.
const STORE_WHAT: &str = "the in-memory settings store";
static STORE_POISON_REPORTED: AtomicBool = AtomicBool::new(false);

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
        recover_mutex(self.inner.lock(), STORE_WHAT, &STORE_POISON_REPORTED).clone()
    }

    fn save(&self, settings: &Settings) -> anyhow::Result<()> {
        *recover_mutex(self.inner.lock(), STORE_WHAT, &STORE_POISON_REPORTED) = settings.clone();
        Ok(())
    }
}
