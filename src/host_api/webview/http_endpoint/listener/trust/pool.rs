//! A fixed private native manager/profile resource, shared across endpoint/document lifetimes.
//! Only transient probe views are per request; ticket equality fences old native callbacks.
use super::*;
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProbeId(u64);
static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
struct Active {
    id: ProbeId,
    request: TrustRequest,
    current: Box<dyn Fn() -> bool>,
}
pub(crate) struct ProbePool<T> {
    resource: T,
    active: Option<Active>,
}
impl<T> ProbePool<T> {
    pub(crate) fn new(resource: T) -> Self {
        Self {
            resource,
            active: None,
        }
    }
    pub(crate) fn resource(&self) -> &T {
        &self.resource
    }
    /// The process-local native owner polls eligible endpoint inboxes using the same resource.
    /// Capture the actual PreparedView/epoch/source predicate; it may be GTK-local and non-Send.
    pub(crate) fn poll(
        &mut self,
        inbox: &NativeTrust,
        current: impl Fn() -> bool + 'static,
    ) -> Option<ProbeId> {
        if let Some(active) = &self.active {
            if !(active.current)() {
                active.request.0.cancel();
            }
            return None;
        }
        if !current() {
            return None;
        }
        let request = inbox.poll()?;
        let next = NEXT_ID
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                value.checked_add(1)
            })
            .ok()?;
        if !current() {
            request.0.cancel();
            return None;
        }
        let id = ProbeId(next);
        self.active = Some(Active {
            id,
            request,
            current: Box::new(current),
        });
        Some(id)
    }
    /// Only this ticket's live native view can obtain its original opaque trust capability.
    pub(crate) fn request(&self, id: ProbeId) -> Option<&TrustRequest> {
        self.active
            .as_ref()
            .filter(|a| a.id == id && (a.current)() && a.request.is_current())
            .map(|a| &a.request)
    }
    pub(crate) fn ack(&self, id: ProbeId, observation: TrustObservation) -> bool {
        let Some(active) = self.active.as_ref().filter(|a| a.id == id) else {
            return false;
        };
        if !(active.current)() {
            active.request.0.cancel();
            return false;
        }
        active.request.ack(observation)
    }
    pub(crate) fn reject(&self, id: ProbeId) -> bool {
        self.active
            .as_ref()
            .filter(|a| a.id == id)
            .is_some_and(|a| a.request.reject())
    }
    /// Native explicitly confirms transient view/callback teardown. Socket retirement must also
    /// have finished: ACK alone cannot release this sole resource for another endpoint/probe.
    pub(crate) fn complete(&mut self, id: ProbeId) -> bool {
        let Some(active) = self.active.as_ref().filter(|a| a.id == id) else {
            return false;
        };
        if !(active.current)() {
            active.request.0.cancel();
        }
        let slot = &active.request.0;
        if !slot.stopped.load(Ordering::Acquire) || (slot.current() && slot.verdict().is_none()) {
            return false;
        }
        self.active = None;
        true
    }
    /// Cancellation retains resource and request until complete/native teardown or actual Drop.
    pub(crate) fn retire(&self) {
        if let Some(active) = &self.active {
            active.request.0.cancel();
        }
    }
}
impl<T> Drop for ProbePool<T> {
    fn drop(&mut self) {
        self.retire();
    }
}
