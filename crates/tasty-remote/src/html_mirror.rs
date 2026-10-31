//! Client entry state shared by transport effects and host chrome; no native or local-file access.
mod http_source;
mod resources;
mod retry;
pub use http_source::{HttpSource, HttpView};

use crate::{
    connection::ConnectionEpoch,
    html_body::{EntryBodyStore, ReceivedEntry},
    html_document::{HtmlDocument, ReceiveState},
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use tasty_ipc::html_transfer::{HtmlMetadata, HtmlReply, HtmlRequest, TransferId};

static BODY_POISONED: AtomicBool = AtomicBool::new(false);
pub struct HtmlMirror {
    pub remote_surface: u32,
    pub binding: u64,
    pub generation: u64,
    pub connection: ConnectionEpoch,
    pub connected: bool,
    pub document: Option<HtmlDocument>,
    pub id: Option<TransferId>,
    pub metadata: Option<HtmlMetadata>,
    pub bodies: Arc<Mutex<EntryBodyStore>>,
    pub failure: Option<String>,
    http_source: Option<HttpSource>,
    pending: Vec<HtmlRequest>,
    reload: bool,
    scan: Option<(
        TransferId,
        mpsc::Receiver<std::io::Result<Arc<ReceivedEntry>>>,
    )>,
    scan_cancel: Arc<AtomicBool>,
    resources: resources::ResourceDriver,
}
impl HtmlMirror {
    pub fn new(remote_surface: u32, binding: u64, connection: ConnectionEpoch) -> Self {
        Self {
            remote_surface,
            binding,
            generation: 1,
            connection,
            connected: true,
            document: None,
            id: None,
            metadata: None,
            bodies: Arc::default(),
            failure: None,
            http_source: None,
            pending: Vec::new(),
            reload: true,
            scan: None,
            scan_cancel: Arc::new(AtomicBool::new(false)),
            resources: Default::default(),
        }
    }
    fn current(&self, id: TransferId) -> bool {
        self.connected
            && self.connection.is_active()
            && !self.reload
            && self
                .document
                .as_ref()
                .is_some_and(|d| d.resources.contains_key(&id))
    }
    pub fn body_store(&self) -> std::sync::MutexGuard<'_, EntryBodyStore> {
        tasty_utils::poison::recover_mutex(
            self.bodies.lock(),
            "remote HTML body store",
            &BODY_POISONED,
        )
    }
    pub fn needs_request(&self) -> bool {
        self.connected && self.connection.is_active() && self.reload
    }
    /// The caller allocates monotonically increasing transfer numbers in actual send order.
    pub fn begin(&mut self, id: TransferId) -> Vec<HtmlRequest> {
        if !self.needs_request()
            || id.surface_id != self.remote_surface
            || id.binding != self.binding
            || id.document != self.generation
        {
            return vec![];
        }
        self.retire_http_source();
        self.cancel_scan();
        let mut cancelled = self.document.take().map_or_else(Vec::new, |mut d| d.stop());
        let mut document = HtmlDocument::new(id.epoch, id.surface_id, id.binding, id.document);
        let start = document.request(id).expect("fresh entry identity");
        let start = self.resources.start(id, start, &mut cancelled);
        cancelled.push(start);
        self.document = Some(document);
        self.id = Some(id);
        self.reload = false;
        self.failure = None;
        self.body_store().begin(id);
        cancelled
    }
    pub fn receive(&mut self, reply: HtmlReply) -> bool {
        if !self.current(reply.id()) {
            return false;
        }
        let reply = self.validate_resource_metadata(reply);
        let id = reply.id();
        let failure = if let HtmlReply::HtmlFailure { reason, .. } = &reply {
            Some(reason.clone())
        } else {
            None
        };
        let Some(document) = &mut self.document else {
            return false;
        };
        let accepted = document.receive(reply);
        if accepted {
            if self.id == Some(id) {
                // An expired size question keeps its card; only a real end is a load failure.
                self.failure = failure.filter(|_| {
                    document
                        .resources
                        .get(&id)
                        .is_some_and(|r| r.state == ReceiveState::Failed)
                });
            } else if document
                .resources
                .get(&id)
                .is_some_and(|r| matches!(r.state, ReceiveState::Failed | ReceiveState::Cancelled))
            {
                document.skipped += 1;
            }
            self.metadata = self
                .id
                .and_then(|id| document.resources.get(&id))
                .and_then(|r| r.metadata.clone());
        }
        self.invalidate_http_source();
        accepted
    }
    pub fn credit(&mut self) -> Option<HtmlRequest> {
        if !self.current(self.id?) {
            return None;
        }
        self.next_credit()
    }
    /// A notice click carries the exact transfer ID drawn by the host, never just a surface number.
    pub fn allow(&mut self, id: TransferId) -> bool {
        if !self.current(id) {
            return false;
        }
        let Some(d) = self.document.as_mut() else {
            return false;
        };
        if d.notices.front() != Some(&id)
            || d.resources
                .get(&id)
                .is_none_or(|r| r.state != ReceiveState::SizeNotice)
        {
            return false;
        }
        d.allow_next() == Some(id)
    }
    pub fn decline(&mut self, id: TransferId) -> Option<HtmlRequest> {
        if !self.current(id)
            || self
                .document
                .as_ref()
                .is_none_or(|d| d.notices.front() != Some(&id))
        {
            return None;
        }
        if self.id == Some(id) {
            self.cancel_scan();
            self.body_store().cancel(id);
        } else if let Some(resources) = self.body_store().resources_mut() {
            resources.fail(id, crate::html_body::ResourceFailure::Declined);
        }
        self.document.as_mut()?.decline(id)
    }
    pub fn continue_receiving(&mut self, id: TransferId) -> bool {
        if !self.current(id) {
            return false;
        }
        let Some(d) = self.document.as_mut().filter(|d| d.paused) else {
            return false;
        };
        d.continue_receiving();
        true
    }
    pub fn stop(&mut self, id: TransferId) -> Vec<HtmlRequest> {
        if !self.current(id) {
            return vec![];
        }
        self.cancel_scan();
        if self.state() != Some(ReceiveState::Complete) {
            self.body_store().clear();
        } else {
            self.body_store()
                .stop_resources(crate::html_body::ResourceFailure::Stopped);
        }
        self.document
            .as_mut()
            .map_or_else(Vec::new, HtmlDocument::stop)
    }
    pub fn queue_decline(&mut self, id: TransferId) {
        if let Some(cancel) = self.decline(id) {
            self.pending.push(cancel);
        }
    }
    pub fn queue_stop(&mut self, id: TransferId) {
        if !self.connection.is_active() || self.document.as_ref().is_none_or(|d| !d.paused) {
            return;
        }
        let cancel = self.stop(id);
        self.pending.extend(cancel);
    }
    pub fn take_pending(&mut self) -> Vec<HtmlRequest> {
        std::mem::take(&mut self.pending)
    }
    pub fn user_reload(&mut self) -> bool {
        if !self.connected || !self.connection.is_active() || self.reload {
            return false;
        }
        let Some(next) = self.generation.checked_add(1) else {
            return false;
        };
        self.retire_http_source();
        self.cancel_scan();
        self.body_store().clear();
        self.generation = next;
        self.reload = true;
        true
    }
    /// Native work awaiting admission and aggregate consent still keep the host receiving page.
    pub fn receiving(&self) -> bool {
        self.document.as_ref().is_none_or(HtmlDocument::receiving)
            || self
                .body_store()
                .resources_mut()
                .is_some_and(|resources| resources.has_pending())
    }
    pub fn state(&self) -> Option<ReceiveState> {
        self.document
            .as_ref()?
            .resources
            .get(&self.id?)
            .map(|r| r.state)
    }
    /// Only one scan per body; publication remains guarded even if a worker finishes after cancellation.
    pub fn poll_scan(&mut self, notify: Arc<dyn Fn() + Send + Sync>) -> bool {
        if self.connected
            && self.connection.is_active()
            && !self.reload
            && self.scan.is_none()
            && self.state() == Some(ReceiveState::Complete)
            && self.body_store().current().is_none()
            && self.failure.is_none()
        {
            let id = self.id.expect("complete entry has identity");
            let resource = self.document.as_ref().unwrap().resources[&id].clone();
            let (tx, rx) = mpsc::sync_channel(1);
            let cancel = Arc::clone(&self.scan_cancel);
            match std::thread::Builder::new()
                .name("remote-html-scan".into())
                .spawn(move || {
                    let result = ReceivedEntry::from_complete_cancellable(id, &resource, &cancel)
                        .and_then(|entry| {
                            entry
                                .map(Arc::new)
                                .ok_or_else(|| std::io::Error::other("entry is incomplete"))
                        });
                    if tx.send(result).is_ok() {
                        notify();
                    }
                }) {
                Ok(_worker) => self.scan = Some((id, rx)),
                Err(error) => self.failure = Some(error.to_string()),
            }
        }
        let Some((id, rx)) = &self.scan else {
            return false;
        };
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return false,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err(std::io::Error::other("entry scan worker disconnected"))
            }
        };
        let id = *id;
        self.scan = None;
        if !self.connected || !self.connection.is_active() || self.id != Some(id) || self.reload {
            return false;
        }
        match result {
            Ok(entry) => self.body_store().publish(entry),
            Err(error) => {
                self.failure = Some(error.to_string());
                true
            }
        }
    }
    pub fn disconnect(&mut self) {
        self.retire_http_source();
        self.connected = false;
        self.pending.clear();
        self.cancel_scan();
        self.body_store().disconnect();
        if let Some(d) = self.document.as_mut() {
            d.stop();
        }
    }
    fn cancel_scan(&mut self) {
        self.scan_cancel.store(true, Ordering::Release);
        self.scan = None;
        self.scan_cancel = Arc::new(AtomicBool::new(false));
    }
}
impl Drop for HtmlMirror {
    fn drop(&mut self) {
        self.retire_http_source();
        self.cancel_scan();
    }
}

#[cfg(test)]
mod tests;
