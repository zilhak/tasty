//! Production DI wiring — `Core` 의 11 port 에 production adapter 주입.
//!
//! 호출처는 `App::new` (또는 향후 entrypoint). test 시 별 wiring (`CoreBuilder`
//! 에 mock adapter 주입).

use std::sync::{Arc, Mutex};

use tasty_memory::MemoryStorage;
use tasty_presets::PresetStore;
use tasty_settings::{FileSettingsStorage, SettingsStorage};
use tasty_themes::{ThemeStorage, ThemeStore};
use winit::event_loop::EventLoopProxy;

use crate::AppEvent;
use crate::adapters::production::{
    arboard_clip::ArboardClipboard, directories_home::DirectoriesHome,
    portable_pty::PortablePtyService, std_clock::SystemClock, std_fs::StdFileSystem,
    std_process::StdProcessSpawner, winit_waker::WinitWaker,
};
use crate::core::Core;
use crate::core::builder::CoreBuilder;

/// Production `Core` 빌드. winit proxy 가 필요한 `WinitWaker` 때문에 인자 1 개.
///
/// Memory: production 시 `tasty_memory::with_store` 의 전역 static 과 *별 instance*
/// 가 잠시 공존 (D.3.A.4 의 호환 layer). D.3.C 에서 host 호출처 마이그레이션 시
/// 전역 static 폐기 + 본 instance 가 유일 owner.
///
/// 현재 단계 — 호출처 0 (placeholder). D.3.A.7 또는 D.3.C 에서 사용.
#[allow(dead_code)]
pub(crate) fn build_production_core(proxy: EventLoopProxy<AppEvent>) -> anyhow::Result<Core> {
    let pty: Arc<dyn crate::ports::pty::PtyService> = Arc::new(PortablePtyService);
    let waker: Arc<dyn crate::ports::pty::TerminalWaker> = Arc::new(WinitWaker::new(proxy));
    let fs: Arc<dyn crate::ports::fs::FileSystem> = Arc::new(StdFileSystem);
    let clock: Arc<dyn crate::ports::clock::Clock> = Arc::new(SystemClock);
    let clipboard: Arc<dyn crate::ports::clipboard::ClipboardSystem> = Arc::new(ArboardClipboard);
    let process: Arc<dyn crate::ports::process::ProcessSpawner> = Arc::new(StdProcessSpawner);
    let home: Arc<dyn crate::ports::home::HomeDirectory> = Arc::new(DirectoriesHome);

    // Memory: in-memory 인스턴스 임시 생성. 실 SQLite 사용은 D.3.C 에서 `MemoryStore::open_with_config`.
    let memory: Arc<Mutex<dyn MemoryStorage>> = {
        let store = tasty_memory::MemoryStore::open_in_memory()
            .map_err(|e| anyhow::anyhow!("placeholder memory store: {e:?}"))?;
        Arc::new(Mutex::new(store))
    };

    let themes: Arc<dyn ThemeStorage> = Arc::new(ThemeStore::new());
    let preset_store: Arc<Mutex<PresetStore>> = Arc::new(Mutex::new(PresetStore::load_default()));
    let settings_storage: Arc<dyn SettingsStorage> = Arc::new(FileSettingsStorage);

    CoreBuilder::new()
        .with_pty(pty)
        .with_waker(waker)
        .with_fs(fs)
        .with_clock(clock)
        .with_clipboard(clipboard)
        .with_process(process)
        .with_home(home)
        .with_memory(memory)
        .with_themes(themes)
        .with_preset_store(preset_store)
        .with_settings_storage(settings_storage)
        .build()
}
