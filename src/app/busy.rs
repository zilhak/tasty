//! `AppEvent::BusyPoll` 처리 — 모든 surface 의 busy 상태 갱신.

use crate::app::App;

impl App {
    /// Refresh the busy-surface cache for every live AppState. Triggered ~1s
    /// from the background ticker via `AppEvent::BusyPoll`. Marks any window
    /// whose set actually changed as dirty so the indicators redraw.
    pub(crate) fn poll_busy_states(&mut self) {
        for w in self.windows.values_mut() {
            let changed = match w.as_main_mut() {
                Some(main) => main.state.engine.refresh_busy_surfaces(),
                None => false,
            };
            if changed {
                w.mark_dirty();
            }
        }
        for state in &mut self.parked_states {
            // parked state는 윈도우가 없어 redraw 의미가 없다. bool 반환값은 무의미.
            state.engine.refresh_busy_surfaces();
        }
    }
}
