//! Process-local connection identity. Independent of VT content epochs and durable journal IDs.
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Identifies one physical PTY connection (or one detached terminal connection).
/// Adopt preserves it; replacing the connection allocates another value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResourceGeneration(u64);

impl ResourceGeneration {
    pub(crate) fn fresh() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(
            NEXT.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
                .expect("terminal resource generation space exhausted"),
        )
    }
}

/// Revokes callbacks and queued writes without retaining an OS handle or Terminal state.
#[derive(Clone)]
pub(crate) struct ConnectionLease {
    generation: ResourceGeneration,
    active: Arc<AtomicBool>,
}

impl ConnectionLease {
    pub(crate) fn new() -> Self {
        Self {
            generation: ResourceGeneration::fresh(),
            active: Arc::new(AtomicBool::new(true)),
        }
    }
    pub(crate) fn generation(&self) -> ResourceGeneration {
        self.generation
    }
    pub(crate) fn is_active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }
    pub(crate) fn revoke(&self) {
        self.active.store(false, Ordering::Release);
    }
}
