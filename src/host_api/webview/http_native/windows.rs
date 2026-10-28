//! WebView2 publication of one native HTTP document. Its view authenticates to the document
//! endpoint as a Basic HTTP proxy; the credentials never leave this process.
use super::http_endpoint::listener::{Endpoint, ProxyCredentials};
use crate::app::http_session::HttpSession;
use std::sync::{Arc, Weak};

pub(crate) struct PendingNative {
    session: Arc<HttpSession>,
}
pub(crate) struct NativeDocument {
    _native_owner: Arc<()>,
    pub(crate) session: Arc<HttpSession>,
    endpoint: Weak<Endpoint>,
    pub(crate) credentials: ProxyCredentials,
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
            _native_owner: native_owner,
            session: self.session.clone(),
            endpoint: Arc::downgrade(&endpoint),
            credentials,
        }))
    }
}
impl NativeDocument {
    /// The exact leaf the endpoint presented for this authority, if it is still current.
    pub(crate) fn expected_leaf(&self, host: &str, port: u16) -> Option<[u8; 32]> {
        if !self.session.is_current() {
            return None;
        }
        self.endpoint.upgrade()?.expected_leaf(host, port)
    }
}
impl Drop for NativeDocument {
    fn drop(&mut self) {
        // Revoke endpoint and body authority before the native view is destroyed.
        self.session.close();
        self.session.cancel_retained();
    }
}
