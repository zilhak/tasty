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
/// Memory: boot 가 `tasty_memory::init_with_config` 로 새 `Arc<Mutex<MemoryStore>>`
/// 를 만들어 본 함수에 전달. Core 가 그 Arc 의 유일 owner — 모든 하위 표면
/// (AppState.memory, CoreState.memory, worker thread capture) 은 Core 의 Arc clone 을 공유한다.
///
/// `memory` 가 `None` 이면 (homedir 미확인 등 boot fail), in-memory placeholder
/// 로 fallback 해 앱 자체는 기동시킨다 — handler 가 자체적으로 store 의 가용성을 평가.
pub(crate) fn build_production_core(
    proxy: EventLoopProxy<AppEvent>,
    memory_arc: Option<Arc<Mutex<tasty_memory::MemoryStore>>>,
) -> anyhow::Result<Core> {
    let pty: Arc<dyn crate::ports::pty::PtyService> = Arc::new(PortablePtyService);
    let waker: Arc<dyn crate::ports::pty::TerminalWaker> = Arc::new(WinitWaker::new(proxy));
    let fs: Arc<dyn crate::ports::fs::FileSystem> = Arc::new(StdFileSystem);
    let clock: Arc<dyn crate::ports::clock::Clock> = Arc::new(SystemClock);
    let clipboard: Arc<dyn crate::ports::clipboard::ClipboardSystem> = Arc::new(ArboardClipboard);
    let process: Arc<dyn crate::ports::process::ProcessSpawner> = Arc::new(StdProcessSpawner);
    let home: Arc<dyn crate::ports::home::HomeDirectory> = Arc::new(DirectoriesHome);

    // Memory: boot 에서 받은 Arc 를 dyn coerce. 실패 시 in-memory fallback.
    let memory: Arc<Mutex<dyn MemoryStorage>> = match memory_arc {
        Some(arc) => arc,
        None => {
            let store = tasty_memory::MemoryStore::open_in_memory()
                .map_err(|e| anyhow::anyhow!("fallback memory store: {e:?}"))?;
            Arc::new(Mutex::new(store))
        }
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
