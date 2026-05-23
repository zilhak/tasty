//! `AppEvent::BusyPoll` 처리 — 모든 surface 의 busy 상태 갱신.

use crate::app::App;

impl App {
    /// Refresh the busy-surface cache. Triggered ~1s from the background ticker
    /// via `AppEvent::BusyPoll`. Marks all windows dirty so the indicators redraw
    /// when the set actually changed.
    pub(crate) fn poll_busy_states(&mut self) {
        let changed = self.engine_state_mut().refresh_busy_surfaces();
        if changed {
            for w in self.windows.values_mut() {
                w.mark_dirty();
            }
        }
    }
}
