//! WebView2 or WebKit publication of one native HTTP document. Its view authenticates to the
//! document endpoint as a Basic HTTP proxy; the credentials never leave this process.
use super::http_endpoint::listener::{Endpoint, ProxyCredentials};
use crate::app::http_session::HttpSession;
use std::{
    cell::RefCell,
    sync::{Arc, Weak},
};

/// Page-world script that every frame of a remote HTTP view runs when its document is created.
/// WebRTC sends STUN over UDP from the client and neither engine has a setting that keeps it on
/// the proxy, so the script deletes the WebRTC constructors. This narrows the measured paths; it
/// is not a guarantee. Contract: `docs/design/systems/webview.md`.
pub(crate) const WEBRTC_MITIGATION: &str = include_str!("webrtc_mitigation.js");

pub(crate) struct PendingNative {
    session: Arc<HttpSession>,
}
/// One native view context across retained Reloads. Each Reload swaps in the successor session,
/// whose endpoint binds the same loopback proxy address with fresh credentials.
pub(crate) struct NativeDocument {
    native_owner: Arc<()>,
    session: RefCell<Arc<HttpSession>>,
    endpoint: RefCell<Weak<Endpoint>>,
    credentials: RefCell<ProxyCredentials>,
    port: u16,
    replaced: RefCell<Option<Arc<HttpSession>>>,
}
impl PendingNative {
    pub(crate) fn new(session: Arc<HttpSession>) -> Self {
        Self { session }
    }
    pub(crate) fn current(&self) -> bool {
        self.session.is_current()
    }
    pub(crate) fn poll(
        &mut self,
        _wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Option<NativeDocument>, String> {
        let result = self.poll_inner();
        if result.is_err() {
            self.session.fail();
        }
        result
    }
    fn poll_inner(&mut self) -> Result<Option<NativeDocument>, String> {
        if !self.current() {
            return Err("native HTTP preparation retired".into());
        }
        let Some(endpoint) = self.session.endpoint() else {
            return Ok(None);
        };
        let credentials = endpoint
            .proxy_credentials()
            .ok_or("native HTTP endpoint lacks proxy credentials")?;
        self.session.native_store.publish();
        let native_owner = Arc::new(());
        self.session.bind_native_owner(&native_owner)?;
        Ok(Some(NativeDocument {
            native_owner,
            session: RefCell::new(self.session.clone()),
            endpoint: RefCell::new(Arc::downgrade(&endpoint)),
            credentials: RefCell::new(credentials),
            port: endpoint.port(),
            replaced: RefCell::new(None),
        }))
    }
}
impl NativeDocument {
    pub(crate) fn session(&self) -> Arc<HttpSession> {
        self.session.borrow().clone()
    }
    /// The proxy URI the view was created with; it never changes.
    pub(crate) fn proxy_uri(&self) -> String {
        self.credentials.borrow().uri.clone()
    }
    /// Whether `uri` and `challenge` name the current endpoint's own challenge, and its
    /// user name and password if so.
    #[cfg(windows)]
    pub(crate) fn answer(&self, uri: &str, challenge: &str) -> Option<(&'static str, String)> {
        if !self.session().is_current() {
            return None;
        }
        let credentials = self.credentials.borrow();
        (uri == credentials.uri && challenge == credentials.challenge)
            .then(|| (credentials.user, credentials.password.clone()))
    }
    /// WebKit reports no proxy realm, so the endpoint address alone names its challenge. The
    /// server removes Proxy-Authorization before forwarding, so a remote 407 cannot obtain it.
    #[cfg(target_os = "macos")]
    pub(crate) fn answer_proxy(&self, uri: &str) -> Option<(&'static str, String)> {
        if !self.session().is_current() {
            return None;
        }
        let credentials = self.credentials.borrow();
        (uri == credentials.uri).then(|| (credentials.user, credentials.password.clone()))
    }
    /// The exact leaf the endpoint presented for this authority, if it is still current.
    pub(crate) fn expected_leaf(&self, host: &str, port: u16) -> Option<[u8; 32]> {
        if !self.session().is_current() {
            return None;
        }
        self.endpoint.borrow().upgrade()?.expected_leaf(host, port)
    }
    pub(crate) fn retaining(&self) -> bool {
        self.replaced
            .borrow()
            .as_ref()
            .is_some_and(|old| old.retaining())
    }
    /// Keep this view for the successor document. Returns false when the current document has
    /// no Reload receipt to continue; the caller then replaces the view instead.
    pub(crate) fn request_reload(&self) -> Result<bool, String> {
        if self.retaining() {
            return Err("native HTTP replacement already pending".into());
        }
        let old = self.session();
        if !old.reload_captured() {
            return Ok(false);
        }
        old.retain_proxy_port(self.port)?;
        old.retire_reload()?;
        old.request_retained();
        old.close();
        *self.replaced.borrow_mut() = Some(old);
        Ok(true)
    }
    /// Adopt the successor once the predecessor endpoint has joined and the successor endpoint
    /// listens on the same address. Returns true when the view should resume its Reload.
    pub(crate) fn poll_reload(&self, session: &Arc<HttpSession>) -> Result<bool, String> {
        let mut replaced = self.replaced.borrow_mut();
        if replaced.as_ref().is_some_and(|old| !old.retaining()) {
            replaced.take();
            return Ok(false);
        }
        let Some(old) = replaced.as_ref() else {
            return Ok(false);
        };
        if !old.endpoint_joined()? || Arc::ptr_eq(old, session) {
            return Ok(false);
        }
        if !session.is_current() {
            return Err("replacement HTTP session retired".into());
        }
        let Some(endpoint) = session.endpoint() else {
            return Ok(false);
        };
        if endpoint.port() != self.port {
            return Err("replacement HTTP endpoint lost the proxy address".into());
        }
        let credentials = endpoint
            .proxy_credentials()
            .ok_or("native HTTP endpoint lacks proxy credentials")?;
        session.bind_native_owner(&self.native_owner)?;
        *self.session.borrow_mut() = session.clone();
        *self.endpoint.borrow_mut() = Arc::downgrade(&endpoint);
        *self.credentials.borrow_mut() = credentials;
        replaced.take();
        Ok(true)
    }
}
impl Drop for NativeDocument {
    fn drop(&mut self) {
        // Revoke endpoint and body authority before the native view is destroyed.
        let session = self.session();
        session.close();
        session.cancel_retained();
        if let Some(old) = self.replaced.borrow_mut().take() {
            old.cancel_retained();
        }
    }
}
