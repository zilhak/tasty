//! 공용 busy 타이머에서 IdleTimeout 훅을 확인한다.

use std::collections::HashSet;

use super::CoreState;

impl CoreState {
    /// 마지막 출력 후 경과로 실행할 훅을 고른다. 바인딩 실행과 host 이벤트 등록은 호출자가 맡는다.
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
