//! Core에 파일시스템·시계·프로세스·저장소 등 실제 구현을 연결한다.

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

/// 부팅에서 연 저장소 Arc를 Core와 하위 사용자가 공유한다. 없으면 메모리 대체 저장소를 시도한다.
#[cfg(feature = "gui")]
pub(crate) fn build_production_core(
    memory_arc: Option<Arc<Mutex<tasty_memory::MemoryStore>>>,
) -> anyhow::Result<Core> {
    let clipboard: Arc<dyn crate::ports::clipboard::ClipboardSystem> = Arc::new(ArboardClipboard);
    let sound_player: Arc<dyn crate::ports::notification_sound::NotificationSoundPlayer> =
        Arc::new(PlatformPlayer);
    build_production_core_inner(clipboard, sound_player, memory_arc)
}

/// 헤드리스는 로컬 클립보드·소리 대신 미지원 구현을 연결한다.
#[cfg(not(feature = "gui"))]
pub(crate) fn build_production_core_headless(
    memory_arc: Option<Arc<Mutex<tasty_memory::MemoryStore>>>,
) -> anyhow::Result<Core> {
    let clipboard: Arc<dyn crate::ports::clipboard::ClipboardSystem> = Arc::new(NullClipboard);
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

    let store: Arc<Mutex<tasty_memory::MemoryStore>> = match memory_arc {
        Some(arc) => arc,
        None => {
            let unknown = tasty_memory::MemoryInitError::Other(
                "no memory store was handed over at boot".into(),
            );
            let store = tasty_memory::MemoryStore::open_in_memory_after_init_failure(&unknown)
                .map_err(|e| anyhow::anyhow!("fallback memory store: {e:?}"))?;
            Arc::new(Mutex::new(store))
        }
    };
    // trait object로 바꾸기 전에 구체 저장소가 제공하는 진단 핸들을 꺼낸다.
    let db_latency = db_latency_of(&store);
    let memory_pragmas = memory_pragmas_of(&store);
    let memory_init_fallback = memory_init_fallback_of(&store);
    let memory: Arc<Mutex<dyn MemoryStorage>> = store;

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
        .with_db_latency(db_latency)
        .with_memory_pragmas(memory_pragmas)
        .with_memory_init_fallback(memory_init_fallback)
        .with_themes(themes)
        .with_preset_store(preset_store)
        .with_settings_storage(settings_storage)
        .build()
}

/// 부팅 때 공유 게이지를 꺼내 두어 이후 지연 조회가 DB mutex를 다시 기다리지 않게 한다.
fn db_latency_of(
    store: &Arc<Mutex<tasty_memory::MemoryStore>>,
) -> Arc<tasty_memory::DbLatencyStats> {
    crate::poison::recover_mutex(store.lock(), MEMORY_WHAT, &MEMORY_POISONED).db_latency()
}

/// 저장소를 열 때 확인한 pragma 결과를 복사한다. 현재 연결을 다시 조회하는 것은 아니다.
fn memory_pragmas_of(
    store: &Arc<Mutex<tasty_memory::MemoryStore>>,
) -> tasty_memory::pragma::AppliedPragmas {
    crate::poison::recover_mutex(store.lock(), MEMORY_WHAT, &MEMORY_POISONED)
        .applied_pragmas()
        .clone()
}

/// 메모리 대체 저장소 여부와 원인을 가져온다.
fn memory_init_fallback_of(
    store: &Arc<Mutex<tasty_memory::MemoryStore>>,
) -> Option<tasty_memory::InitFallback> {
    crate::poison::recover_mutex(store.lock(), MEMORY_WHAT, &MEMORY_POISONED)
        .init_fallback()
        .cloned()
}

/// 파일 저장소 실패 시 메모리 저장소로 계속할 수 있게 한다. 쓰기는 재시작 후 남지 않는다.
/// 대체 저장소도 열지 못하면 None이며 Core 구성 단계에서 한 번 더 시도한다.
pub(crate) fn memory_fallback_after(
    err: &tasty_memory::MemoryInitError,
) -> Option<Arc<Mutex<tasty_memory::MemoryStore>>> {
    match tasty_memory::MemoryStore::open_in_memory_after_init_failure(err) {
        Ok(store) => {
            tracing::warn!(
                "memory.db falls back to an in-memory store (cause={}) — writes will not survive a restart",
                err.cause()
            );
            Some(Arc::new(Mutex::new(store)))
        }
        Err(e) => {
            tracing::error!("in-memory fallback for memory.db failed to open: {e}");
            None
        }
    }
}

static MEMORY_POISONED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
const MEMORY_WHAT: &str = "memory store (db latency handle)";

/// 헤드리스 로컬 클립보드는 지원하지 않으며 호출되면 오류를 반환한다.
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use tasty_memory::{MemoryValue, PutOpts, Scope};

    /// 메모리 기본값이 아니라 실제 파일 저장소에서 읽은 pragma를 전달해야 한다.
    #[test]
    fn the_extracted_pragmas_are_the_ones_the_store_read_back() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Arc::new(Mutex::new(
            tasty_memory::MemoryStore::open(&tmp.path().join("memory.db")).unwrap(),
        ));
        let pragmas = memory_pragmas_of(&store);
        assert!(!pragmas.in_memory);
        assert_eq!(
            pragmas.get("journal_mode").unwrap().effective.as_deref(),
            Some("wal")
        );
        assert_eq!(&pragmas, store.lock().unwrap().applied_pragmas());
    }

    #[test]
    fn the_fallback_store_carries_the_init_failure_and_a_file_store_does_not() {
        let err = tasty_memory::MemoryInitError::Corrupt("/x/memory.db".into());
        let store = memory_fallback_after(&err).expect("fallback opens");
        let fallback = memory_init_fallback_of(&store).expect("fallback is marked");
        assert_eq!(fallback.cause, "corrupt");
        assert_eq!(fallback.error, err.to_string());

        let tmp = tempfile::tempdir().unwrap();
        let file = Arc::new(Mutex::new(
            tasty_memory::MemoryStore::open(&tmp.path().join("memory.db")).unwrap(),
        ));
        assert_eq!(memory_init_fallback_of(&file), None);
    }

    /// 새 기본 게이지가 아니라 실제 저장소의 쓰기를 기록한 공유 게이지여야 한다.
    #[test]
    fn the_extracted_gauge_is_the_one_the_store_raises() {
        let store = Arc::new(Mutex::new(
            tasty_memory::MemoryStore::open_in_memory().unwrap(),
        ));
        let gauge = db_latency_of(&store);
        assert_eq!(gauge.snapshot().commits, 0);

        store
            .lock()
            .unwrap()
            .put(
                "com.tasty.test",
                &Scope::Surface(1),
                "k",
                &MemoryValue::Text("v".into()),
                &PutOpts::default(),
            )
            .unwrap();

        assert_eq!(
            gauge.snapshot().commits,
            1,
            "꺼낸 게이지가 스토어의 것이 아니다"
        );
    }
}
