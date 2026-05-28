//! `Core` — 도메인 본체 + 단일 mutate 진입점 + 외부 자원 (port) 주입.
//!
//! ```text
//! handler   ──read──>   &CoreState        (외부 read-only 노출)
//! handler   ──enqueue─> Intent            (HandlerCtx.intents)
//! dispatch  ──drain──>  Core::apply(...)  (Core 만이 mutate)
//! Core::apply ──mutate self.state          (도메인 일관성 보장)
//! ```
//!
//! Phase D 진행 중. 현재는 *기반 골격* — D.3.C 의 도메인 마이그레이션 때
//! 호출처 들이 본 Core 의 메서드 / port 들을 통해 동작.

pub(crate) mod builder;
pub(crate) mod intent;

use std::sync::{Arc, Mutex};

use intent::{CoreEvent, CoreIntent};
use tasty_memory::MemoryStorage;
use tasty_presets::PresetStorage;
use tasty_settings::SettingsStorage;
use tasty_themes::ThemeStorage;

use crate::ports::clipboard::ClipboardSystem;
use crate::ports::clock::Clock;
use crate::ports::fs::FileSystem;
use crate::ports::home::HomeDirectory;
use crate::ports::process::ProcessSpawner;
use crate::ports::pty::{PtyService, TerminalWaker};

/// 도메인 데이터 — handler 가 받는 read-only 인터페이스.
pub(crate) struct CoreState {
    /// Layout preset 디스크 캐시 (`~/.tasty/presets/`). App 전역 단일 인스턴스.
    /// `Arc<Mutex<>>` 로 MainWindow / PresetWindow / EngineState 모두에 공유한다 —
    /// 단일 source of truth.
    pub preset_store: Arc<Mutex<tasty_presets::PresetStore>>,
    // sub-step 마다 더 추가됨 (plan 참조).
}

impl CoreState {
    pub(crate) fn new() -> Self {
        Self {
            preset_store: Arc::new(Mutex::new(tasty_presets::PresetStore::load_default())),
        }
    }
}

/// 도메인 본체. `state` 를 보유하며, 외부에는 `state()` 로 read-only 만 노출한다.
/// 변경은 `apply(intent)` 단일 진입점.
///
/// 11 outbound port (7 external + 4 internal) 를 `Arc<dyn>` 으로 보유. test 시
/// `CoreBuilder::for_test()` 로 mock 주입.
#[allow(dead_code)]
pub(crate) struct Core {
    state: CoreState,

    // ─── External ports (bin 안 정의, src/ports/) ───
    pty: Arc<dyn PtyService>,
    waker: Arc<dyn TerminalWaker>,
    fs: Arc<dyn FileSystem>,
    clock: Arc<dyn Clock>,
    clipboard: Arc<dyn ClipboardSystem>,
    process: Arc<dyn ProcessSpawner>,
    home: Arc<dyn HomeDirectory>,

    // ─── Internal crate trait ports ───
    /// `Sync` 아님 (SQLite Connection 의 `RefCell` 캐시) — `Mutex` 보호.
    memory: Arc<Mutex<dyn MemoryStorage>>,
    themes: Arc<dyn ThemeStorage>,
    /// `&mut self` 메서드 (save/delete/rename) 가 있어 `Mutex` 보호.
    presets: Arc<Mutex<dyn PresetStorage>>,
    settings_storage: Arc<dyn SettingsStorage>,
}

impl Core {
    /// 도메인 데이터의 read-only 참조.
    pub(crate) fn state(&self) -> &CoreState {
        &self.state
    }

    /// (Phase D 진행 중 임시) Phase D 완료 시 제거 — 모든 mutate 는 `apply` 통해서만.
    #[allow(dead_code)]
    pub(crate) fn state_mut(&mut self) -> &mut CoreState {
        &mut self.state
    }

    /// 도메인 변경의 단일 진입점. handler 가 발행한 `CoreIntent` 를 받아
    /// `self.state` 를 자기 메서드로만 mutate. 결과 이벤트 목록을 반환.
    #[allow(dead_code)]
    pub(crate) fn apply(&mut self, intent: CoreIntent) -> anyhow::Result<Vec<CoreEvent>> {
        match intent {
            // Phase D 진행 중. variant 추가 시 본 match 도 채워진다.
        }
    }
}
