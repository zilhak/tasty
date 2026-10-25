//! One-shot ownership of an existing native request; no reconstructed uploads.
use super::*;

#[cfg(test)]
use webkit2gtk::NavigationPolicyDecision;

enum Action {
    #[cfg(test)]
    Navigation(NavigationPolicyDecision),
    Reload,
    InitialGet {
        uri: String,
        provisional: Option<String>,
    },
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
    // Caller attests the actual initial native request was an empty GET for this source,
    // with no committed response. A failed provisional load can leave WebKit URI unset or empty.
    pub(super) fn initial_get(view: &Rc<view::State>, url: &str) -> Result<Self, ContextError> {
        let target = url::Url::parse(url).map_err(|_| ContextError::Policy)?;
        let provisional = view.webview.uri().map(|uri| uri.to_string());
        if !view.live()
            || provisional.as_ref().is_some_and(|uri| !uri.is_empty())
            || !matches!(target.scheme(), "http" | "https")
            || target.host().is_none()
            || !target.username().is_empty()
            || target.password().is_some()
        {
            return Err(ContextError::Policy);
        }
        Ok(Self {
            view: Rc::downgrade(view),
            epoch: view.epoch.get(),
            target: Some(url.into()),
            action: Some(Action::InitialGet {
                uri: url.into(),
                provisional,
            }),
        })
    }
    #[cfg(test)]
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
        match &self.action {
            Some(Action::InitialGet { provisional, .. }) => {
                webview.uri().as_deref() == provisional.as_deref()
            }
            _ => !self.is_reload() || webview.uri().as_deref() == self.target.as_deref(),
        }
    }
    pub(super) fn resume(mut self, webview: &WebView) {
        match self.action.take().expect("one-shot native action") {
            #[cfg(test)]
            Action::Navigation(decision) => decision.use_(),
            Action::Reload => webview.reload_bypass_cache(),
            Action::InitialGet { uri, .. } => webview.load_uri(&uri),
        }
    }
}
#[cfg(test)]
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
