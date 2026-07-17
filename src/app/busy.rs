//! `AppEvent::BusyPoll` 처리 — 모든 surface 의 busy 상태 갱신.

use crate::app::App;

impl App {
    /// Refresh the busy-surface cache for every live AppState. Triggered ~1s
    /// from the background ticker via `AppEvent::BusyPoll`. Marks any window
    /// whose set actually changed as dirty so the indicators redraw.
    ///
    /// Also forwards busy transitions to any attach client occupying one of
    /// this instance's surfaces (`StreamControl::Activity`) — the same 1Hz tick
    /// that refreshes the local busy cache doubles as the cadence for that
    /// push, so a remote mirror's status dot never lags local by more than one
    /// tick. `stream_hub` is cloned once up front (cheap — internal `Arc`) so
    /// the per-engine forward calls don't need to borrow all of `self` while
    /// `self.view.views.values_mut()` already holds a mutable borrow.
    pub(crate) fn poll_busy_states(&mut self) {
        let hub = self.stream_hub.clone();
        for w in self.view.views.values_mut() {
            let changed = match w.as_main_mut() {
                Some(main) => {
                    let changed = crate::core::Core::update_busy_surfaces(&mut main.core_state);
                    main.core_state.forward_busy_activity(&hub);
                    changed
                }
                None => false,
            };
            if changed {
                w.mark_dirty();
            }
        }
        for (_, engine) in self.parked_states.iter_mut() {
            // parked 는 window 가 없어 redraw 의미가 없다. 반환값은 무시.
            crate::core::Core::update_busy_surfaces(engine);
            engine.forward_busy_activity(&hub);
        }
    }
}
