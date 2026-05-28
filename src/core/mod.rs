//! `Core` — 도메인 본체. GUI 가 떠 있든 떠 있지 않든 항상 존재하는 상태.
//!
//! Phase C 의 strangler fig 마이그레이션 중. 현재는 빈 골격이며, `Engine` /
//! `EngineState` 의 필드를 sub-step 마다 한 그룹씩 이동시킨다. 마이그레이션
//! 계획은 `.claude-workspace/plans/phase-c-strangler-restructure.md`.

use std::sync::{Arc, Mutex};

pub(crate) struct Core {
    /// Layout preset 디스크 캐시 (`~/.tasty/presets/`). App 전역 단일 인스턴스.
    /// `Arc<Mutex<>>` 로 MainWindow / PresetWindow / EngineState 모두에 공유한다 —
    /// 단일 source of truth.
    pub preset_store: Arc<Mutex<tasty_presets::PresetStore>>,
    // sub-step 마다 더 추가됨 (plan 참조).
}

impl Core {
    pub(crate) fn new() -> Self {
        Self {
            preset_store: Arc::new(Mutex::new(tasty_presets::PresetStore::load_default())),
        }
    }
}
