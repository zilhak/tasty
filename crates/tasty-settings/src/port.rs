//! `SettingsStorage` trait — Hexagonal architecture 의 *internal port*.
//!
//! Production: `FileSettingsStorage` — `~/.tasty/settings.toml` 읽기/쓰기.
//! Test: `testing::InMemorySettingsStorage`.

use crate::Settings;

pub trait SettingsStorage: Send + Sync {
    /// 디스크 (또는 fallback) 에서 설정 로드.
    fn load(&self) -> Settings;

    /// 디스크에 저장.
    fn save(&self, settings: &Settings) -> anyhow::Result<()>;
}
