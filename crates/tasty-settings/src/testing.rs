//! In-memory `SettingsStorage` — test 시 disk 우회.

use std::sync::Mutex;
use std::sync::atomic::AtomicBool;

use tasty_utils::poison::recover_mutex;

use crate::Settings;
use crate::port::SettingsStorage;

/// 이 test double 의 락은 자료구조 임계구역이라 poison 을 복구한다. 조용한 복구는
/// 조용한 유실과 구분되지 않으므로 헬퍼로 첫-1 회 보고를 태운다.
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
