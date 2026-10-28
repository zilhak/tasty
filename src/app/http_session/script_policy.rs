//! WebView2 fixes script enablement when a navigation starts, before any response exists.
//! The verified main response scan decides instead, and a document whose scripts are not
//! allowed receives a client-local script-blocking policy on every response.
use super::*;
use crate::webview::{http_endpoint::listener::EndpointError, script_gate::ScriptGate};
use tasty_ipc::html_http::Header;
use tasty_remote::html_http_native::NativeRequest;

#[derive(Default)]
pub(super) struct ScriptPolicy {
    gate: Mutex<Option<ScriptGate>>,
    expected_main: Mutex<Option<String>>,
    javascript: AtomicBool,
}
fn blocking() -> Vec<Header> {
    vec![
        Header {
            name: "Content-Security-Policy".into(),
            value: b"script-src 'none'".to_vec(),
        },
        // A blocked copy must not satisfy a later allowed load from the HTTP cache.
        Header {
            name: "Cache-Control".into(),
            value: b"no-store".to_vec(),
        },
    ]
}
impl ScriptPolicy {
    pub(super) fn clear(&self) {
        // reason: a poisoned slot holds no gate to release; the session is closing anyway.
        if let Ok(mut gate) = self.gate.lock() {
            gate.take();
        }
    }
}
impl HttpSession {
    /// The gate decides from the main response scan. The view keeps its own copy.
    pub(crate) fn attach_script_gate(&self, gate: ScriptGate) {
        match self.script.gate.lock() {
            Ok(mut slot) => *slot = Some(gate),
            Err(poison) => {
                drop(poison);
                tracing::error!("native HTTP script gate slot poisoned; scripts stay blocked");
            }
        }
    }
    /// The main-frame navigation whose next matching response decides the document's scripts.
    pub(crate) fn expect_main_response(&self, url: &str) {
        let url = url.split('#').next().unwrap_or(url).to_owned();
        match self.script.expected_main.lock() {
            Ok(mut expected) => *expected = Some(url),
            Err(poison) => {
                drop(poison);
                tracing::error!("native HTTP main navigation slot poisoned");
            }
        }
    }
    /// A commit before the endpoint served the expected main response is WebView2's own page,
    /// such as the proxy challenge attempt or an error page, not a remote document.
    pub(crate) fn awaiting_main_response(&self) -> bool {
        self.script
            .expected_main
            .lock()
            .map_or(true, |expected| expected.is_some())
    }
    pub(super) fn local_headers(
        &self,
        request: &NativeRequest,
    ) -> Result<Vec<Header>, EndpointError> {
        let main = {
            let mut expected = self
                .script
                .expected_main
                .lock()
                .map_err(|_| EndpointError::Worker)?;
            if expected.as_deref() == Some(request.logical_url.as_str()) {
                expected.take();
                true
            } else {
                false
            }
        };
        if main {
            let body = self
                .claim_main_response(&request.logical_url, &request.method)
                .map_err(|error| {
                    tracing::warn!(%error, "native HTTP main response refused");
                    EndpointError::Identity
                })?;
            let gate = self
                .script
                .gate
                .lock()
                .map_err(|_| EndpointError::Worker)?
                .clone();
            let javascript = gate.is_some_and(|gate| {
                gate.main_response_scanned(&request.logical_url, Some(body.scan()))
            });
            self.bodies.finish_replay(&body);
            self.script.javascript.store(javascript, Ordering::Release);
        }
        Ok(if self.script.javascript.load(Ordering::Acquire) {
            Vec::new()
        } else {
            blocking()
        })
    }
}
