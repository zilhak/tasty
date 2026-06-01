//! `AppEvent::BusyPoll` 처리 — 모든 surface 의 busy 상태 갱신.

use crate::app::App;

impl App {
    /// Refresh the busy-surface cache for every live AppState. Triggered ~1s
    /// from the background ticker via `AppEvent::BusyPoll`. Marks any window
    /// whose set actually changed as dirty so the indicators redraw.
    pub(crate) fn poll_busy_states(&mut self) {
        for w in self.view.windows.values_mut() {
            let changed = match w.as_main_mut() {
                Some(main) => crate::core::Core::update_busy_surfaces(&mut main.engine_state),
                None => false,
            };
            if changed {
                w.mark_dirty();
            }
        }
        for (_, engine) in self.parked_states.iter_mut() {
            // parked 는 window 가 없어 redraw 의미가 없다. 반환값은 무시.
            crate::core::Core::update_busy_surfaces(engine);
        }
    }
}
