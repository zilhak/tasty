//! Finite HTTP receive state, encoded consent and worker handoff, independent of host/native I/O.
//! The caller sends each returned command exactly once in shared allocation order. Enqueue failure
//! closes this document. Source/grant/native publication fencing remains the caller's responsibility.
mod budget;
mod flow;
mod lifecycle;
mod receive;
mod worker;
use crate::{
    connection::ConnectionEpoch,
    html_http_body::{BodyBudget, VerifiedBody},
};
use budget::Lease;
pub use budget::PendingBudget;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use tasty_ipc::{
    html_http::{HttpReply, HttpRequest, ResponseHead, ResponseId},
    html_transfer::TransferId,
};
pub use worker::VerifyJob;

/// Captured authority, never constructed from an incoming reply.
pub struct DocumentKey {
    pub entry: TransferId,
    pub source_generation: u64,
    pub entry_url: String,
    pub allow_remote_content: bool,
}
/// Finite injected caps; this model has no default or network/runtime configuration.
#[derive(Clone, Copy)]
pub struct ClientLimits {
    pub requests: usize,
    pub upload_bytes: usize,
    pub encoded_bytes: usize,
    pub decoded_bytes: usize,
    pub verify_jobs: usize,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientError {
    Invalid,
    Closed,
    Stale,
    Quota,
    AlreadyUsed,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiveState {
    Uploading,
    WaitingHead,
    SizeNotice,
    Ready,
    Receiving,
    Verifying,
    Verified,
    Reusing,
    Cancelling,
    Skipped,
    Failed,
    Cancelled,
    Released,
}
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub received: u64,
    pub reserved: u64,
    pub paused: bool,
    pub receiving: bool,
    pub skipped: usize,
    pub notices: Vec<TransferId>,
}
struct RequestOwner {
    begin: HttpRequest,
    _lease: Lease,
}
struct Upload {
    bytes: Arc<Vec<u8>>,
    offset: usize,
    window: Option<usize>,
    _lease: Lease,
}
struct Credit {
    max: usize,
    reserved: u64,
}
struct Attempt {
    id: TransferId,
    request: Option<Arc<RequestOwner>>,
    state: ReceiveState,
    upload: Option<Upload>,
    head: Option<ResponseHead>,
    head_lease: Option<Lease>,
    body: Vec<u8>,
    body_lease: Option<Lease>,
    offset: u64,
    credit: Option<Credit>,
    allowed: bool,
    committed: bool,
    verified: Option<Arc<VerifiedBody>>,
    replay_head: bool,
    completion: Option<(
        tasty_ipc::html_http::BodyIdentity,
        tasty_ipc::html_http::BodyIdentity,
    )>,
    owner: Option<Arc<()>>,
    cancel: Arc<AtomicBool>,
    job: Option<VerifyJob>,
    result: Option<mpsc::Receiver<worker::ResultBody>>,
}
impl Attempt {
    fn id(&self) -> TransferId {
        self.id
    }
    fn response(&self) -> Option<ResponseId> {
        self.head.as_ref().map(|h| h.id)
    }
    fn terminal(&self) -> bool {
        matches!(
            self.state,
            ReceiveState::Failed | ReceiveState::Cancelled | ReceiveState::Released
        ) || (self.state == ReceiveState::Skipped && self.credit.is_none())
    }
    fn settle(&mut self) -> u64 {
        self.credit.take().map_or(0, |c| c.reserved)
    }
    fn discard(&mut self) {
        self.cancel.store(true, Ordering::Release);
        self.owner = None;
        self.result = None;
        self.job = None;
        self.upload = None;
        self.body = Vec::new();
        self.body_lease = None;
        self.head_lease = None;
        self.head = None;
        self.verified = None;
        self.request = None;
    }
}
/// One uniquely owned document generation. Closing it never retires a shared ConnectionEpoch.
pub struct ClientDocument {
    key: DocumentKey,
    epoch: ConnectionEpoch,
    limits: ClientLimits,
    pending: PendingBudget,
    verified_budget: BodyBudget,
    slots: PendingBudget,
    wake: Arc<dyn Fn() + Send + Sync>,
    attempts: Vec<Attempt>,
    _storage: Lease,
    last_request: u64,
    last_transfer: u64,
    received: u64,
    reserved: u64,
    continued: bool,
    paused: bool,
    stopped: bool,
    closed: bool,
    skipped: usize,
}
impl ClientDocument {
    pub fn new(
        key: DocumentKey,
        epoch: ConnectionEpoch,
        limits: ClientLimits,
        pending: PendingBudget,
        verified: BodyBudget,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Self, ClientError> {
        if [
            limits.requests,
            limits.upload_bytes,
            limits.encoded_bytes,
            limits.decoded_bytes,
            limits.verify_jobs,
        ]
        .contains(&0)
            || key.source_generation == 0
            || key.entry_url.is_empty()
        {
            return Err(ClientError::Invalid);
        }
        if !epoch.is_active() {
            return Err(ClientError::Closed);
        }
        let capacity = limits
            .requests
            .checked_mul(std::mem::size_of::<Attempt>())
            .ok_or(ClientError::Quota)?;
        let mut storage = pending.reserve(
            capacity
                .checked_add(key.entry_url.capacity())
                .ok_or(ClientError::Quota)?,
        )?;
        let mut attempts = Vec::new();
        attempts
            .try_reserve_exact(limits.requests)
            .map_err(|_| ClientError::Quota)?;
        if attempts.capacity() > limits.requests {
            storage
                .grow((attempts.capacity() - limits.requests) * std::mem::size_of::<Attempt>())?;
        }
        Ok(Self {
            key,
            epoch,
            limits,
            pending,
            verified_budget: verified,
            slots: PendingBudget::new(limits.verify_jobs),
            wake,
            attempts,
            _storage: storage,
            last_request: 0,
            last_transfer: 0,
            received: 0,
            reserved: 0,
            continued: false,
            paused: false,
            stopped: false,
            closed: false,
            skipped: 0,
        })
    }
    fn active(&self) -> bool {
        !self.closed && self.epoch.is_active()
    }
    fn document_matches(&self, id: TransferId) -> bool {
        let k = self.key.entry;
        (id.epoch, id.surface_id, id.binding, id.document)
            == (k.epoch, k.surface_id, k.binding, k.document)
    }
    /// Returns Begin immediately. Caller sends it before allocating the next shared identity.
    /// Failed admission advances neither freshness cursor; no command allocates an identity here.
    pub fn admit(
        &mut self,
        begin: HttpRequest,
        upload: Arc<Vec<u8>>,
    ) -> Result<HttpRequest, ClientError> {
        use sha2::{Digest, Sha256};
        if !self.active() || self.stopped {
            return Err(ClientError::Closed);
        }
        begin.validate().map_err(|_| ClientError::Invalid)?;
        let (begin, previous) = begin.into_initial();
        if previous.is_some() && !self.attempts.is_empty() {
            return Err(ClientError::Invalid);
        }
        let HttpRequest::Begin {
            id,
            source_generation,
            method,
            logical_url,
            headers,
            upload: identity,
            allow_remote_content,
        } = &begin
        else {
            return Err(ClientError::Invalid);
        };
        if !self.document_matches(*id) || *source_generation != self.key.source_generation {
            return Err(ClientError::Stale);
        }
        if *allow_remote_content != self.key.allow_remote_content
            || (self.attempts.is_empty()
                && (*id != self.key.entry || *logical_url != self.key.entry_url))
        {
            return Err(ClientError::Invalid);
        }
        if id.request <= self.last_request || id.transfer <= self.last_transfer {
            return Err(ClientError::AlreadyUsed);
        }
        if self.attempts.len() == self.limits.requests || upload.len() > self.limits.upload_bytes {
            return Err(ClientError::Quota);
        }
        if identity.bytes != upload.len() as u64
            || identity.sha256 != <[u8; 32]>::from(Sha256::digest(&*upload))
        {
            return Err(ClientError::Invalid);
        }
        let upload_lease = self.pending.reserve(upload.capacity())?;
        // Clone only the retained request metadata, measuring the resulting capacities as well.
        let weight = headers
            .iter()
            .try_fold(
                logical_url
                    .capacity()
                    .checked_add(method.capacity())
                    .ok_or(ClientError::Quota)?
                    .checked_add(
                        headers
                            .capacity()
                            .checked_mul(std::mem::size_of::<tasty_ipc::html_http::Header>())
                            .ok_or(ClientError::Quota)?,
                    )
                    .ok_or(ClientError::Quota)?,
                |n, h| {
                    n.checked_add(h.name.capacity())?
                        .checked_add(h.value.capacity())
                },
            )
            .ok_or(ClientError::Quota)?;
        let mut lease = self.pending.reserve(weight)?;
        let stored = begin.clone();
        let actual = request_bytes(&stored);
        lease.retain(actual)?;
        self.last_request = id.request;
        self.last_transfer = id.transfer;
        self.attempts.push(Attempt {
            id: *id,
            request: Some(Arc::new(RequestOwner {
                begin: stored,
                _lease: lease,
            })),
            state: ReceiveState::Uploading,
            upload: Some(Upload {
                bytes: upload,
                offset: 0,
                window: None,
                _lease: upload_lease,
            }),
            head: None,
            head_lease: None,
            body: Vec::new(),
            body_lease: None,
            offset: 0,
            credit: None,
            allowed: false,
            committed: false,
            verified: None,
            replay_head: false,
            completion: None,
            owner: Some(Arc::new(())),
            cancel: Arc::new(AtomicBool::new(false)),
            job: None,
            result: None,
        });
        match previous {
            Some(previous) => begin
                .with_predecessor(previous)
                .map_err(|_| ClientError::Invalid),
            None => Ok(begin),
        }
    }
    pub fn receive(&mut self, reply: HttpReply) -> bool {
        self.receive_inner(reply)
    }
    pub fn state(&self, id: TransferId) -> Option<ReceiveState> {
        self.attempts.iter().find(|a| a.id() == id).map(|a| a.state)
    }
    pub fn verified(&self, id: TransferId) -> Option<Arc<VerifiedBody>> {
        if !self.active() {
            return None;
        }
        self.attempts
            .iter()
            .find(|a| a.id() == id && a.state == ReceiveState::Verified)?
            .verified
            .clone()
    }
    fn fail(&mut self, index: usize) {
        let a = &mut self.attempts[index];
        self.reserved -= a.settle();
        a.discard();
        a.state = ReceiveState::Failed;
    }
}
fn request_bytes(request: &HttpRequest) -> usize {
    let HttpRequest::Begin {
        method,
        logical_url,
        headers,
        ..
    } = request
    else {
        return 0;
    };
    headers.iter().fold(
        method
            .capacity()
            .saturating_add(logical_url.capacity())
            .saturating_add(
                headers
                    .capacity()
                    .saturating_mul(std::mem::size_of::<tasty_ipc::html_http::Header>()),
            ),
        |n, h| {
            n.saturating_add(h.name.capacity())
                .saturating_add(h.value.capacity())
        },
    )
}
impl Drop for ClientDocument {
    fn drop(&mut self) {
        for a in &mut self.attempts {
            a.discard();
        }
    }
}
#[cfg(test)]
mod tests;

#[cfg(test)]
mod boundary_tests;

#[cfg(test)]
mod worker_tests;
