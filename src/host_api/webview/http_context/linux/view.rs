use super::*;
use gtk::prelude::*;
use std::collections::BTreeMap;
pub(super) struct State {
    pub webview: WebView,
    pub context: WebContext,
    pub profile: NativeProfileLease,
    pub config: ContextConfig,
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
    ) -> Rc<Self> {
        let webview = WebView::with_context(&context);
        store::retain_profile(&webview, &profile);
        let state = Rc::new(Self {
            webview,
            context,
            profile,
            config,
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
        tls::install(&state);
        state
    }
    pub fn live(&self) -> bool {
        !self.closed.get() && (self.config.current)()
    }
    pub fn close(&self) {
        if self.closed.replace(true) {
            return;
        }
        let probes = std::mem::take(&mut *self.pending.borrow_mut());
        drop(probes);
        let resuming = std::mem::take(&mut *self.resuming.borrow_mut());
        drop(resuming);
        self.webview.stop_loading();
        // SAFETY: this dedicated GTK widget is destroyed once on its owning native thread.
        unsafe {
            self.webview.destroy();
        }
    }
}
