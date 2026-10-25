//! Private per-connection WK data stores. This component does not activate App capabilities.
//! Native controls observe loopback proxy bypass; do not wire this as an App HTTP adapter.
#[path = "macos/delegate.rs"]
mod delegate;
#[path = "macos/proxy.rs"]
mod proxy;
#[cfg(test)]
#[path = "macos/tests.rs"]
mod tests;
#[path = "macos/trust.rs"]
mod trust;

use block2::RcBlock;
pub use delegate::NavigationDelegate;
use objc2::{MainThreadMarker, rc::Retained};
use objc2_foundation::{NSDate, NSSet};
use objc2_web_kit::{
    WKWebView, WKWebViewConfiguration, WKWebsiteDataStore, WKWebsiteDataTypeDiskCache,
    WKWebsiteDataTypeFetchCache, WKWebsiteDataTypeMemoryCache,
    WKWebsiteDataTypeServiceWorkerRegistrations,
};
pub use proxy::ProxyRoute;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
pub use trust::{Authority, NativeObservation};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextError {
    Unsupported,
    InvalidProxy,
    Native,
    Retired,
    Busy,
    Capacity,
}
pub type Current = Rc<dyn Fn() -> bool>;
struct State {
    closed: Cell<bool>,
    clearing: Cell<bool>,
    epoch: Cell<u64>,
    current: Current,
    grants: RefCell<Vec<NativeObservation>>,
}
impl State {
    fn current(&self) -> bool {
        !self.closed.get() && (self.current)()
    }
}
/// One store for one connection, retained by every view and asynchronous clear callback.
#[derive(Clone)]
pub struct DataStore {
    store: Retained<WKWebsiteDataStore>,
    state: Rc<State>,
}
impl DataStore {
    pub fn new(
        route: ProxyRoute,
        current: Current,
        mtm: MainThreadMarker,
    ) -> Result<Self, ContextError> {
        // SAFETY: main-thread new private store, never the shared default store.
        let store = unsafe { WKWebsiteDataStore::nonPersistentDataStore(mtm) };
        let network = proxy::Network::load(&store)?;
        network.configure(&store, &route)?;
        Ok(Self {
            store,
            state: Rc::new(State {
                closed: Cell::new(false),
                clearing: Cell::new(false),
                epoch: Cell::new(0),
                current,
                grants: RefCell::default(),
            }),
        })
    }
    pub fn configure(&self, configuration: &WKWebViewConfiguration) -> Result<(), ContextError> {
        if !self.state.current() {
            return Err(ContextError::Retired);
        }
        if self.state.clearing.get() {
            return Err(ContextError::Busy);
        }
        // SAFETY: before creating the target view; configuration retains this exact store.
        unsafe {
            configuration.setWebsiteDataStore(&self.store);
        }
        Ok(())
    }
    pub fn native(&self) -> &WKWebsiteDataStore {
        &self.store
    }
    pub fn is_current(&self) -> bool {
        self.state.current() && !self.state.clearing.get()
    }
    pub fn close(&self) {
        self.state.closed.set(true);
        self.state.grants.borrow_mut().clear();
    }
    /// Retain the same view/request/cookies. Completion is a barrier; no reload or GET is issued.
    pub fn clear_selected(
        &self,
        done: impl FnOnce(Result<(), ContextError>) + 'static,
    ) -> Result<(), ContextError> {
        if !self.state.current() {
            return Err(ContextError::Retired);
        }
        if self.state.clearing.replace(true) {
            return Err(ContextError::Busy);
        }
        let Some(epoch) = self.state.epoch.get().checked_add(1) else {
            self.close();
            return Err(ContextError::Capacity);
        };
        self.state.epoch.set(epoch);
        self.state.grants.borrow_mut().clear();
        // SAFETY: these symbols predate macOS 11; selected set omits cookies and persistent storage.
        let types = unsafe {
            NSSet::from_slice(&[
                WKWebsiteDataTypeDiskCache,
                WKWebsiteDataTypeMemoryCache,
                WKWebsiteDataTypeFetchCache,
                WKWebsiteDataTypeServiceWorkerRegistrations,
            ])
        };
        let retained = self.clone();
        let completion = RefCell::new(Some(done));
        let callback = RcBlock::new(move || {
            retained.state.clearing.set(false);
            if let Some(done) = completion.borrow_mut().take() {
                done(
                    if retained.state.current() && retained.state.epoch.get() == epoch {
                        Ok(())
                    } else {
                        Err(ContextError::Retired)
                    },
                );
            }
        });
        // SAFETY: retained store and copied block survive native asynchronous deletion.
        unsafe {
            self.store
                .removeDataOfTypes_modifiedSince_completionHandler(
                    &types,
                    &NSDate::distantPast(),
                    &callback,
                );
        }
        Ok(())
    }
    /// Called only from a successful native-only probe capability. Finite per-document history.
    pub fn reserve(&self, observation: NativeObservation) -> Result<(), ContextError> {
        if !self.is_current() {
            return Err(ContextError::Retired);
        }
        let mut grants = self.state.grants.borrow_mut();
        if grants.contains(&observation) {
            return Ok(());
        }
        if grants
            .iter()
            .any(|grant| grant.authority() == observation.authority())
        {
            return Err(ContextError::Native);
        }
        if grants.len() >= 8 {
            return Err(ContextError::Capacity);
        }
        grants.push(observation);
        Ok(())
    }
    /// Compose into the target WKNavigationDelegate; never replace its navigation/body policy.
    pub fn accepts(&self, observation: &NativeObservation) -> bool {
        self.is_current() && self.state.grants.borrow().contains(observation)
    }
    pub fn delegate(&self, mtm: MainThreadMarker) -> Retained<NavigationDelegate> {
        NavigationDelegate::new(self.clone(), None, mtm)
    }
    /// Stop before retiring the owner; delayed callbacks see closed and cannot continue requests.
    pub fn close_view(&self, view: &WKWebView) {
        self.close();
        // SAFETY: main-thread owned view; neither focus nor window visibility changes.
        unsafe {
            view.stopLoading();
        }
    }
}
