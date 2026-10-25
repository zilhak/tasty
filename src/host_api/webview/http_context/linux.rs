//! Dedicated mirror data store and document-local TLS decisions on the GTK thread.
#[path = "linux/allowance.rs"]
mod allowance;
#[path = "linux/authority.rs"]
mod authority;
#[path = "linux/store.rs"]
mod store;
#[path = "linux/tls.rs"]
mod tls;
#[path = "linux/view.rs"]
mod view;

#[path = "linux/continuation.rs"]
mod continuation;
#[path = "linux/replacement.rs"]
mod replacement;
pub(crate) use continuation::{NativeContinuation, ReadyView, RetiredView};

use super::profile::{NativeProfileLease, PreparedProfile};
use gtk::{gio, glib};
use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
    sync::Arc,
};
use webkit2gtk::{
    NavigationPolicyDecisionExt, PolicyDecisionExt, SettingsExt as WebSettingsExt, URIRequestExt,
    WebContextExt, WebResourceExt, WebViewExt, WebsiteDataManagerExt, WebsiteDataManagerExtManual,
};
use webkit2gtk::{WebContext, WebView, WebsiteDataManager};

#[path = "linux/native_probe.rs"]
mod native_probe;
#[path = "linux/native_trust.rs"]
mod native_trust;
pub(crate) use native_probe::{NativeProbeEvidence, NativeProbeStore};
pub(crate) use native_trust::NativeTrustTarget;

type ExpectedLeaf = Arc<dyn Fn(&str, u16) -> Option<[u8; 32]> + Send + Sync>;

#[derive(Clone)]
pub(crate) struct ContextConfig {
    pub proxy_uri: String,
    pub current: Arc<dyn Fn() -> bool + Send + Sync>,
    pub expected_leaf: ExpectedLeaf,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContextError {
    Retired,
    Busy,
    Cache,
    Tls,
    Policy,
    Capacity,
    Storage,
}
type Done = Box<dyn FnOnce(Result<PreparedView, ContextError>)>;
pub(crate) struct DataStore(Rc<store::State>);
pub(crate) struct PreparedView(Rc<view::State>);

impl DataStore {
    pub(crate) fn new(profile: PreparedProfile) -> Result<Self, ContextError> {
        store::State::new(profile).map(|state| Self(Rc::new(state)))
    }
    pub(crate) fn prepare(&self, config: ContextConfig, done: Done) {
        self.0.prepare(config, done);
    }
    pub(crate) fn retire_document(
        &self,
        view: &PreparedView,
        action: NativeContinuation,
    ) -> Result<RetiredView, ContextError> {
        self.0.retire_document(&view.0, action)
    }
    pub(crate) fn close(&self) {
        self.0.close();
    }
}
impl Drop for DataStore {
    fn drop(&mut self) {
        self.close();
    }
}
impl PreparedView {
    pub(crate) fn initial_get_continuation(
        &self,
        url: &str,
    ) -> Result<NativeContinuation, ContextError> {
        NativeContinuation::initial_get(&self.0, url)
    }
    pub(crate) fn reload_continuation(&self) -> NativeContinuation {
        NativeContinuation::reload(&self.0)
    }
    /// The caller must supply the navigation decision delivered for this view.
    #[cfg(test)]
    pub(crate) fn retain_navigation(
        &self,
        decision: &webkit2gtk::NavigationPolicyDecision,
    ) -> NativeContinuation {
        NativeContinuation::navigation(&self.0, decision)
    }
    pub(crate) fn webview(&self) -> &WebView {
        &self.0.webview
    }
    pub(crate) fn close(&self) {
        self.0.close();
    }
}
impl Drop for PreparedView {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
#[path = "linux/tests.rs"]
mod tests;
