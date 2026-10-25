//! One-shot ownership of an existing native request; no reconstructed uploads.
use super::*;

use webkit2gtk::NavigationPolicyDecision;

enum Action {
    Navigation(NavigationPolicyDecision),
    Reload,
}
pub(crate) struct NativeContinuation {
    view: Weak<view::State>,
    epoch: u64,
    pub(super) target: Option<String>,
    action: Option<Action>,
}
impl NativeContinuation {
    pub(super) fn reload(view: &Rc<view::State>) -> Self {
        Self {
            view: Rc::downgrade(view),
            epoch: view.epoch.get(),
            target: view.webview.uri().map(|u| u.to_string()),
            action: Some(Action::Reload),
        }
    }
    pub(super) fn navigation(view: &Rc<view::State>, decision: &NavigationPolicyDecision) -> Self {
        Self {
            view: Rc::downgrade(view),
            epoch: view.epoch.get(),
            target: decision
                .navigation_action()
                .and_then(|a| a.request())
                .and_then(|r| r.uri())
                .map(|u| u.to_string()),
            action: Some(Action::Navigation(decision.clone())),
        }
    }
    pub(super) fn is_reload(&self) -> bool {
        matches!(self.action, Some(Action::Reload))
    }
    pub(super) fn belongs_to(&self, view: &Rc<view::State>) -> bool {
        self.epoch == view.epoch.get() && self.view.upgrade().is_some_and(|v| Rc::ptr_eq(&v, view))
    }
    pub(super) fn still_matches(&self, webview: &WebView) -> bool {
        !self.is_reload() || webview.uri().as_deref() == self.target.as_deref()
    }
    pub(super) fn resume(mut self, webview: &WebView) {
        match self.action.take().expect("one-shot native action") {
            Action::Navigation(decision) => decision.use_(),
            Action::Reload => webview.reload_bypass_cache(),
        }
    }
}
impl Drop for NativeContinuation {
    fn drop(&mut self) {
        if let Some(Action::Navigation(decision)) = self.action.take() {
            decision.ignore();
        }
    }
}

pub(crate) struct RetiredView {
    pub(super) store: Option<Rc<store::State>>,
    pub(super) id: u64,
}
pub(crate) struct ReadyView {
    pub(super) store: Option<Rc<store::State>>,
    pub(super) id: u64,
}
pub(super) type ReadyDone = Box<dyn FnOnce(Result<ReadyView, ContextError>)>;
impl RetiredView {
    /// Caller prerequisite: revoke old body/endpoint ownership, complete actual
    /// socket joins and off-thread fresh authority-pin preparation. This method
    /// does not establish or fabricate any of those external facts.
    pub(crate) fn after_endpoint_join(mut self, config: ContextConfig, done: ReadyDone) {
        let store = self.store.take().expect("one-shot retirement");
        store.clear_retained(self.id, config, done);
    }
}
impl ReadyView {
    /// Explicitly release the captured native action once, after revalidation.
    pub(crate) fn resume(mut self) -> Result<(), ContextError> {
        let store = self.store.take().expect("one-shot continuation");
        store.resume_retained(self.id)
    }
}
impl Drop for RetiredView {
    fn drop(&mut self) {
        if let Some(store) = self.store.take() {
            store.abandon_retained(self.id);
        }
    }
}
impl Drop for ReadyView {
    fn drop(&mut self) {
        if let Some(store) = self.store.take() {
            store.abandon_retained(self.id);
        }
    }
}
