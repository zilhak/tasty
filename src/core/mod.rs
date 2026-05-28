//! `Core` — 도메인 본체. GUI 가 떠 있든 떠 있지 않든 항상 존재하는 상태.
//!
//! Phase C 의 strangler fig 마이그레이션 중. 현재는 빈 골격이며, `Engine` /
//! `EngineState` 의 필드를 sub-step 마다 한 그룹씩 이동시킨다. 마이그레이션
//! 계획은 `.claude-workspace/plans/phase-c-strangler-restructure.md`.

#[allow(dead_code)]
pub(crate) struct Core {
    // sub-step 마다 한 필드씩 추가됨.
    // C.1.1 — preset_store
    // C.2.1 — workspaces, next_ids, default_cols/rows, waker, waker_factory
    // ... (plan 참조)
}

impl Core {
    pub(crate) fn new() -> Self {
        Self {}
    }
}
