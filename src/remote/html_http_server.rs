//! Bounded encoded HTTP bridge. Actual attach authority and ordered ID admission are host hooks.
mod commands;
mod convert;
mod fingerprint;
mod poll;
use super::html_http as core;
use convert::*;
pub use convert::{failure_reply, rejection_reply};
use std::{collections::HashMap, sync::Arc};
use tasty_ipc::{
    html_http as wire,
    html_transfer::TransferId,
    stream::{FRAME_HEADER_LEN, MAX_FRAME_LEN, StreamFrame},
};

/// Opaque server-local identity, minted only from authoritative engine capture.
#[derive(Clone)]
pub struct OwnerToken(Arc<Capture>);
/// Trusted engine metadata, never deserialized from an HTTP command.
pub struct Capture {
    pub entry: TransferId,
    pub source_generation: u64,
    pub entry_url: String,
    pub allow_remote_content: bool,
}
impl OwnerToken {
    pub fn entry(&self) -> TransferId {
        self.0.entry
    }
    pub fn source_generation(&self) -> u64 {
        self.0.source_generation
    }
}
pub struct Server {
    backend: core::Backend,
    documents: Vec<Owner>,
    max_documents: usize,
    max_requests: usize,
    max_upload: usize,
    header_limit: usize,
}
struct Owner {
    token: OwnerToken,
    document: core::Document,
    requests: HashMap<TransferId, Slot>,
    stopped: bool,
    transferred: u64,
    cursor: usize,
}
struct Slot {
    begin: wire::HttpRequest,
    fingerprint: [u8; 32],
    state: State,
}
enum State {
    Upload {
        builder: core::UploadBuilder,
        offset: u64,
        ready: bool,
    },
    Running(Running),
    Replay {
        response: Arc<core::StoredResponse>,
        head: bool,
    },
    Complete(wire::ResponseId),
    Terminal(Option<wire::HttpReply>),
}
struct Running {
    operation: core::Operation,
    response: Option<wire::ResponseId>,
    offset: u64,
    outstanding: Option<u32>,
    allowed: bool,
    known_length: Option<u64>,
}
impl Server {
    /// Backend must be built on a setup worker. Pass the very same limits used to build it.
    pub fn new(
        backend: core::Backend,
        max_documents: usize,
        limits: &core::Limits,
    ) -> core::Result<Self> {
        if max_documents == 0 || limits.requests_per_document == 0 {
            return Err(core::Error::Invalid("bridge limits"));
        }
        Ok(Self {
            backend,
            documents: Vec::new(),
            max_documents,
            max_requests: limits.requests_per_document,
            max_upload: limits.upload_bytes,
            header_limit: limits.header_bytes,
        })
    }
    /// Call after actual grant/source/capability validation. Shared ID admission precedes Begin.
    pub fn admit(&mut self, capture: Capture) -> core::Result<OwnerToken> {
        if self.documents.len() >= self.max_documents {
            return Err(core::Error::Capacity);
        }
        if capture.entry_url.len() > wire::URL_BYTES {
            return Err(core::Error::Invalid("entry URL bounds"));
        }
        let id = capture.entry;
        let document = self.backend.document(
            core::DocumentKey {
                epoch: id.epoch,
                surface_id: id.surface_id,
                binding: id.binding,
                document: id.document,
                source_generation: capture.source_generation,
            },
            &capture.entry_url,
            capture.allow_remote_content,
        )?;
        let token = OwnerToken(Arc::new(capture));
        self.documents.push(Owner {
            token: token.clone(),
            document,
            requests: HashMap::new(),
            stopped: false,
            transferred: 0,
            cursor: 0,
        });
        Ok(token)
    }
    fn index(&self, token: &OwnerToken) -> core::Result<usize> {
        self.documents
            .iter()
            .position(|o| Arc::ptr_eq(&o.token.0, &token.0))
            .ok_or(core::Error::Stale)
    }
    fn current(
        &mut self,
        token: &OwnerToken,
        valid: impl FnOnce(&OwnerToken) -> bool,
    ) -> core::Result<usize> {
        let i = self.index(token)?;
        if !valid(token) {
            self.retire(token);
            return Err(core::Error::Stale);
        }
        Ok(i)
    }
    /// Retire on detach/source replacement/reload/close; never wait for worker joins here.
    pub fn retire(&mut self, token: &OwnerToken) {
        if let Ok(i) = self.index(token) {
            self.documents.swap_remove(i).document.close();
        }
    }
    pub fn transferred_bytes(&self, token: &OwnerToken) -> core::Result<u64> {
        Ok(self.documents[self.index(token)?].transferred)
    }
    /// One bounded frame, with no transport-owned queue. Reserve a full frame in the caller's
    /// existing connection byte budget first; charge actual payload+framing bytes after return.
    pub fn poll(
        &mut self,
        token: &OwnerToken,
        valid: impl FnOnce(&OwnerToken) -> bool,
        available_bytes: usize,
    ) -> core::Result<Option<StreamFrame>> {
        let i = self.current(token, valid)?;
        if available_bytes < (MAX_FRAME_LEN as usize).saturating_add(FRAME_HEADER_LEN) {
            return Ok(None);
        }
        poll::owner(&mut self.documents[i])
    }
}
impl Owner {
    fn matches(&self, id: TransferId, generation: u64) -> bool {
        let e = self.token.entry();
        (id.epoch, id.surface_id, id.binding, id.document, generation)
            == (
                e.epoch,
                e.surface_id,
                e.binding,
                e.document,
                self.token.source_generation(),
            )
    }
}
#[cfg(test)]
mod tests;
