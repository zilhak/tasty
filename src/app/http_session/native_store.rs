//! Physical native allocation generation shared by current and retained logical documents.
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

pub(crate) struct NativeStore {
    generation: AtomicU64,
    live: AtomicBool,
}
impl Default for NativeStore {
    fn default() -> Self {
        Self {
            generation: AtomicU64::new(1),
            live: AtomicBool::new(false),
        }
    }
}
impl NativeStore {
    pub(crate) fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }
    pub(crate) fn is_current(&self, generation: u64) -> bool {
        generation != u64::MAX && self.generation() == generation
    }
    pub(crate) fn is_live(&self) -> bool {
        self.live.load(Ordering::Acquire)
    }
    pub(crate) fn publish(&self) {
        self.live.store(true, Ordering::Release);
    }
    pub(crate) fn invalidate(&self, generation: u64) {
        // A late old-view drop must not revoke a newer allocation on the same surface.
        if generation != u64::MAX
            && self
                .generation
                .compare_exchange(
                    generation,
                    generation + 1,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_ok()
        {
            self.live.store(false, Ordering::Release);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_invalidation_retires_old_and_prepared_documents() {
        let store = NativeStore::default();
        let old = store.generation();
        let prepared = store.generation();
        store.publish();
        store.invalidate(prepared);
        assert!(!store.is_current(old));
        assert!(!store.is_current(prepared));
        assert!(!store.is_live());
    }
    #[test]
    fn late_old_drop_preserves_fresh_allocation_and_other_surface() {
        let store = NativeStore::default();
        let other = NativeStore::default();
        let old = store.generation();
        store.invalidate(old);
        let fresh = store.generation();
        store.publish();
        store.invalidate(old);
        assert!(store.is_current(fresh));
        assert!(store.is_live());
        assert!(other.is_current(old));
    }
    #[test]
    fn exhausted_generation_never_becomes_current() {
        let store = NativeStore {
            generation: AtomicU64::new(u64::MAX - 1),
            live: AtomicBool::new(true),
        };
        store.invalidate(u64::MAX - 1);
        store.invalidate(u64::MAX);
        assert_eq!(store.generation(), u64::MAX);
        assert!(!store.is_current(u64::MAX));
        assert!(!store.is_live());
    }
}
