//! Engine delivery queues. View selection only produces values; it owns no lifecycle executor.
use crate::core::host_event::{PendingHostEvent, PendingSurfaceClosed};
use crate::runtime::engine_access::EngineMut;
impl EngineMut<'_> {
    pub fn enqueue_surface_closed(
        &mut self,
        surface_id: u32,
        kind: Option<&'static str>,
        is_user_close: bool,
    ) {
        self.runtime
            .pending_lifecycle_events
            .push(PendingSurfaceClosed {
                surface_id,
                kind: kind.map(str::to_owned),
                is_user_close,
            });
    }

    #[cfg(any(feature = "gui", test))]
    pub fn take_pending_lifecycle_events(&mut self) -> Vec<PendingSurfaceClosed> {
        std::mem::take(&mut self.runtime.pending_lifecycle_events)
    }

    pub fn enqueue_host_event(&mut self, event: PendingHostEvent) {
        self.runtime.pending_host_events.push(event);
    }

    pub fn take_pending_host_events(&mut self) -> Vec<PendingHostEvent> {
        std::mem::take(&mut self.runtime.pending_host_events)
    }
}
