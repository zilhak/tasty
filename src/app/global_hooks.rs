//! Busy tick에서 창과 parked engine의 전역 훅 조건을 각각 확인한다.

use crate::app::App;

impl App {
    pub(crate) fn poll_global_hooks(&mut self) {
        for engine in self.engines_mut().windowed_and_parked() {
            engine.poll_global_hooks();
        }
    }
}
