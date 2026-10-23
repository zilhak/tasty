//! Native response ownership. Remote source paths are provenance and are never opened here.
use std::sync::{
    Arc, Mutex, Weak,
    atomic::{AtomicBool, Ordering},
};
use tasty_remote::html_body::{
    EntryBodyStore, NativeBodyResponse, NativeViewBinding, ReceivedEntry,
};

use tasty_remote::html_body::ResponseWait;

static STORE_POISONED: AtomicBool = AtomicBool::new(false);
#[derive(Clone)]
pub(crate) struct EntryResponseSource {
    store: Weak<Mutex<EntryBodyStore>>,
    expected: Option<NativeViewBinding>,
    live: Arc<AtomicBool>,
    notify: Option<Arc<dyn Fn() + Send + Sync>>,
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
            notify: None,
        }
    }
    pub(crate) fn csp(&self) -> &'static str {
        if self.resources_enabled() {
            FILE_CSP
        } else {
            ENTRY_CSP
        }
    }
    #[cfg(target_os = "linux")]
    pub(crate) fn lost_native_view(&self) {
        self.with_current(|s| s.retire_native_view())
            .unwrap_or_default();
        self.retire();
    }
    pub(crate) fn with_notify(mut self, notify: Arc<dyn Fn() + Send + Sync>) -> Self {
        self.notify = Some(notify);
        self
    }
    fn wake(&self) {
        if let Some(notify) = &self.notify {
            notify();
        }
    }
    pub(crate) fn retire(&self) {
        self.live.store(false, Ordering::Release);
        self.wake();
    }
    fn with_current<T>(&self, f: impl FnOnce(&mut EntryBodyStore) -> T) -> Option<T> {
        if !self.live.load(Ordering::Acquire) {
            return None;
        }
        let store = self.store.upgrade()?;
        let mut store = tasty_utils::poison::recover_mutex(
            store.lock(),
            "native entry body store",
            &STORE_POISONED,
        );
        if !self.expected.as_ref()?.is_current(&store) {
            return None;
        }
        Some(f(&mut store))
    }
    pub(crate) fn is_current(&self) -> bool {
        self.with_current(|_| ()).is_some()
    }
    pub(crate) fn entry(&self, uri: &str) -> Option<Arc<ReceivedEntry>> {
        self.with_current(|s| s.entry_for_uri(uri)).flatten()
    }
    pub(crate) fn response(&self, uri: &str) -> Option<Arc<NativeBodyResponse>> {
        self.with_current(|s| s.response_for_uri(uri)).flatten()
    }
    pub(crate) fn allows_uri(&self, uri: &str) -> bool {
        self.with_current(|s| s.response_for_uri(uri).is_some() || s.supports_resource_uri(uri))
            .unwrap_or(false)
    }
    pub(crate) fn resources_enabled(&self) -> bool {
        self.with_current(|s| s.resources_enabled())
            .unwrap_or(false)
    }
    pub(crate) fn pending_resources(&self) -> bool {
        self.with_current(|s| s.resources_mut().is_some_and(|r| r.has_pending()))
            .unwrap_or(false)
    }
    pub(crate) fn request(&self, uri: &str) -> Option<ResponseWait> {
        let wait = self.with_current(|s| s.request_resource(uri))?;
        self.wake();
        Some(wait)
    }
    /// Called only with a native main-resource identity, never inferred from the URI.
    pub(crate) fn main_resource(&self, uri: &str) -> bool {
        if uri == "about:blank" {
            return false;
        }
        let stop = self
            .with_current(|store| {
                if store.entry_for_uri(uri).is_some() {
                    return false;
                }
                if !store.navigate(uri) {
                    store.retire_native_view();
                }
                true
            })
            .unwrap_or(true);
        if stop {
            self.retire();
        }
        stop
    }
}
#[cfg(any(target_os = "macos", target_os = "windows", test))]
impl EntryResponseSource {
    /// Poll only on the native apartment/main thread. Completion wakes the App without
    /// transferring native task/COM objects to the producer thread.
    pub(super) fn poll_response(
        &self,
        wait: &mut ResponseWait,
    ) -> std::task::Poll<Result<Arc<NativeBodyResponse>, tasty_remote::html_body::ResourceFailure>>
    {
        use std::{future::Future, pin::Pin, task::Context};
        if !self.is_current() {
            return std::task::Poll::Ready(Err(
                tasty_remote::html_body::ResourceFailure::Cancelled,
            ));
        }
        let waker = std::task::Waker::from(Arc::new(self.clone()));
        Pin::new(wait).poll(&mut Context::from_waker(&waker))
    }
}
#[cfg(any(target_os = "macos", target_os = "windows", test))]
impl std::task::Wake for EntryResponseSource {
    fn wake(self: Arc<Self>) {
        EntryResponseSource::wake(&self);
    }
}
/// Entry-only browser policy. C replaces only the unsupported resource boundary; remote network
/// access remains an isolated executor concern. An HTML meta policy cannot relax this header.
pub(crate) const ENTRY_CSP: &str = "default-src 'none'; script-src 'unsafe-inline' 'unsafe-eval'; style-src 'unsafe-inline'; img-src data:; font-src data:; media-src data:; connect-src 'none'; frame-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'; worker-src 'none'";

/// Same-root file resources still have no ambient HTTP/file lookup and cannot start workers.
pub(crate) const FILE_CSP: &str = "default-src 'none'; script-src 'self' 'unsafe-inline' 'unsafe-eval'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self' data:; media-src 'self' data:; connect-src 'self'; frame-src 'self'; object-src 'none'; base-uri 'self'; form-action 'none'; worker-src 'none'";

#[cfg(target_os = "linux")]
pub(super) mod linux;

#[cfg(target_os = "linux")]
pub(crate) fn callbacks_pending() -> bool {
    linux::callbacks_pending()
}

#[cfg(target_os = "macos")]
pub(super) mod macos;

#[cfg(target_os = "windows")]
pub(super) mod windows;

#[cfg(test)]
mod tests;
