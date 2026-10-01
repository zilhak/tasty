//! Shared preset names for the picker, without exposing the disk writer or its lock.
use std::sync::{Arc, Mutex};
use tasty_presets::{PresetKind, PresetStore};

pub(crate) struct PresetCatalog(Arc<Mutex<PresetStore>>);
impl PresetCatalog {
    pub(crate) fn new(store: Arc<Mutex<PresetStore>>) -> Self {
        Self(store)
    }
    pub(crate) fn list(&self, kind: PresetKind) -> Vec<String> {
        crate::poison::recover_mutex(
            self.0.lock(),
            crate::core::PRESET_STORE_WHAT,
            &crate::core::PRESET_STORE_POISONED,
        )
        .list(kind)
    }
}
