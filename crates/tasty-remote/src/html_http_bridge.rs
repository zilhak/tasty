//! Native request owners meet the ordered model only at the real engine-driven Begin send.
mod drive;
mod reload;
mod wait;
use crate::{
    connection::ConnectionSender,
    html_http_body::{BodyBudget, VerifiedBody},
    html_http_client::{ClientLimits, DocumentKey, PendingBudget, Snapshot, VerifyJob},
    html_http_native::NativeRequest,
    html_http_session::{CapturedDocument, HttpAction},
};
pub use reload::ReloadAuthority;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use tasty_ipc::{
    html_http::{HttpReply, ResponseId},
    html_transfer::TransferId,
};
pub use wait::{NativeResponse, NativeWait};
#[derive(Clone)]
pub struct BridgeScope {
    pub epoch: u64,
    pub surface_id: u32,
    pub binding: u64,
    pub document: u64,
    pub source_generation: u64,
    pub entry_url: String,
    pub allow_remote_content: bool,
}
pub struct BridgeConfig {
    pub scope: BridgeScope,
    pub limits: ClientLimits,
    pub pending: PendingBudget,
    pub bodies: BodyBudget,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BridgeError {
    Invalid,
    Stale,
    Closed,
    Capacity,
    Declined,
    Failed,
}
type Current = Arc<dyn Fn() -> bool + Send + Sync>;
struct Inner {
    document: Option<Arc<CapturedDocument>>,
    slots: Vec<Arc<wait::Slot>>,
    jobs: Vec<VerifyJob>,
}
pub struct HttpBridge {
    config: BridgeConfig,
    reload: Option<ReloadAuthority>,
    reload_reserved: Arc<AtomicBool>,
    sender: Arc<ConnectionSender>,
    source: Current,
    wake: Arc<dyn Fn() + Send + Sync>,
    live: Arc<AtomicBool>,
    closing: AtomicBool,
    driving: AtomicBool,
    inner: Mutex<Inner>,
}
impl HttpBridge {
    pub fn new(
        config: BridgeConfig,
        sender: Arc<ConnectionSender>,
        current: Current,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Arc<Self>, BridgeError> {
        let s = &config.scope;
        let l = config.limits;
        let cap = tasty_ipc::admission::QUEUED_BYTES_LIMIT;
        if [
            s.epoch,
            u64::from(s.surface_id),
            s.binding,
            s.document,
            s.source_generation,
        ]
        .contains(&0)
            || l.requests == 0
            || l.requests > 256
            || l.verify_jobs == 0
            || l.verify_jobs > 8
            || [l.upload_bytes, l.encoded_bytes, l.decoded_bytes]
                .iter()
                .any(|n| *n == 0 || *n > cap)
        {
            return Err(BridgeError::Invalid);
        }
        let url = url::Url::parse(&s.entry_url).map_err(|_| BridgeError::Invalid)?;
        if !matches!(url.scheme(), "http" | "https")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
            || url.as_str() != s.entry_url
            || s.entry_url.len() > tasty_ipc::html_http::URL_BYTES
        {
            return Err(BridgeError::Invalid);
        }
        if !sender.epoch().is_active() || !current() {
            return Err(BridgeError::Stale);
        }
        Ok(Arc::new(Self {
            config,
            reload: None,
            reload_reserved: Arc::new(AtomicBool::new(false)),
            sender,
            source: current,
            wake,
            live: Arc::new(AtomicBool::new(true)),
            closing: AtomicBool::new(false),
            driving: AtomicBool::new(false),
            inner: Mutex::new(Inner {
                document: None,
                slots: Vec::new(),
                jobs: Vec::new(),
            }),
        }))
    }
    fn current(&self) -> bool {
        !self.closing.load(Ordering::Acquire)
            && self.live.load(Ordering::Acquire)
            && self.sender.epoch().is_active()
            && (self.source)()
            && self.reload.as_ref().is_none_or(ReloadAuthority::valid)
    }
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Inner>, BridgeError> {
        self.inner.lock().map_err(|_| {
            self.live.store(false, Ordering::Release);
            tracing::error!("native HTTP bridge poisoned; publication refused");
            BridgeError::Failed
        })
    }
    pub fn submit(
        self: &Arc<Self>,
        request: Arc<NativeRequest>,
    ) -> Result<NativeWait, BridgeError> {
        self.wait(request, None)
    }
    pub fn reuse(
        self: &Arc<Self>,
        response: ResponseId,
        request: Arc<NativeRequest>,
    ) -> Result<NativeWait, BridgeError> {
        if !self.current() {
            return Err(BridgeError::Stale);
        }
        let document = self.document().ok_or(BridgeError::Invalid)?;
        let body = document
            .with_current(|m| m.verified(response.request))
            .flatten()
            .ok_or(BridgeError::Invalid)?;
        if !body.matches_native(response, &request.method, request.identity)
            || body.head().logical_url != request.logical_url
        {
            return Err(BridgeError::Invalid);
        }
        self.wait(request, Some(response))
    }
    fn wait(
        self: &Arc<Self>,
        request: Arc<NativeRequest>,
        reuse: Option<ResponseId>,
    ) -> Result<NativeWait, BridgeError> {
        if !self.current() {
            return Err(BridgeError::Stale);
        }
        let slot = {
            let mut inner = self.lock()?;
            if !self.current() {
                return Err(BridgeError::Stale);
            }
            if inner.slots.len() >= 8 {
                return Err(BridgeError::Capacity);
            }
            let slot = Arc::new(wait::Slot::new(request, reuse, self.wake.clone()));
            inner.slots.push(slot.clone());
            slot
        };
        (self.wake)();
        Ok(NativeWait::new(self, slot))
    }
    pub fn document(&self) -> Option<Arc<CapturedDocument>> {
        if !self.current() {
            return None;
        }
        self.lock().ok()?.document.clone()
    }
    pub fn receive(&self, reply: HttpReply) -> bool {
        let document = self.document();
        let first = document.as_ref().is_some_and(|d| d.entry() == reply.id());
        let ready = matches!(&reply, HttpReply::UploadReady { .. });
        let accepted = document
            .as_ref()
            .is_some_and(|document| document.receive(reply));
        if accepted
            && first
            && let Some(reload) = &self.reload
        {
            let state = document.and_then(|d| d.with_current(|m| m.state(d.entry())).flatten());
            if ready && state == Some(crate::html_http_client::ReceiveState::Uploading) {
                reload.consumed();
            }
            if matches!(
                state,
                Some(
                    crate::html_http_client::ReceiveState::Failed
                        | crate::html_http_client::ReceiveState::Cancelled
                        | crate::html_http_client::ReceiveState::Released
                )
            ) {
                reload.abandon();
            }
        }
        if accepted {
            (self.wake)();
        }
        accepted
    }
    pub fn snapshot(&self) -> Snapshot {
        if let Some(document) = self.document() {
            return document
                .with_current(|m| m.snapshot())
                .unwrap_or_else(empty);
        }
        let mut snapshot = empty();
        snapshot.receiving = self.current()
            && self.lock().is_ok_and(|inner| {
                inner
                    .slots
                    .iter()
                    .any(|slot| !slot.abandoned.load(Ordering::Acquire))
            });
        snapshot
    }
    fn action(&self, action: HttpAction) -> bool {
        let accepted = self
            .document()
            .is_some_and(|doc| doc.apply(&self.sender, action).unwrap_or(false));
        if accepted {
            (self.wake)();
        }
        accepted
    }
    pub fn allow(&self, id: TransferId) -> bool {
        self.action(HttpAction::Allow(id))
    }
    pub fn decline(&self, id: TransferId) -> bool {
        self.action(HttpAction::Decline(id))
    }
    pub fn continue_receiving(&self) -> bool {
        self.action(HttpAction::Continue)
    }
    pub fn stop(&self) -> bool {
        self.action(HttpAction::Stop)
    }
    pub fn release(&self, response: ResponseId) -> bool {
        self.action(HttpAction::Release(response))
    }
    pub fn close(&self) {
        if self.closing.swap(true, Ordering::AcqRel) {
            return;
        }
        self.retire_contents(true);
    }
    fn retire_contents(&self, publish_close: bool) {
        let retired = self.lock().ok().map(|mut inner| {
            (
                inner.document.take(),
                std::mem::take(&mut inner.slots),
                std::mem::take(&mut inner.jobs),
            )
        });
        if let Some((document, slots, jobs)) = retired {
            if let Some(document) = document {
                if publish_close && let Err(error) = document.close_and_send(&self.sender) {
                    tracing::debug!(?error, "native HTTP bridge close not enqueued");
                }
                document.retire();
            }
            self.live.store(false, Ordering::Release);
            for slot in slots {
                slot.finish(Err(BridgeError::Closed));
            }
            drop(jobs);
        } else {
            self.live.store(false, Ordering::Release);
        }
        if let Some(reload) = &self.reload {
            reload.abandon();
        }
        (self.wake)();
    }
}
impl Drop for HttpBridge {
    fn drop(&mut self) {
        self.close();
    }
}
fn empty() -> Snapshot {
    Snapshot {
        received: 0,
        reserved: 0,
        paused: false,
        receiving: false,
        skipped: 0,
        notices: vec![],
    }
}
#[cfg(test)]
mod tests;

#[cfg(test)]
mod boundary_tests;

#[cfg(test)]
mod reload_tests;
