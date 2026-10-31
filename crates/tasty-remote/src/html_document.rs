//! Received-body ownership and decimal byte budgets, independent of native WebViews.
use base64::Engine;
use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};
use tasty_ipc::html_transfer::{
    AUTO_BYTES, CHUNK_BYTES, DOCUMENT_BYTES, HtmlMetadata, HtmlReply, HtmlRequest, TransferId,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiveState {
    Metadata,
    SizeNotice,
    Ready,
    Receiving,
    Complete,
    Skipped,
    Failed,
    Cancelled,
}
#[derive(Debug, Clone)]
pub struct Resource {
    id: TransferId,
    pub metadata: Option<HtmlMetadata>,
    pub state: ReceiveState,
    body: Vec<u8>,
    completed: Option<Arc<Vec<u8>>>,
    credit: bool,
    allowed: bool,
    /// The server ended this transfer while it waited for the user (size or aggregate consent).
    /// The question stays; consent issues a fresh request instead of reviving the old transfer.
    expired: bool,
    /// Consent arrived after expiry; the owner must bind a fresh transfer identity.
    retry: bool,
    /// The exact size the user already accepted. A different size asks again.
    allowed_size: Option<u64>,
}
impl Resource {
    fn fresh(id: TransferId) -> Self {
        Self {
            id,
            metadata: None,
            state: ReceiveState::Metadata,
            body: Vec::new(),
            completed: None,
            credit: false,
            allowed: false,
            expired: false,
            retry: false,
            allowed_size: None,
        }
    }
    pub fn expired(&self) -> bool {
        self.expired
    }
    pub fn id(&self) -> TransferId {
        self.id
    }
    /// Script allowance reloads reuse this exact completed body, without issuing another request.
    pub fn body(&self) -> Option<&[u8]> {
        self.completed.as_deref().map(Vec::as_slice)
    }
    /// Share the completed allocation with a scanner and native response; prefixes remain private.
    pub fn shared_body(&self) -> Option<Arc<Vec<u8>>> {
        self.completed.clone()
    }
}
#[derive(Debug)]
pub struct HtmlDocument {
    pub epoch: u64,
    pub surface: u32,
    pub binding: u64,
    pub generation: u64,
    pub received: u64,
    pub skipped: usize,
    pub paused: bool,
    pub stopped: bool,
    continued: bool,
    pub resources: HashMap<TransferId, Resource>,
    pub notices: VecDeque<TransferId>,
}
impl HtmlDocument {
    pub fn new(epoch: u64, surface: u32, binding: u64, generation: u64) -> Self {
        Self {
            epoch,
            surface,
            binding,
            generation,
            received: 0,
            skipped: 0,
            paused: false,
            stopped: false,
            continued: false,
            resources: HashMap::new(),
            notices: VecDeque::new(),
        }
    }
    fn matches(&self, id: TransferId) -> bool {
        id.epoch == self.epoch
            && id.surface_id == self.surface
            && id.binding == self.binding
            && id.document == self.generation
    }
    pub fn request(&mut self, id: TransferId) -> Option<HtmlRequest> {
        if !self.matches(id)
            || self.stopped
            || self.resources.contains_key(&id)
            || self.resources.len() >= 256
        {
            return None;
        }
        self.resources.insert(id, Resource::fresh(id));
        Some(HtmlRequest::HtmlEntryRequest { id })
    }
    /// Returns false for stale/unsolicited replies. Invalid current replies terminate only that resource.
    pub fn receive(&mut self, reply: HtmlReply) -> bool {
        let id = reply.id();
        if !self.matches(id) {
            return false;
        }
        let Some(r) = self.resources.get_mut(&id) else {
            return false;
        };
        if matches!(
            r.state,
            ReceiveState::Complete
                | ReceiveState::Skipped
                | ReceiveState::Failed
                | ReceiveState::Cancelled
        ) || r.expired
        {
            return false;
        }
        let valid = match reply {
            HtmlReply::HtmlMetadata { metadata, .. }
                if r.state == ReceiveState::Metadata && metadata.regular =>
            {
                let accepted = r.allowed_size.take() == Some(metadata.size);
                r.state = if metadata.size > AUTO_BYTES && !accepted {
                    r.allowed = false;
                    self.notices.push_back(id);
                    ReceiveState::SizeNotice
                } else {
                    r.allowed = metadata.size > AUTO_BYTES;
                    ReceiveState::Ready
                };
                r.metadata = Some(metadata);
                true
            }
            HtmlReply::HtmlMetadata { metadata, .. }
                if r.state == ReceiveState::Metadata && !metadata.regular =>
            {
                r.metadata = Some(metadata);
                r.state = ReceiveState::Failed;
                true
            }
            HtmlReply::HtmlChunk {
                offset, data_b64, ..
            } if r.credit => {
                r.credit = false;
                let bytes = (data_b64.len() <= CHUNK_BYTES.div_ceil(3) * 4)
                    .then(|| base64::engine::general_purpose::STANDARD.decode(data_b64))
                    .and_then(Result::ok);
                if let Some(bytes) = bytes.filter(|b| {
                    !b.is_empty()
                        && b.len() <= CHUNK_BYTES
                        && offset == r.body.len() as u64
                        && offset
                            .checked_add(b.len() as u64)
                            .is_some_and(|end| r.metadata.as_ref().is_some_and(|m| end <= m.size))
                }) {
                    self.received = self.received.saturating_add(bytes.len() as u64);
                    r.body.extend_from_slice(&bytes);
                    r.state = ReceiveState::Ready;
                    true
                } else {
                    false
                }
            }
            HtmlReply::HtmlComplete { size, .. }
                if r.metadata.as_ref().is_some_and(|m| size == m.size)
                    && size == r.body.len() as u64 =>
            {
                r.credit = false;
                r.completed = Some(Arc::new(std::mem::take(&mut r.body)));
                r.state = ReceiveState::Complete;
                true
            }
            // The server's credit timeout cannot be told apart from other failures without
            // comparing reason text, so every failure while waiting on the user keeps the
            // question; a changed or missing file is reported again by the fresh request.
            HtmlReply::HtmlFailure { .. }
                if r.state == ReceiveState::SizeNotice
                    || (self.paused && r.state == ReceiveState::Ready && !r.credit) =>
            {
                self.received = self.received.saturating_sub(r.body.len() as u64);
                r.body.clear();
                r.expired = true;
                true
            }
            HtmlReply::HtmlFailure { .. } => {
                r.state = ReceiveState::Failed;
                r.body.clear();
                r.credit = false;
                true
            }
            HtmlReply::HtmlCancelled { .. } => {
                r.state = ReceiveState::Cancelled;
                r.body.clear();
                r.credit = false;
                true
            }
            _ => false,
        };
        if !valid {
            r.state = ReceiveState::Failed;
            r.body.clear();
            r.credit = false;
        }
        if matches!(
            r.state,
            ReceiveState::Complete | ReceiveState::Failed | ReceiveState::Cancelled
        ) {
            self.notices.retain(|n| *n != id);
        }
        true
    }
    /// One in-flight read per resource; duplicate credits cannot grow the server's output queue.
    pub fn credit(&mut self, id: TransferId) -> Option<HtmlRequest> {
        if self.paused || self.stopped {
            return None;
        }
        let r = self.resources.get_mut(&id)?;
        if r.state != ReceiveState::Ready || r.credit || r.expired {
            return None;
        }
        let size = r.metadata.as_ref()?.size;
        let remaining = size.saturating_sub(r.body.len() as u64);
        // Reserve against all outstanding credits, not merely already received chunks.
        let outstanding: u64 = self
            .resources
            .values()
            .filter(|r| r.credit)
            .map(|r| {
                r.metadata.as_ref().map_or(0, |m| {
                    (m.size - r.body.len() as u64).min(CHUNK_BYTES as u64)
                })
            })
            .sum();
        if !self.continued
            && self
                .received
                .saturating_add(outstanding)
                .saturating_add(remaining.min(CHUNK_BYTES as u64))
                > DOCUMENT_BYTES
        {
            self.paused = true;
            return None;
        }
        let r = self.resources.get_mut(&id)?;
        r.credit = true;
        r.state = ReceiveState::Receiving;
        Some(HtmlRequest::HtmlRead {
            id,
            offset: r.body.len() as u64,
            allow: r.allowed,
        })
    }
    pub fn allow_next(&mut self) -> Option<TransferId> {
        let id = self.notices.pop_front()?;
        let r = self.resources.get_mut(&id)?;
        if r.expired {
            r.retry = true;
            r.state = ReceiveState::Metadata;
            r.allowed_size = r.metadata.as_ref().map(|m| m.size);
        } else {
            r.allowed = true;
            r.state = ReceiveState::Ready;
        }
        Some(id)
    }
    pub fn decline(&mut self, id: TransferId) -> Option<HtmlRequest> {
        let r = self.resources.get_mut(&id)?;
        if matches!(
            r.state,
            ReceiveState::Complete
                | ReceiveState::Skipped
                | ReceiveState::Failed
                | ReceiveState::Cancelled
        ) {
            return None;
        }
        r.body.clear();
        r.credit = false;
        r.state = ReceiveState::Skipped;
        // An expired transfer has no server worker left to cancel.
        let cancel = (!r.expired).then_some(HtmlRequest::HtmlCancel { id });
        r.retry = false;
        self.skipped += 1;
        self.notices.retain(|n| *n != id);
        cancel
    }
    /// Files whose transfer expired during the pause are requested again, keeping their consent.
    pub fn continue_receiving(&mut self) {
        self.paused = false;
        self.continued = true;
        for r in self.resources.values_mut() {
            if r.expired && r.state == ReceiveState::Ready {
                r.retry = true;
                r.state = ReceiveState::Metadata;
                r.allowed_size = r
                    .allowed
                    .then(|| r.metadata.as_ref().map(|m| m.size))
                    .flatten();
            }
        }
    }
    /// Expired transfers the user already answered, oldest first; empty while paused or stopped.
    pub fn retries(&self) -> Vec<TransferId> {
        if self.paused || self.stopped {
            return vec![];
        }
        let mut ids: Vec<_> = self
            .resources
            .values()
            .filter(|r| r.retry)
            .map(|r| r.id)
            .collect();
        ids.sort_by_key(|id| id.transfer);
        ids
    }
    /// Replace an answered expired transfer with a fresh identity in the same document.
    pub fn retry(&mut self, old: TransferId, new: TransferId) -> bool {
        if !self.matches(new)
            || self.paused
            || self.stopped
            || self.resources.contains_key(&new)
            || self.resources.get(&old).is_none_or(|r| !r.retry)
        {
            return false;
        }
        let Some(old) = self.resources.remove(&old) else {
            return false;
        };
        let mut fresh = Resource::fresh(new);
        fresh.metadata = old.metadata;
        fresh.allowed_size = old.allowed_size;
        self.resources.insert(new, fresh);
        true
    }
    pub fn stop(&mut self) -> Vec<HtmlRequest> {
        let ids: Vec<_> = self.resources.keys().copied().collect();
        let out = ids.into_iter().filter_map(|id| self.decline(id)).collect();
        self.stopped = true;
        self.paused = false;
        out
    }
    /// Close, convert and reconnect release all bodies; callers send returned cancels on the old transport.
    pub fn reset(&mut self, epoch: u64, binding: u64, generation: u64) -> Vec<HtmlRequest> {
        let cancel = self.stop();
        *self = Self::new(epoch, self.surface, binding, generation);
        cancel
    }
    pub fn receiving(&self) -> bool {
        self.paused
            || self.resources.values().any(|r| {
                matches!(
                    r.state,
                    ReceiveState::Metadata
                        | ReceiveState::SizeNotice
                        | ReceiveState::Ready
                        | ReceiveState::Receiving
                )
            })
    }
}

/// Empty capability lists preserve the old-peer placeholder behavior.
#[derive(Default)]
pub struct HtmlSession {
    pub server_transfer: bool,
    pub server_entry: bool,
    pub server_resources: bool,
    pub server_http: bool,
    pub server_https: bool,
    pub server_http_reload: bool,
    pub http_documents: crate::html_http_session::HttpDocuments,
    pub viewer_declared: bool,
    /// The current native renderer declaration, updated when its actual window is parked/restored.
    pub viewer_http_declared: bool,
    pub mirrors: HashMap<u32, Arc<std::sync::Mutex<crate::html_mirror::HtmlMirror>>>,
    epoch: u64,
    sequence: u64,
    pub documents: HashMap<u32, HtmlDocument>,
}
impl HtmlSession {
    pub fn negotiated(server_transfer: bool, server_entry: bool) -> Self {
        Self {
            server_transfer,
            server_entry,
            ..Default::default()
        }
    }
    pub fn entry_id(&mut self, surface_id: u32, binding: u64, document: u64) -> Option<TransferId> {
        static NEXT_EPOCH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        if self.epoch == 0 {
            self.epoch = NEXT_EPOCH
                .fetch_update(
                    std::sync::atomic::Ordering::AcqRel,
                    std::sync::atomic::Ordering::Acquire,
                    |v| v.checked_add(1),
                )
                .ok()?;
        }
        self.sequence = self.sequence.checked_add(1)?;
        Some(TransferId {
            epoch: self.epoch,
            surface_id,
            binding,
            document,
            request: self.sequence,
            transfer: self.sequence,
        })
    }
    pub fn from_descriptor(descriptor: &serde_json::Value) -> Self {
        Self {
            server_transfer: descriptor[tasty_ipc::stream::DESCRIPTOR_CAPABILITIES]
                .as_array()
                .is_some_and(|v| {
                    v.iter()
                        .any(|c| c.as_str() == Some(tasty_ipc::html_transfer::CAPABILITY))
                }),
            server_entry: crate::transport::descriptor_has_capability(
                descriptor,
                tasty_ipc::html_transfer::VIEWER_CAPABILITY,
            ),
            server_http: crate::transport::descriptor_has_capability(
                descriptor,
                tasty_ipc::html_http::CAPABILITY,
            ),
            server_http_reload: crate::transport::descriptor_has_capability(
                descriptor,
                tasty_ipc::html_http::RELOAD_CAPABILITY,
            ) && crate::transport::descriptor_has_capability(
                descriptor,
                tasty_ipc::html_http::CAPABILITY,
            ),
            server_https: crate::transport::descriptor_has_capability(
                descriptor,
                tasty_ipc::html_http::HTTPS_CAPABILITY,
            ),
            server_resources: crate::transport::descriptor_has_capability(
                descriptor,
                tasty_ipc::html_transfer::RESOURCE_CAPABILITY,
            ),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests;
