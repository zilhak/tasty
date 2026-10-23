//! Registration-bound reservation includes queued frames and the production writer's live frame.
use super::*;
use crate::stream::{FRAME_HEADER_LEN, MAX_FRAME_LEN};
use std::ops::Deref;

struct ByteLease {
    counter: Arc<AtomicUsize>,
    bytes: usize,
}
impl ByteLease {
    fn reserve(counter: &Arc<AtomicUsize>, bytes: usize, limit: usize) -> Option<Self> {
        counter
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(bytes).filter(|n| *n <= limit)
            })
            .ok()?;
        Some(Self {
            counter: counter.clone(),
            bytes,
        })
    }
    fn shrink(&mut self, bytes: usize) {
        self.counter.fetch_sub(self.bytes - bytes, Ordering::AcqRel);
        self.bytes = bytes;
    }
}
impl Drop for ByteLease {
    fn drop(&mut self) {
        self.counter.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}

/// Drop the unique owner to invalidate queued frames for a retired document or grant.
/// A new document needs a newly allocated fence; numeric IDs cannot revive this one.
#[derive(Default)]
pub struct PublicationFence(Arc<()>);

/// Non-cloneable full-frame reservation captured before polling an executor event slot.
/// Dropping an unused reservation releases its original registration's reserved bytes.
pub struct FrameReservation {
    client: StreamClientId,
    binding: Weak<()>,
    owner: Option<Weak<()>>,
    lease: ByteLease,
}
impl FrameReservation {
    pub fn for_owner(mut self, owner: &PublicationFence) -> Self {
        self.owner = Some(Arc::downgrade(&owner.0));
        self
    }
    pub fn capacity(&self) -> usize {
        self.lease.bytes
    }
    pub(super) fn attach(
        mut self,
        client: StreamClientId,
        sink: &StreamSink,
        frame: StreamFrame,
    ) -> Result<HeldFrame, HeldFrame> {
        let bytes = frame.payload.len().checked_add(FRAME_HEADER_LEN);
        let valid = self.client == client
            && self.binding.ptr_eq(&Arc::downgrade(&sink.binding))
            && Arc::ptr_eq(&self.lease.counter, &sink.bytes)
            && self
                .owner
                .as_ref()
                .is_none_or(|owner| owner.upgrade().is_some())
            && frame.payload.len() <= MAX_FRAME_LEN as usize
            && bytes.is_some_and(|n| n <= self.lease.bytes);
        if valid {
            self.lease.shrink(bytes.expect("validated frame length"));
        }
        let held = HeldFrame {
            frame,
            binding: self.binding,
            owner: self.owner,
            _lease: self.lease,
        };
        if valid { Ok(held) } else { Err(held) }
    }
}

/// The production socket writer retains this lease through the actual write, including errors.
/// Payload access borrows the frame; releasing it returns the connection's charged bytes.
pub struct HeldFrame {
    frame: StreamFrame,
    binding: Weak<()>,
    owner: Option<Weak<()>>,
    _lease: ByteLease,
}
impl Deref for HeldFrame {
    type Target = StreamFrame;
    fn deref(&self) -> &StreamFrame {
        &self.frame
    }
}
impl HeldFrame {
    /// Recheck immediately before a socket write. Owner retirement never authorizes a
    /// replacement document to publish a queued frame captured by the previous owner.
    pub fn is_current(&self) -> bool {
        self.binding.upgrade().is_some()
            && self
                .owner
                .as_ref()
                .is_none_or(|owner| owner.upgrade().is_some())
    }
    pub(super) fn charge(frame: StreamFrame, sink: &StreamSink) -> Result<Self, Self> {
        let counter = &sink.bytes;
        let binding = Arc::downgrade(&sink.binding);
        // Existing traffic keeps its frame-count admission policy. It still counts against
        // HTTP reservations, so a congested unrelated stream cannot add HTTP payloads.
        let bytes = frame.payload.len().saturating_add(FRAME_HEADER_LEN);
        let lease = ByteLease::reserve(counter, bytes, usize::MAX);
        match lease {
            Some(lease) => Ok(Self {
                frame,
                binding,
                owner: None,
                _lease: lease,
            }),
            None => Err(Self {
                frame,
                binding,
                owner: None,
                _lease: ByteLease {
                    counter: counter.clone(),
                    bytes: 0,
                },
            }),
        }
    }
    pub(super) fn into_frame(self) -> StreamFrame {
        self.frame
    }
}
impl StreamHub {
    /// Reserve before consuming an executor event. The limit is the host's injected connection
    /// budget; other queued/live frames and outstanding reservations are included atomically.
    pub fn reserve_frame(
        &self,
        client: StreamClientId,
        binding: &Weak<()>,
        byte_limit: usize,
    ) -> Option<FrameReservation> {
        let sinks =
            tasty_utils::poison::recover_mutex(self.sinks.lock(), SINKS_WHAT, &SINKS_POISONED);
        let sink = sinks.get(&client)?;
        if !binding.ptr_eq(&Arc::downgrade(&sink.binding)) {
            return None;
        }
        let lease = ByteLease::reserve(
            &sink.bytes,
            MAX_FRAME_LEN as usize + FRAME_HEADER_LEN,
            byte_limit,
        )?;
        Some(FrameReservation {
            client,
            binding: binding.clone(),
            owner: None,
            lease,
        })
    }
    /// Consume the captured registration exactly once. Excess reservation is reconciled to
    /// actual payload plus framing; failed/stale sends release it without another allocation.
    pub fn push_reserved(&self, reservation: FrameReservation, frame: StreamFrame) -> PushResult {
        let client = reservation.client;
        let binding = reservation.binding.clone();
        self.push_to(client, Some(&binding), frame, Some(reservation))
    }
}
impl SinkReceiver {
    /// Unlike the legacy raw-frame convenience methods, this keeps the byte charge while
    /// the caller writes to the socket. Production writers must use this method.
    pub fn recv_held_timeout(
        &self,
        timeout: std::time::Duration,
    ) -> Result<HeldFrame, mpsc::RecvTimeoutError> {
        self.rx.recv_timeout(timeout).map(|frame| self.took(frame))
    }
}

#[cfg(test)]
mod tests;
