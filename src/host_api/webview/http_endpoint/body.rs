//! Actual endpoint requests await the shared verified-body bridge; replay is explicit and finite.
use super::{
    io::Current,
    listener::{EndpointError, Handler, ResponseOwner},
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use tasty_ipc::html_http::ResponseId;
use tasty_remote::{
    html_http_body::VerifiedBody,
    html_http_bridge::{BridgeError, HttpBridge, NativeResponse},
    html_http_native::NativeRequest,
};
impl ResponseOwner for NativeResponse {
    fn body(&self) -> &Arc<VerifiedBody> {
        &self.body
    }
    fn request(&self) -> &Arc<NativeRequest> {
        &self.request
    }
}
struct Replay {
    body: Arc<VerifiedBody>,
    used: bool,
    delivered: bool,
}
pub(crate) struct BodySource {
    bridge: Arc<HttpBridge>,
    view: tasty_remote::html_mirror::HttpView,
    current: Current,
    live: AtomicBool,
    replay: Mutex<Option<Replay>>,
    stop: tokio::sync::watch::Sender<bool>,
}
impl BodySource {
    pub(crate) fn new(
        bridge: Arc<HttpBridge>,
        view: tasty_remote::html_mirror::HttpView,
        current: Current,
    ) -> Arc<Self> {
        let (stop, _) = tokio::sync::watch::channel(false);
        Arc::new(Self {
            bridge,
            view,
            current,
            live: AtomicBool::new(true),
            replay: Mutex::new(None),
            stop,
        })
    }
    fn current(&self) -> bool {
        self.live.load(Ordering::Acquire) && self.view.is_current() && (self.current)()
    }
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Option<Replay>>, EndpointError> {
        self.replay.lock().map_err(|_| {
            self.retire();
            tracing::error!("native response replay state poisoned; delivery refused");
            EndpointError::Body
        })
    }
    /// The actual current main-resource policy supplies this retained full identity only after
    /// the user's script action. This method does not grant JavaScript or trigger navigation.
    pub(crate) fn replay_once(&self, response: ResponseId) -> Result<(), EndpointError> {
        if !self.current() {
            return Err(EndpointError::Retired);
        }
        let body = self
            .bridge
            .document()
            .and_then(|document| {
                document
                    .with_current(|model| model.verified(response.request))
                    .flatten()
            })
            .filter(|body| body.head().id == response)
            .ok_or(EndpointError::Identity)?;
        let mut replay = self.lock()?;
        if replay.is_some() {
            return Err(EndpointError::Capacity);
        }
        if !self.current() {
            return Err(EndpointError::Retired);
        }
        *replay = Some(Replay {
            body,
            used: false,
            delivered: false,
        });
        Ok(())
    }
    /// Clear only after the native main-response policy validated this exact delivered body.
    /// A late acknowledgement cannot clear a replacement response's one-use replay fence.
    pub(crate) fn finish_replay(&self, body: &Arc<VerifiedBody>) -> bool {
        if !self.current() {
            return false;
        }
        let Ok(mut replay) = self.lock() else {
            return false;
        };
        if replay
            .as_ref()
            .is_some_and(|state| state.used && state.delivered && Arc::ptr_eq(&state.body, body))
        {
            replay.take();
            true
        } else {
            false
        }
    }
    fn route(&self, request: &NativeRequest) -> Result<Option<ResponseId>, EndpointError> {
        if !self.current() {
            return Err(EndpointError::Retired);
        }
        let mut replay = self.lock()?;
        let Some(state) = replay
            .as_mut()
            .filter(|state| state.body.head().logical_url == request.logical_url)
        else {
            return Ok(None);
        };
        // Keep the used/refused tombstone until exact native-policy acknowledgement or document
        // retirement. A browser retry must not fall through to another upstream POST.
        if state.used {
            return Err(EndpointError::Identity);
        }
        state.used = true;
        let response = state.body.head().id;
        if !state
            .body
            .matches_native(response, &request.method, request.identity)
        {
            return Err(EndpointError::Identity);
        }
        Ok(Some(response))
    }
    pub(crate) fn handler(self: &Arc<Self>) -> Handler {
        let source = self.clone();
        Arc::new(move |request| {
            let source = source.clone();
            Box::pin(async move {
                let replay = source.route(&request)?;
                let wait = match replay {
                    Some(response) => source.bridge.reuse(response, request),
                    None => source.bridge.submit(request),
                }
                .map_err(error)?;
                let mut stop = source.stop.subscribe();
                if !source.current() {
                    return Err(EndpointError::Retired);
                }
                let response = tokio::select! {result=wait=>result.map_err(error)?,_=stop.changed()=>return Err(EndpointError::Retired)};
                if !source.current() {
                    return Err(EndpointError::Retired);
                }
                if replay.is_some() {
                    let mut state = source.lock()?;
                    if let Some(state) = state
                        .as_mut()
                        .filter(|state| state.used && Arc::ptr_eq(&state.body, &response.body))
                    {
                        state.delivered = true;
                    } else {
                        return Err(EndpointError::Identity);
                    }
                }
                Ok(Box::new(response) as Box<dyn ResponseOwner>)
            })
        })
    }
    pub(crate) fn retire(&self) {
        self.live.store(false, Ordering::Release);
        self.stop.send_replace(true);
    }
}
fn error(error: BridgeError) -> EndpointError {
    match error {
        BridgeError::Invalid => EndpointError::Identity,
        BridgeError::Stale | BridgeError::Closed => EndpointError::Retired,
        BridgeError::Capacity => EndpointError::Capacity,
        BridgeError::Declined | BridgeError::Failed => EndpointError::Body,
    }
}
#[cfg(test)]
mod tests;
