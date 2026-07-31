//! `AppEvent::BusyPoll` 에 편승한 글로벌 훅(`tasty set global-hook`) 조건 평가/발화(TODO12).
//!
//! 글로벌 훅은 surface 가 아니라 `CoreState`(엔진) 단위로 등록되므로, busy-state 갱신과
//! 동일하게 창마다·parked 엔진마다 각자의 `GlobalHookManager` 를 개별적으로 tick 한다 —
//! 다른 엔진의 훅이 중복 발화되지 않는다.

use crate::app::App;

impl App {
    pub(crate) fn poll_global_hooks(&mut self) {
        for w in self.view.views.values_mut() {
            if let Some(main) = w.as_main_mut() {
                main.core_state.poll_global_hooks();
            }
        }
        for (_, engine) in self.parked_states.iter_mut() {
            engine.poll_global_hooks();
        }
    }
}
