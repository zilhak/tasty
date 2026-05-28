//! `Core` — 도메인 본체. GUI 가 떠 있든 떠 있지 않든 항상 존재하는 상태.
//!
//! Phase C 의 strangler fig 마이그레이션 중. 현재는 빈 골격이며, `Engine` /
//! `EngineState` 의 필드를 sub-step 마다 한 그룹씩 이동시킨다. 마이그레이션
//! 계획은 `.claude-workspace/plans/phase-c-strangler-restructure.md`.

//! `Core` — 도메인 본체 + 단일 mutate 진입점.
//!
//! ```text
//! handler   ──read──>   &CoreState        (외부 read-only 노출)
//! handler   ──enqueue─> Intent            (HandlerCtx.intents)
//! dispatch  ──drain──>  Core::apply(...)  (Core 만이 mutate)
//! Core::apply ──mutate self.state          (도메인 일관성 보장)
//! ```
//!
//! Phase D 진행 중. 현재는 골격 — EngineState 의 데이터를 점진 흡수해 가며
//! 외부 노출 어휘를 `CoreState` 로, mutate 어휘를 `Core` 로 분리한다.

use std::sync::{Arc, Mutex};

/// 도메인 데이터 — handler 가 받는 read-only 인터페이스.
///
/// 현재 Phase D 초기: `preset_store` 만 보유. EngineState 의 데이터 필드들을
/// 점진 이전한다.
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
/// 변경은 `apply(intent)` 단일 진입점으로 — Phase D 진행 중 점진 도입.
pub(crate) struct Core {
    state: CoreState,
}

impl Core {
    pub(crate) fn new() -> Self {
        Self {
            state: CoreState::new(),
        }
    }

    /// 도메인 데이터의 read-only 참조.
    pub(crate) fn state(&self) -> &CoreState {
        &self.state
    }

    /// (Phase D 진행 중 임시) mutate 진입점이 아직 모든 도메인 변경을 cover 하지
    /// 않으므로, 본 phase 마이그레이션 동안 *제한적* mutable 참조 노출.
    /// Phase D 완료 시점에 제거 — 모든 mutate 는 `apply` 통해서만.
    #[allow(dead_code)]
    pub(crate) fn state_mut(&mut self) -> &mut CoreState {
        &mut self.state
    }
}
