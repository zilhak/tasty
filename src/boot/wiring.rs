//! Production DI wiring — `Core` 의 10 port 에 production adapter 주입.
//!
//! 호출처는 `App::new` (또는 향후 entrypoint). test 시 별 wiring (`CoreBuilder`
//! 에 mock adapter 주입).

use std::sync::{Arc, Mutex};

use tasty_memory::MemoryStorage;
use tasty_presets::PresetStore;
use tasty_settings::{FileSettingsStorage, SettingsStorage};
use tasty_themes::{ThemeStorage, ThemeStore};

#[cfg(feature = "gui")]
use crate::adapters::production::arboard_clip::ArboardClipboard;
#[cfg(feature = "gui")]
use crate::adapters::production::notification_sound::PlatformPlayer;
use crate::adapters::production::{
    directories_home::DirectoriesHome, std_clock::SystemClock, std_fs::StdFileSystem,
    std_process::StdProcessSpawner,
};
use crate::core::Core;
use crate::core::builder::CoreBuilder;

/// Production `Core` 빌드.
///
/// Memory: boot 가 `tasty_memory::init_with_config` 로 새 `Arc<Mutex<MemoryStore>>`
/// 를 만들어 본 함수에 전달. Core 가 그 Arc 의 유일 owner — 모든 하위 표면
/// (AppState.memory, CoreState.memory, worker thread capture) 은 Core 의 Arc clone 을 공유한다.
///
/// `memory` 가 `None` 이면 (homedir 미확인 등 boot fail), in-memory placeholder
/// 로 fallback 해 앱 자체는 기동시킨다 — handler 가 자체적으로 store 의 가용성을 평가.
#[cfg(feature = "gui")]
pub(crate) fn build_production_core(
    memory_arc: Option<Arc<Mutex<tasty_memory::MemoryStore>>>,
) -> anyhow::Result<Core> {
    let clipboard: Arc<dyn crate::ports::clipboard::ClipboardSystem> = Arc::new(ArboardClipboard);
    // PlatformPlayer 는 OS 별 alias — macOS+gui = MacBeepPlayer, Windows =
    // WinBeepPlayer, Linux = LinuxBeepPlayer, headless macOS / 그 외 = NoopPlayer.
    let sound_player: Arc<dyn crate::ports::notification_sound::NotificationSoundPlayer> =
        Arc::new(PlatformPlayer);
    build_production_core_inner(clipboard, sound_player, memory_arc)
}

/// Headless variant — gui-only adapter 의존성 (arboard) 을 제외.
#[cfg(not(feature = "gui"))]
pub(crate) fn build_production_core_headless(
    memory_arc: Option<Arc<Mutex<tasty_memory::MemoryStore>>>,
) -> anyhow::Result<Core> {
    // headless 빌드는 clipboard 가 안 쓰이지만 ClipboardSystem trait 를 구현하는
    // null placeholder 로 채워 Core 시그니처를 만족시킨다.
    let clipboard: Arc<dyn crate::ports::clipboard::ClipboardSystem> = Arc::new(NullClipboard);
    // headless 빌드도 sound 재생 미지원 — NoopPlayer 명시 주입.
    let sound_player: Arc<dyn crate::ports::notification_sound::NotificationSoundPlayer> =
        Arc::new(crate::ports::notification_sound::NoopPlayer);
    build_production_core_inner(clipboard, sound_player, memory_arc)
}

fn build_production_core_inner(
    clipboard: Arc<dyn crate::ports::clipboard::ClipboardSystem>,
    sound_player: Arc<dyn crate::ports::notification_sound::NotificationSoundPlayer>,
    memory_arc: Option<Arc<Mutex<tasty_memory::MemoryStore>>>,
) -> anyhow::Result<Core> {
    let fs: Arc<dyn crate::ports::fs::FileSystem> = Arc::new(StdFileSystem);
    let clock: Arc<dyn crate::ports::clock::Clock> = Arc::new(SystemClock);
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
        .with_fs(fs)
        .with_clock(clock)
        .with_clipboard(clipboard)
        .with_process(process)
        .with_home(home)
        .with_sound_player(sound_player)
        .with_memory(memory)
        .with_themes(themes)
        .with_preset_store(preset_store)
        .with_settings_storage(settings_storage)
        .build()
}

/// Headless 빌드용 no-op ClipboardSystem. clipboard 호출은 IPC 표면에서
/// `MethodNotFound` 로 차단되므로 실제 호출은 도달하지 않는다.
#[cfg(not(feature = "gui"))]
struct NullClipboard;

#[cfg(not(feature = "gui"))]
impl crate::ports::clipboard::ClipboardSystem for NullClipboard {
    fn read_text(&self) -> anyhow::Result<String> {
        anyhow::bail!("clipboard unavailable in headless build")
    }
    fn write_text(&self, _text: &str) -> anyhow::Result<()> {
        anyhow::bail!("clipboard unavailable in headless build")
    }
    fn read_image(&self) -> anyhow::Result<crate::ports::clipboard::ClipboardImage> {
        anyhow::bail!("clipboard unavailable in headless build")
    }
    fn write_image(&self, _image: &crate::ports::clipboard::ClipboardImage) -> anyhow::Result<()> {
        anyhow::bail!("clipboard unavailable in headless build")
    }
}
