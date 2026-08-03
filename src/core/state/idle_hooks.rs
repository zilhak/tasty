//! IdleTimeout 훅(`tasty set hook --event idle-timeout:SECS`) 폴링.
//! `AppEvent::BusyPoll` 1Hz cadence 에 편승해 호출된다 — `global_hooks.rs`/
//! `GlobalHookManager::tick()` 와 동일한 이유로 전용 ticker 스레드를 새로 두지
//! 않는다.

use std::collections::HashSet;

use super::CoreState;

impl CoreState {
    /// `IdleTimeout` 훅이 걸린 surface 들을 순회해 idle 경과시간을 확인하고
    /// 발사된 훅을 `(surface_id, FiredHook)` 쌍으로 반환한다.
    ///
    /// 이 레이어는 `HostIpcInjector`/`AppState` 를 모르는 순수 engine 레이어라
    /// 바인딩 실행 + host event enqueue 는 하지 않는다 — 호출자
    /// (`App::poll_idle_timeout_hooks`)가 담당한다(`cascade_terminal_bell_ring`
    /// 과 동일한 책임 분리).
    pub(crate) fn poll_idle_timeout_hooks(&mut self) -> Vec<(u32, tasty_hooks::FiredHook)> {
        let surface_ids: HashSet<u32> = self
            .hook_manager
            .list_hooks(None)
            .iter()
            .filter(|h| matches!(h.event, tasty_hooks::HookEvent::IdleTimeout(_)))
            .map(|h| h.surface_id)
            .collect();

        let mut fired = Vec::new();
        for sid in surface_ids {
            let Some(terminal) = self.terminals.get(sid) else {
                continue;
            };
            let last_output_at = terminal.last_output_at();
            let elapsed_secs = last_output_at.elapsed().as_secs();
            for f in self
                .hook_manager
                .check_idle_timeouts(sid, elapsed_secs, last_output_at)
            {
                fired.push((sid, f));
            }
        }
        fired
    }
}
