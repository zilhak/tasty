//! 공용 busy 타이머에서 IdleTimeout 훅을 확인한다.

use super::CoreState;
use crate::core::host_event::PendingHostEvent;

impl CoreState {
    /// 터미널의 마지막 출력 시각으로 IdleTimeout 훅을 발화하고 HookFired를 돌려준다.
    /// host 이벤트 등록은 호출자가 맡는다.
    pub(crate) fn fire_idle_timeout_hooks(
        &mut self,
        exec: &crate::hook_runtime::HookExecutor,
    ) -> Vec<PendingHostEvent> {
        let terminals = &self.terminals;
        self.hooks
            .fire_idle_timeouts(exec, |sid| terminals.get(sid).map(|t| t.last_output_at()))
    }
}
