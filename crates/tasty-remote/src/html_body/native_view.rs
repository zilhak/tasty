//! Native view lifetime is distinct from immutable body lifetime and wire transfer identity.
use super::*;
use std::sync::Weak;
#[derive(Clone, Debug)]
pub struct NativeViewBinding {
    generation: TransferId,
    source_generation: u64,
    owner: Weak<()>,
}
impl PartialEq for NativeViewBinding {
    fn eq(&self, other: &Self) -> bool {
        self.generation == other.generation
            && self.source_generation == other.source_generation
            && self.owner.ptr_eq(&other.owner)
    }
}
impl Eq for NativeViewBinding {}
impl NativeViewBinding {
    pub fn is_current(&self, store: &EntryBodyStore) -> bool {
        store.expects(self.generation)
            && self.owner.ptr_eq(&Arc::downgrade(&store.view_owner))
            && store.current().is_some_and(|entry| {
                entry.response().generation() == self.generation
                    && entry.response().source_generation() == self.source_generation
            })
    }
}
impl EntryBodyStore {
    pub fn native_binding(&self) -> Option<NativeViewBinding> {
        let entry = self.current()?;
        Some(NativeViewBinding {
            generation: entry.response().generation(),
            source_generation: entry.response().source_generation(),
            owner: Arc::downgrade(&self.view_owner),
        })
    }
    /// Revoke callbacks from the native view while retaining the same completed document/cache.
    pub fn retire_native_view(&mut self) {
        self.view_owner = Arc::new(());
    }
}
