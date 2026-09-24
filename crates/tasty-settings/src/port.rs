//! config.toml 저장소와 시험용 메모리 저장소의 공통 인터페이스.

use crate::Settings;

pub trait SettingsStorage: Send + Sync {
    /// 디스크 (또는 fallback) 에서 설정 로드.
    fn load(&self) -> Settings;

    /// 디스크에 저장.
    fn save(&self, settings: &Settings) -> anyhow::Result<()>;
}
