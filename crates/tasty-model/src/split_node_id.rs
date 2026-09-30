//! Process-local identity for a split node's compatibility metadata. This is
//! neither a surface ID nor a journal/replay identity and is never sent on wire.

use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SplitNodeId(u64);

impl SplitNodeId {
    pub fn allocate() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(
            NEXT.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
                .expect("split node identity space exhausted"),
        )
    }
}
