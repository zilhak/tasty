//! `Core` — 도메인 본체 + 단일 mutate 진입점 + 외부 자원 (port) 주입.
//!
//! ```text
//! handler   ──read──>   &CoreState        (외부 read-only 노출)
//! handler   ──enqueue─> Intent            (HandlerCtx.intents)
//! dispatch  ──drain──>  Core::apply(...)  (Core 만이 mutate)
//! Core::apply ──mutate self.state          (도메인 일관성 보장)
//! ```
//!
//! Phase D 진행 중. 본 Core 는 11 outbound port + preset_store 직속 보유만.
//! 도메인 데이터 (`CoreState`) 는 `crate::engine_state` 에 — App.engine_state
//! 가 main owner. D.3.C 의 도메인 마이그레이션으로 점진 흡수 예정.

pub(crate) mod builder;
pub(crate) mod intent;

use std::sync::{Arc, Mutex};

use intent::{CoreEvent, CoreIntent};
use tasty_memory::MemoryStorage;
use tasty_presets::{PresetStorage, PresetStore};
use tasty_settings::SettingsStorage;
use tasty_themes::ThemeStorage;

use crate::ports::clipboard::ClipboardSystem;
use crate::ports::clock::Clock;
use crate::ports::fs::FileSystem;
use crate::ports::home::HomeDirectory;
use crate::ports::process::ProcessSpawner;
use crate::ports::pty::{PtyService, TerminalWaker};

/// 도메인 본체. 11 outbound port (7 external + 4 internal) + preset_store 직속.
///
/// 도메인 데이터 (`crate::engine_state::CoreState`) 는 본 struct 가 아닌
/// `App.engine_state` 가 main owner — Phase D 진행 중의 *공존 layer*. D.3.C
/// 에서 점진 흡수.
#[allow(dead_code)]
pub(crate) struct Core {
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
    /// `preset_store` 와 *같은 allocation* (coerce 된 trait Arc).
    presets: Arc<Mutex<dyn PresetStorage>>,
    settings_storage: Arc<dyn SettingsStorage>,

    /// Layout preset 디스크 캐시. 구체 Arc — MainWindow / PresetWindow 에 clone
    /// 으로 전달해 *공유 owner* 가 된다. `presets` (trait Arc) 와 같은 allocation.
    pub(crate) preset_store: Arc<Mutex<PresetStore>>,
}

impl Core {
    /// 도메인 변경의 단일 진입점. handler 가 발행한 `CoreIntent` 를 받아
    /// 결과 이벤트 목록을 반환. Phase D 진행 중 — variant 추가 시 본 match 도 채움.
    #[allow(dead_code)]
    pub(crate) fn apply(&mut self, intent: CoreIntent) -> anyhow::Result<Vec<CoreEvent>> {
        match intent {
            CoreIntent::UpdateSettings(new_settings) => {
                // Phase D 진행 중 — 본 stub 은 *이벤트만 발행*. cascade
                // (Theme apply / Scrollback limit / clipboard max / notification
                // coalesce) 는 후속 sub-step (호출처 전환과 함께) 에서 통합.
                Ok(vec![CoreEvent::SettingsUpdated(new_settings)])
            }
        }
    }
}
