use super::*;
use gtk::prelude::*;
use std::collections::BTreeMap;
pub(super) struct State {
    pub webview: WebView,
    pub context: WebContext,
    pub profile: NativeProfileLease,
    pub config: RefCell<ContextConfig>,
    pub epoch: Cell<u64>,
    pub blocked: Cell<bool>,
    pub endpoint_trust: Cell<bool>,
    handlers: RefCell<Vec<glib::SignalHandlerId>>,
    owner: Weak<store::State>,
    pub closed: Cell<bool>,
    pub pending: RefCell<BTreeMap<u64, tls::Probe>>,
    pub resuming: RefCell<BTreeMap<u64, tls::Resume>>,
    pub trusted: RefCell<BTreeMap<authority::Authority, [u8; 32]>>,
    pub history: Rc<RefCell<allowance::History>>,
    #[cfg(test)]
    pub held_methods: RefCell<Vec<String>>,
    #[cfg(test)]
    pub observations: RefCell<Vec<u32>>,
    pub sequence: Cell<u64>,
    pub failure: Cell<Option<ContextError>>,
}
impl State {
    pub fn new(
        context: WebContext,
        config: ContextConfig,
        history: Rc<RefCell<allowance::History>>,
        profile: NativeProfileLease,
        owner: Weak<store::State>,
    ) -> Rc<Self> {
        let webview = WebView::with_context(&context);
        store::retain_profile(&webview, &profile);
        let state = Rc::new(Self {
            webview,
            context,
            profile,
            config: RefCell::new(config),
            epoch: Cell::new(0),
            blocked: Cell::new(false),
            endpoint_trust: Cell::new(false),
            handlers: RefCell::new(Vec::new()),
            owner,
            history,
            closed: Cell::new(false),
            pending: RefCell::new(BTreeMap::new()),
            resuming: RefCell::new(BTreeMap::new()),
            trusted: RefCell::new(BTreeMap::new()),
            #[cfg(test)]
            observations: RefCell::new(Vec::new()),
            #[cfg(test)]
            held_methods: RefCell::new(Vec::new()),
            sequence: Cell::new(0),
            failure: Cell::new(None),
        });
        *state.handlers.borrow_mut() = tls::install(&state);
        state
    }
    pub fn live(&self) -> bool {
        !self.closed.get() && !self.blocked.get() && self.current()
    }
    pub fn current(&self) -> bool {
        let current = self.config.borrow().current.clone();
        current()
    }
    pub fn pin(&self, authority: &authority::Authority) -> Option<[u8; 32]> {
        if !self.current() {
            return None;
        }
        let expected = self.config.borrow().expected_leaf.clone();
        expected(&authority.host, authority.port)
    }
    pub fn revoke(self: &Rc<Self>) -> Result<(), ContextError> {
        let epoch = self
            .epoch
            .get()
            .checked_add(1)
            .ok_or(ContextError::Capacity)?;
        self.blocked.set(true);
        self.epoch.set(epoch);
        self.clear_callbacks();
        self.trusted.borrow_mut().clear();
        self.failure.set(None);
        *self.handlers.borrow_mut() = tls::install(self);
        Ok(())
    }
    fn clear_callbacks(&self) {
        for handler in self.handlers.borrow_mut().drain(..) {
            self.webview.disconnect(handler);
        }
        let probes = std::mem::take(&mut *self.pending.borrow_mut());
        drop(probes);
        let resuming = std::mem::take(&mut *self.resuming.borrow_mut());
        drop(resuming);
    }
    pub fn close(&self) {
        if self.closed.replace(true) {
            return;
        }
        self.blocked.set(true);
        self.clear_callbacks();
        if let Some(owner) = self.owner.upgrade() {
            owner.cancel_view(self);
        }
        self.webview.stop_loading();
        // SAFETY: this dedicated GTK widget is destroyed once on its owning native thread.
        unsafe {
            self.webview.destroy();
        }
    }
}
