use super::*;
use gtk::gio::prelude::*;
use webkit2gtk::{NetworkProxyMode, NetworkProxySettings, TLSErrorsPolicy, WebsiteDataTypes};
struct Pending {
    config: ContextConfig,
    done: Done,
    cancel: gio::Cancellable,
}
pub(super) struct State {
    pub manager: WebsiteDataManager,
    profile: NativeProfileLease,
    pub history: Rc<RefCell<allowance::History>>,
    closed: Cell<bool>,
    pending: RefCell<Option<Pending>>,
    active: RefCell<Weak<view::State>>,
}
impl State {
    pub fn new(profile: PreparedProfile) -> Result<Self, ContextError> {
        let data = profile.data_dir().to_str().ok_or(ContextError::Storage)?;
        let cache = profile.cache_dir().to_str().ok_or(ContextError::Storage)?;
        let manager = WebsiteDataManager::builder()
            .base_data_directory(data)
            .base_cache_directory(cache)
            .build();
        let profile = profile.into_native();
        retain_profile(&manager, &profile);
        if manager.is_ephemeral() {
            return Err(ContextError::Storage);
        }
        manager.set_persistent_credential_storage_enabled(false);
        Ok(Self {
            manager,
            profile,
            history: Rc::new(RefCell::new(allowance::History::default())),
            closed: Cell::new(false),
            pending: RefCell::new(None),
            active: RefCell::new(Weak::new()),
        })
    }
    pub fn prepare(self: &Rc<Self>, config: ContextConfig, done: Done) {
        if let Some(error) = self.admission(&config) {
            done(Err(error));
            return;
        }
        let cancel = gio::Cancellable::new();
        *self.pending.borrow_mut() = Some(Pending {
            config,
            done,
            cancel: cancel.clone(),
        });
        let weak = glib::thread_guard::ThreadGuard::new(Rc::downgrade(self));
        let profile = self.profile.clone();
        self.manager.clear(
            clear_types(),
            glib::TimeSpan::from_microseconds(0),
            Some(&cancel),
            move |result| {
                let _retained_profile = profile;
                if let Some(state) = weak.into_inner().upgrade() {
                    state.finished(result.is_ok());
                }
            },
        );
    }
    fn admission(&self, config: &ContextConfig) -> Option<ContextError> {
        if self.closed.get() || !(config.current)() {
            return Some(ContextError::Retired);
        }
        if self.pending.borrow().is_some()
            || self
                .active
                .borrow()
                .upgrade()
                .is_some_and(|v| !v.closed.get())
        {
            return Some(ContextError::Busy);
        }
        if self.history.borrow().full() {
            return Some(ContextError::Capacity);
        }
        if !authority::proxy(&config.proxy_uri) {
            return Some(ContextError::Policy);
        }
        None
    }
    pub(super) fn finished(&self, success: bool) {
        let Some(pending) = self.pending.borrow_mut().take() else {
            return;
        };
        let result = if self.closed.get() || !(pending.config.current)() {
            Err(ContextError::Retired)
        } else if !success {
            Err(ContextError::Cache)
        } else {
            self.create(pending.config)
        };
        (pending.done)(result);
    }
    fn create(&self, config: ContextConfig) -> Result<PreparedView, ContextError> {
        if !(config.current)() {
            return Err(ContextError::Retired);
        }
        self.manager.set_tls_errors_policy(TLSErrorsPolicy::Fail);
        let mut proxy = NetworkProxySettings::new(Some(&config.proxy_uri), &[]);
        self.manager
            .set_network_proxy_settings(NetworkProxyMode::Custom, Some(&mut proxy));
        let context = WebContext::with_website_data_manager(&self.manager);
        retain_profile(&context, &self.profile);
        let prepared =
            view::State::new(context, config, self.history.clone(), self.profile.clone());
        if !(prepared.config.current)() {
            prepared.close();
            return Err(ContextError::Retired);
        }
        *self.active.borrow_mut() = Rc::downgrade(&prepared);
        Ok(PreparedView(prepared))
    }
    pub fn close(&self) {
        if self.closed.replace(true) {
            return;
        }
        let pending = self.pending.borrow_mut().take();
        if let Some(pending) = pending {
            pending.cancel.cancel();
            (pending.done)(Err(ContextError::Retired));
        }
        if let Some(active) = self.active.borrow().upgrade() {
            active.close();
        }
    }
}
pub(super) fn clear_types() -> WebsiteDataTypes {
    WebsiteDataTypes::MEMORY_CACHE
        | WebsiteDataTypes::DISK_CACHE
        | WebsiteDataTypes::SERVICE_WORKER_REGISTRATIONS
        | WebsiteDataTypes::DOM_CACHE
}

// Native objects may outlive the Rust view owner through borrowed GObject clones.
// The lease retains the path record; its final Drop never deletes native storage.
pub(super) fn retain_profile(object: &impl glib::object::ObjectExt, profile: &NativeProfileLease) {
    // SAFETY: this module exclusively owns this key and never retrieves or steals
    // its typed data. GObject destruction only drops the filesystem-free lease.
    unsafe {
        object.set_data("tasty-private-http-profile-lease", profile.clone());
    }
}
