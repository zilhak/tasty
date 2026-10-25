//! An untrusted, script-disabled probe manager retains a real worker-prepared private profile.
use super::*;
use gtk::prelude::*;
use std::time::Duration;
use webkit2gtk::{LoadEvent, NetworkProxyMode, NetworkProxySettings, TLSErrorsPolicy};
type Current = Arc<dyn Fn() -> bool + Send + Sync>;
type ProbeDone = Box<dyn FnOnce(Result<NativeProbeEvidence, ContextError>)>;
pub(crate) struct NativeProbeEvidence {
    pub uri: String,
    pub certificate: gio::TlsCertificate,
    pub flags: gio::TlsCertificateFlags,
}
struct Attempt {
    id: u64,
    current: Current,
    done: Option<ProbeDone>,
    cancel: gio::Cancellable,
    view: Option<WebView>,
    timer: Option<glib::SourceId>,
}
pub(crate) struct NativeProbeStore {
    manager: WebsiteDataManager,
    profile: NativeProfileLease,
    sequence: Cell<u64>,
    clearing: Cell<Option<u64>>,
    attempt: RefCell<Option<Attempt>>,
}
impl NativeProbeStore {
    pub(crate) fn new(profile: PreparedProfile) -> Result<Rc<Self>, ContextError> {
        let manager = WebsiteDataManager::builder()
            .base_data_directory(profile.data_dir().to_str().ok_or(ContextError::Storage)?)
            .base_cache_directory(profile.cache_dir().to_str().ok_or(ContextError::Storage)?)
            .build();
        if manager.is_ephemeral() {
            return Err(ContextError::Storage);
        }
        manager.set_persistent_credential_storage_enabled(false);
        manager.set_tls_errors_policy(TLSErrorsPolicy::Fail);
        let profile = profile.into_native();
        store::retain_profile(&manager, &profile);
        Ok(Rc::new(Self {
            manager,
            profile,
            sequence: Cell::new(0),
            clearing: Cell::new(None),
            attempt: RefCell::new(None),
        }))
    }
    pub(crate) fn busy(&self) -> bool {
        self.clearing.get().is_some() || self.attempt.borrow().is_some()
    }
    pub(crate) fn start(
        self: &Rc<Self>,
        proxy: String,
        url: String,
        current: Current,
        done: ProbeDone,
    ) -> Result<(), ContextError> {
        if self.busy() {
            return Err(ContextError::Busy);
        }
        if !current() {
            return Err(ContextError::Retired);
        }
        if !authority::proxy(&proxy) || authority::https(&url).is_none() {
            return Err(ContextError::Policy);
        }
        let id = self
            .sequence
            .get()
            .checked_add(1)
            .ok_or(ContextError::Capacity)?;
        self.sequence.set(id);
        let cancel = gio::Cancellable::new();
        *self.attempt.borrow_mut() = Some(Attempt {
            id,
            current,
            done: Some(done),
            cancel: cancel.clone(),
            view: None,
            timer: None,
        });
        self.clearing.set(Some(id));
        let weak = glib::thread_guard::ThreadGuard::new(Rc::downgrade(self));
        let profile = self.profile.clone();
        self.manager.clear(
            store::clear_types(),
            glib::TimeSpan::from_microseconds(0),
            Some(&cancel),
            move |result| {
                let _profile = profile;
                if let Some(state) = weak.into_inner().upgrade() {
                    if state.clearing.get() == Some(id) {
                        state.clearing.set(None);
                    }
                    if result.is_err() {
                        state.finish(id, Err(ContextError::Cache));
                    } else {
                        state.begin(id, proxy, url);
                    }
                }
            },
        );
        Ok(())
    }
    fn begin(self: &Rc<Self>, id: u64, proxy: String, url: String) {
        if !self
            .attempt
            .borrow()
            .as_ref()
            .is_some_and(|a| a.id == id && (a.current)())
        {
            self.finish(id, Err(ContextError::Retired));
            return;
        }
        let mut proxy = NetworkProxySettings::new(Some(&proxy), &[]);
        self.manager
            .set_network_proxy_settings(NetworkProxyMode::Custom, Some(&mut proxy));
        let context = WebContext::with_website_data_manager(&self.manager);
        store::retain_profile(&context, &self.profile);
        let webview = WebView::with_context(&context);
        store::retain_profile(&webview, &self.profile);
        if let Some(settings) = WebViewExt::settings(&webview) {
            settings.set_enable_javascript(false);
            settings.set_enable_page_cache(false);
        }
        let weak = Rc::downgrade(self);
        webview.connect_load_failed_with_tls_errors(move |_, uri, certificate, flags| {
            if let Some(state) = weak.upgrade() {
                state.finish(
                    id,
                    Ok(NativeProbeEvidence {
                        uri: uri.into(),
                        certificate: certificate.clone(),
                        flags,
                    }),
                );
            }
            true
        });
        let weak = Rc::downgrade(self);
        webview.connect_load_failed(move |_, _, _, _| {
            if let Some(state) = weak.upgrade() {
                state.finish(id, Err(ContextError::Tls));
            }
            true
        });
        let weak = Rc::downgrade(self);
        webview.connect_load_changed(move |_, event| {
            if event == LoadEvent::Finished
                && let Some(state) = weak.upgrade()
            {
                state.finish(id, Err(ContextError::Tls));
            }
        });
        let weak = Rc::downgrade(self);
        let timer = glib::timeout_add_local_once(Duration::from_secs(10), move || {
            if let Some(state) = weak.upgrade() {
                if let Some(attempt) = state.attempt.borrow_mut().as_mut().filter(|a| a.id == id) {
                    attempt.timer.take();
                }
                state.finish(id, Err(ContextError::Tls));
            }
        });
        if let Some(attempt) = self.attempt.borrow_mut().as_mut().filter(|a| a.id == id) {
            attempt.view = Some(webview.clone());
            attempt.timer = Some(timer);
        } else {
            timer.remove();
            return;
        }
        webview.load_uri(&url);
    }
    fn finish(&self, id: u64, result: Result<NativeProbeEvidence, ContextError>) {
        let attempt = {
            let mut attempt = self.attempt.borrow_mut();
            if attempt.as_ref().is_none_or(|a| a.id != id) {
                return;
            }
            attempt.take()
        };
        if let Some(mut attempt) = attempt {
            let current = (attempt.current)();
            let done = attempt.done.take();
            drop(attempt);
            if let Some(done) = done {
                done(if current {
                    result
                } else {
                    Err(ContextError::Retired)
                });
            }
        }
    }
    pub(crate) fn cancel(&self) {
        let id = self.attempt.borrow().as_ref().map(|a| a.id);
        if let Some(id) = id {
            self.finish(id, Err(ContextError::Retired));
        }
    }
}
impl Drop for Attempt {
    fn drop(&mut self) {
        self.cancel.cancel();
        if let Some(timer) = self.timer.take() {
            timer.remove();
        }
        if let Some(view) = self.view.take() {
            view.stop_loading();
            // SAFETY: this private, unembedded widget is destroyed on its creating GTK thread.
            unsafe {
                view.destroy();
            }
        }
    }
}
impl Drop for NativeProbeStore {
    fn drop(&mut self) {
        self.cancel();
    }
}
