//! Host-local HTTP contracts. A common wire owner serializes these separately.
use std::time::Duration;
use tasty_ipc::html_transfer::TransferId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Closed,
    Stale,
    Capacity,
    InProgress,
    AlreadyUsed,
    DuplicateMismatch,
    Invalid(&'static str),
    Quota(&'static str),
    Network,
    Timeout,
    Cancelled,
    UnsupportedScheme,
    UnsupportedUpgrade,
    UnsupportedStreaming,
    UnsupportedEncoding,
    Worker,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "HTTP executor: {self:?}")
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;

/// Resource quotas are independent of the document's 100KB/2MB consent decisions.
#[derive(Debug, Clone)]
pub struct Limits {
    pub workers: usize,
    pub requests_per_document: usize,
    /// Charged payload bytes, excluding bounded transport/frame/allocator overhead.
    pub retained_bytes: usize,
    pub upload_bytes: usize,
    pub encoded_bytes: usize,
    pub decoded_bytes: usize,
    pub header_bytes: usize,
    pub connect_timeout: Duration,
    pub read_timeout: Duration,
    /// Cumulative network await time, excluding consumer credit/consent pauses.
    pub network_time: Duration,
}
impl Limits {
    pub(super) fn validate(&self) -> Result<()> {
        if self.workers == 0
            || self.workers > tokio::sync::Semaphore::MAX_PERMITS
            || self.requests_per_document == 0
            || self.retained_bytes == 0
            || self.header_bytes == 0
            || self.connect_timeout.is_zero()
            || self.read_timeout.is_zero()
            || self.network_time.is_zero()
            || self.upload_bytes > self.retained_bytes
            || self.encoded_bytes > self.retained_bytes
            || self.decoded_bytes > self.retained_bytes
        {
            return Err(Error::Invalid("resource limits"));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocumentKey {
    pub epoch: u64,
    pub surface_id: u32,
    pub binding: u64,
    pub document: u64,
    pub source_generation: u64,
}
impl DocumentKey {
    pub fn matches(&self, id: TransferId) -> bool {
        id.epoch == self.epoch
            && id.surface_id == self.surface_id
            && id.binding == self.binding
            && id.document == self.document
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResponseId {
    pub request: TransferId,
    pub source_generation: u64,
    pub serial: u64,
}
#[derive(Clone, PartialEq, Eq)]
pub struct Header {
    pub name: String,
    pub value: Vec<u8>,
}
impl std::fmt::Debug for Header {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Header")
            .field("name", &self.name)
            .field("value_bytes", &self.value.len())
            .finish()
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyIdentity {
    pub bytes: usize,
    pub sha256: [u8; 32],
}
#[derive(Debug, Clone)]
pub struct ResponseHead {
    pub id: ResponseId,
    pub logical_url: String,
    pub status: u16,
    pub headers: Vec<Header>,
    pub encoded_length: Option<u64>,
    pub has_body: bool,
}
#[derive(Debug, Clone, Copy)]
pub struct Credit {
    pub response: ResponseId,
    pub offset: u64,
    pub max_bytes: usize,
    pub allow_large: bool,
}
#[derive(Debug)]
pub enum Event {
    Head(ResponseHead),
    AllowanceRequired {
        id: ResponseId,
        encoded_at_least: u64,
        known_length: Option<u64>,
    },
    Chunk {
        id: ResponseId,
        offset: u64,
        bytes: Vec<u8>,
    },
    Complete(std::sync::Arc<super::StoredResponse>),
    Failed {
        id: ResponseId,
        error: Error,
    },
}
