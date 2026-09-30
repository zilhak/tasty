//! 엔진별 훅 등록·감시 상태를 소유한다.
//! 여러 엔진이 공유하는 handler 정의 registry는 `hook_handler`에 있고, 여기에는 엔진마다 따로 두는 상태만 둔다.
//! 이름에 global이 붙은 전역 훅도 엔진별 등록이며 프로세스 전역 원본으로 합치지 않는다([ADR-0062](../../docs/adr/0062-task-service-and-hook-runtime.md)).

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64};

use tasty_hooks::{FiredHook, HookBinding, HookEvent, HookManager};

use crate::global_hooks::{GlobalHookManager, HookCondition};

/// surface 훅과 전역 훅의 엔진별 등록·감시 상태. ID 카운터는 다른 엔진과 공유한다.
pub(crate) struct HookRuntimeState {
    surface: HookManager,
    global: GlobalHookManager,
}

impl HookRuntimeState {
    pub(crate) fn with_counters(surface_ids: Arc<AtomicU64>, global_ids: Arc<AtomicU32>) -> Self {
        Self {
            surface: HookManager::with_counter(surface_ids),
            global: GlobalHookManager::with_counter(global_ids),
        }
    }

    pub(crate) fn surface_hooks(&self) -> &HookManager {
        &self.surface
    }

    pub(crate) fn global_hooks(&self) -> &GlobalHookManager {
        &self.global
    }

    pub(crate) fn add_surface_hook(
        &mut self,
        surface_id: u32,
        event: HookEvent,
        binding: HookBinding,
        once: bool,
    ) -> u64 {
        self.surface.add_hook(surface_id, event, binding, once)
    }

    pub(crate) fn remove_surface_hook(&mut self, hook_id: u64) -> bool {
        self.surface.remove_hook(hook_id)
    }

    /// 닫힌 surface의 훅을 모두 해제한다.
    pub(crate) fn forget_surface(&mut self, surface_id: u32) {
        self.surface.remove_surface_hooks(surface_id);
    }

    /// OutputMatch 훅이 있으면 PTY 출력 이벤트를 켜야 한다.
    pub(crate) fn has_output_match_hook(&self, surface_id: u32) -> bool {
        self.surface.has_output_match_hook(surface_id)
    }

    pub(crate) fn add_global_hook(
        &mut self,
        condition: HookCondition,
        command: String,
        label: Option<String>,
    ) -> u32 {
        self.global.add(condition, command, label)
    }

    pub(crate) fn remove_global_hook(&mut self, hook_id: u32) -> bool {
        self.global.remove(hook_id)
    }

    /// 발화 판정만 한다. 바인딩 실행과 HookFired 전달은 호출자가 맡는다.
    pub(crate) fn check_and_fire(
        &mut self,
        surface_id: u32,
        events: &[HookEvent],
    ) -> Vec<FiredHook> {
        self.surface.check_and_fire(surface_id, events)
    }

    pub(crate) fn check_idle_timeouts(
        &mut self,
        surface_id: u32,
        elapsed_secs: u64,
        last_output_at: std::time::Instant,
    ) -> Vec<FiredHook> {
        self.surface
            .check_idle_timeouts(surface_id, elapsed_secs, last_output_at)
    }

    /// 조건을 만족한 전역 훅의 명령을 돌려준다. 실행은 호출자가 맡는다.
    pub(crate) fn tick_global(&mut self) -> Vec<(u32, String)> {
        self.global.tick()
    }
}
