//! Busy tick에서 surface별 IdleTimeout 훅을 확인한다.
//! 판정과 바인딩 실행은 engine의 HookRuntimeState가, 호스트 이벤트 전달은 App이 맡는다.
//! engine 단위 GlobalHookManager와는 별개다.

use crate::app::App;

impl App {
    pub(crate) fn poll_idle_timeout_hooks(&mut self) {
        let exec = self.core.hook_executor();

        for (_, main, engine) in self.engines_mut().window_pairs() {
            let fired = engine.fire_idle_timeout_hooks(&exec);
            if fired.is_empty() {
                continue;
            }
            for event in fired {
                main.state.enqueue_host_event(event);
            }
            main.base.dirty = true;
        }

        // 화면이 없는 parked engine도 계속 판정한다.
        for (state, engine) in self.engines_mut().parked_sessions() {
            for event in engine.fire_idle_timeout_hooks(&exec) {
                state.enqueue_host_event(event);
            }
        }
    }
}
