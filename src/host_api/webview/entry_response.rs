//! Native response ownership. Remote source paths are provenance and are never opened here.
use std::sync::{Arc, Mutex, Weak, atomic::AtomicBool};
use tasty_remote::html_body::{EntryBodyStore, NativeBodyResponse, ReceivedEntry};

static STORE_POISONED: AtomicBool = AtomicBool::new(false);
#[derive(Clone)]
pub(crate) struct EntryResponseSource(Weak<Mutex<EntryBodyStore>>);
impl EntryResponseSource {
    pub(crate) fn new(store: &Arc<Mutex<EntryBodyStore>>) -> Self {
        Self(Arc::downgrade(store))
    }
    pub(crate) fn entry(&self, uri: &str) -> Option<Arc<ReceivedEntry>> {
        let store = self.0.upgrade()?;
        tasty_utils::poison::recover_mutex(store.lock(), "native entry body store", &STORE_POISONED)
            .entry_for_uri(uri)
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
