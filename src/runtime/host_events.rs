//! Engine delivery queues. View selection only produces values; it owns no lifecycle executor.
use crate::core::host_event::PendingHostEvent;
use crate::runtime::engine_access::EngineMut;
impl EngineMut<'_> {
    pub fn enqueue_host_event(&mut self, event: PendingHostEvent) {
        self.runtime.pending_host_events.push(event);
    }

    pub fn take_pending_host_events(&mut self) -> Vec<PendingHostEvent> {
        std::mem::take(&mut self.runtime.pending_host_events)
    }
}
