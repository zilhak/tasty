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
/// boot 가 `memory.db` 를 못 열면 [`memory_fallback_after`] 로 in-memory 대체 저장소를
/// 열어 넘긴다 — 앱 자체는 기동시키되, 그 저장소가 대체라는 사실을 진단·쓰기 응답이
/// 말하게 한다(ADR-0610). `memory` 가 `None` 이면(대체조차 못 연 경우) 여기서 한 번 더
/// 대체를 시도한다.
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

    // Memory: boot 에서 받은 Arc 를 dyn coerce. 없으면 in-memory 대체(원인 미상).
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
    // dyn 으로 접기 **전에** 게이지를 꺼낸다 — trait 에는 이 핸들을 낼 방법이 없고,
    // 여기가 구상 타입을 손에 쥐는 유일한 자리다.
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

/// 스토어가 자기 안에서 재는 DB 지연 게이지를 꺼낸다.
///
/// 뮤텍스를 한 번 잡지만 그 자리는 부팅이고, 이후 진단 읽기는 이 핸들만 쓰므로
/// 다시 안 잡는다 — 적체를 재려고 적체하는 자물쇠를 잡으면 진단이 같이 막힌다.
/// 부팅 시점이라 poison 은 사실상 불가능하지만(아직 아무도 이 스토어를 안 잡았다)
/// 복구는 공용 헬퍼로 한다 — 조용히 넘기면 "게이지를 못 꺼냈다" 가 로그에 안 남는다.
fn db_latency_of(
    store: &Arc<Mutex<tasty_memory::MemoryStore>>,
) -> Arc<tasty_memory::DbLatencyStats> {
    crate::poison::recover_mutex(store.lock(), MEMORY_WHAT, &MEMORY_POISONED).db_latency()
}

/// 스토어가 열 때 되읽은 연결 pragma 결과를 꺼낸다. 이유와 poison 복구는 위
/// [`db_latency_of`] 와 같다 — 열린 뒤로 안 바뀌는 값이라 복제본으로 충분하다.
fn memory_pragmas_of(
    store: &Arc<Mutex<tasty_memory::MemoryStore>>,
) -> tasty_memory::pragma::AppliedPragmas {
    crate::poison::recover_mutex(store.lock(), MEMORY_WHAT, &MEMORY_POISONED)
        .applied_pragmas()
        .clone()
}

/// 스토어가 `memory.db` 초기화 실패의 대체인지 꺼낸다. 이유와 poison 복구는 위
/// [`db_latency_of`] 와 같다 — 열린 뒤로 안 바뀌는 값이다.
fn memory_init_fallback_of(
    store: &Arc<Mutex<tasty_memory::MemoryStore>>,
) -> Option<tasty_memory::InitFallback> {
    crate::poison::recover_mutex(store.lock(), MEMORY_WHAT, &MEMORY_POISONED)
        .init_fallback()
        .cloned()
}

/// `memory.db` 초기화가 `err` 로 실패했을 때 쓸 in-memory 대체 저장소를 연다.
///
/// 부팅은 계속한다 — 손상된 파일로도 앱을 쓸 수 있게 하는 기존 동작이다. 대신 대체라는
/// 사실이 저장소에 실려 `system.pressure` 의 `db_pragmas.memory_db` 가 `degraded` 로,
/// 쓰기 응답이 `durable: false` 로 말한다(ADR-0610). 대체조차 못 열면 `None` 이고,
/// 그때는 [`build_production_core_inner`] 가 한 번 더 시도한다.
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

/// 위 복구의 공용 보고 좌표(첫-1 회).
static MEMORY_POISONED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
const MEMORY_WHAT: &str = "memory store (db latency handle)";

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
}

#[cfg(test)]
mod tests {
    use super::*;
    use tasty_memory::{MemoryValue, PutOpts, Scope};

    /// 꺼낸 pragma 결과가 **그 스토어가 되읽은 값**인지. 파일 DB 로 열어야 in-memory
    /// 기본값과 갈린다 — 기본값을 지어내면 `in_memory` 와 `journal_mode` 가 틀린다.
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

    /// 대체 저장소를 여는 자리가 원인을 실은 저장소를 넘기고, 꺼내는 자리가 **그 저장소의
    /// 값**을 준다. 파일 DB 는 대체가 아니다.
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

    /// 게이지를 꺼내는 자리가 **스토어가 올리는 그 게이지**를 주는지. 새 기본값을
    /// 돌려줘도 타입은 맞으므로, 짝이 맞는지는 실제 쓰기를 한 건 일으켜 봐야 갈린다.
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
