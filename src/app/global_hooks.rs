//! Busy tick에서 창과 parked engine의 전역 훅 조건을 각각 확인한다.

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
