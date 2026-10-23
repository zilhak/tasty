//! Native response ownership. Remote source paths are provenance and are never opened here.
use std::sync::{
    Arc, Mutex, Weak,
    atomic::{AtomicBool, Ordering},
};
use tasty_remote::html_body::{
    EntryBodyStore, NativeBodyResponse, NativeViewBinding, ReceivedEntry,
};

static STORE_POISONED: AtomicBool = AtomicBool::new(false);
#[derive(Clone)]
pub(crate) struct EntryResponseSource {
    store: Weak<Mutex<EntryBodyStore>>,
    expected: Option<NativeViewBinding>,
    live: Arc<AtomicBool>,
}
impl EntryResponseSource {
    pub(crate) fn new(store: &Arc<Mutex<EntryBodyStore>>) -> Self {
        let expected = tasty_utils::poison::recover_mutex(
            store.lock(),
            "native entry body store",
            &STORE_POISONED,
        )
        .native_binding();
        Self {
            store: Arc::downgrade(store),
            expected,
            live: Arc::new(AtomicBool::new(true)),
        }
    }
    pub(crate) fn retire(&self) {
        self.live.store(false, Ordering::Release);
    }
    pub(crate) fn is_current(&self) -> bool {
        if !self.live.load(Ordering::Acquire) {
            return false;
        }
        let Some(store) = self.store.upgrade() else {
            return false;
        };
        let store = tasty_utils::poison::recover_mutex(
            store.lock(),
            "native entry body store",
            &STORE_POISONED,
        );
        self.expected
            .as_ref()
            .is_some_and(|binding| binding.is_current(&store))
    }
    pub(crate) fn entry(&self, uri: &str) -> Option<Arc<ReceivedEntry>> {
        if !self.live.load(Ordering::Acquire) {
            return None;
        }
        let store = self.store.upgrade()?;
        let store = tasty_utils::poison::recover_mutex(
            store.lock(),
            "native entry body store",
            &STORE_POISONED,
        );
        if !self.expected.as_ref()?.is_current(&store) {
            return None;
        }
        store.entry_for_uri(uri)
    }
    pub(crate) fn response(&self, uri: &str) -> Option<Arc<NativeBodyResponse>> {
        self.entry(uri).map(|entry| Arc::clone(entry.response()))
    }
}
/// Entry-only browser policy. C replaces only the unsupported resource boundary; remote network
/// access remains an isolated executor concern. An HTML meta policy cannot relax this header.
pub(crate) const ENTRY_CSP: &str = "default-src 'none'; script-src 'unsafe-inline' 'unsafe-eval'; style-src 'unsafe-inline'; img-src data:; font-src data:; media-src data:; connect-src 'none'; frame-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'; worker-src 'none'";

#[cfg(target_os = "linux")]
pub(super) mod linux;

#[cfg(target_os = "macos")]
pub(super) mod macos;

#[cfg(target_os = "windows")]
pub(super) mod windows;

#[cfg(test)]
mod tests;
