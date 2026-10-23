//! Portable finite HTTP executor, independent of native WebViews and stream dispatch.
//! The caller validates attach ownership and owns one document generation. Reusing a response
//! never opens a second upstream request. HTTPS/upgrade handling belongs to a later adapter.
mod body;
pub mod endpoint;
mod headers;
mod transfer;
mod types;
pub use body::{Body, Upload, UploadBuilder};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};
use tasty_ipc::html_transfer::TransferId;
use tokio::sync::{Semaphore, mpsc, watch};
pub use types::*;

pub struct Request {
    pub id: TransferId,
    pub source_generation: u64,
    pub method: String,
    pub url: String,
    pub headers: Vec<Header>,
    pub upload: Upload,
}
#[derive(Debug)]
pub struct StoredResponse {
    pub head: ResponseHead,
    pub encoded: Body,
    pub decoded: Body,
}
#[derive(Clone)]
pub struct Backend {
    inner: Arc<BackendInner>,
}
struct BackendInner {
    client: reqwest::Client,
    limits: Limits,
    quota: Arc<body::Quota>,
    slots: Arc<Semaphore>,
    serial: AtomicU64,
    runtime: tokio::runtime::Handle,
}
impl Backend {
    pub fn new(limits: Limits, runtime: tokio::runtime::Handle) -> Result<Self> {
        limits.validate()?;
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .no_zstd()
            .http1_only()
            .pool_max_idle_per_host(0)
            .connect_timeout(limits.connect_timeout)
            .read_timeout(limits.read_timeout)
            .build()
            .map_err(|_| Error::Network)?;
        Ok(Self {
            inner: Arc::new(BackendInner {
                client,
                quota: Arc::new(body::Quota {
                    used: 0.into(),
                    limit: limits.retained_bytes,
                }),
                slots: Arc::new(Semaphore::new(limits.workers)),
                limits,
                serial: 1.into(),
                runtime,
            }),
        })
    }
    pub fn upload(&self) -> UploadBuilder {
        UploadBuilder::new(self.inner.quota.clone(), self.inner.limits.upload_bytes)
    }
    pub fn retained_payload_bytes(&self) -> usize {
        self.inner.quota.used.load(Ordering::Acquire)
    }
    pub fn available_workers(&self) -> usize {
        self.inner.slots.available_permits()
    }
    /// Entry origin is trusted surface metadata, not an unvalidated request parameter.
    pub fn document(
        &self,
        key: DocumentKey,
        entry_url: &str,
        allow_cross_origin: bool,
    ) -> Result<Document> {
        let origin = parse_http(entry_url)?.origin().ascii_serialization();
        Ok(Document {
            inner: Arc::new(DocumentInner {
                backend: self.inner.clone(),
                key,
                origin,
                allow_cross_origin,
                closed: AtomicBool::new(false),
                poisoned: AtomicBool::new(false),
                entries: Mutex::new(HashMap::new()),
            }),
        })
    }
}
fn parse_http(url: &str) -> Result<reqwest::Url> {
    let mut url = reqwest::Url::parse(url).map_err(|_| Error::Invalid("absolute HTTP URL"))?;
    if url.scheme() != "http" || url.host_str().is_none() {
        return Err(Error::UnsupportedScheme);
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(Error::Invalid(
            "URL userinfo; use native Authorization header",
        ));
    }
    url.set_fragment(None);
    Ok(url)
}
struct DocumentInner {
    backend: Arc<BackendInner>,
    key: DocumentKey,
    origin: String,
    allow_cross_origin: bool,
    closed: AtomicBool,
    poisoned: AtomicBool,
    entries: Mutex<HashMap<TransferId, Entry>>,
}
struct Entry {
    fingerprint: [u8; 32],
    state: State,
}
enum State {
    Pending(Cancellation),
    Ready(Arc<StoredResponse>),
    Retired,
}
impl DocumentInner {
    fn entries(&self) -> std::sync::MutexGuard<'_, HashMap<TransferId, Entry>> {
        match self.entries.lock() {
            Ok(entries) => entries,
            Err(poison) => {
                if !self.poisoned.swap(true, Ordering::AcqRel) {
                    tracing::error!("HTTP document registry poisoned; closing its operations");
                }
                self.closed.store(true, Ordering::Release);
                let mut entries = poison.into_inner();
                for entry in entries.values_mut() {
                    if let State::Pending(c) = &entry.state {
                        c.cancel()
                    } else {
                        entry.state = State::Retired;
                    }
                }
                entries
            }
        }
    }
}
#[derive(Clone)]
struct Cancellation {
    tx: watch::Sender<bool>,
    flag: Arc<AtomicBool>,
}
impl Cancellation {
    fn new() -> Self {
        let (tx, _) = watch::channel(false);
        Self {
            tx,
            flag: Arc::new(AtomicBool::new(false)),
        }
    }
    fn cancel(&self) {
        self.flag.store(true, Ordering::Release);
        self.tx.send_replace(true);
    }
}
/// Single owner; dropping a document cancels its workers without blocking the main loop.
pub struct Document {
    inner: Arc<DocumentInner>,
}
pub enum Opened {
    Fresh(Operation),
    Reused(Arc<StoredResponse>),
}
impl Document {
    pub fn open(&self, request: Request) -> Result<Opened> {
        use sha2::{Digest, Sha256};
        if self.inner.closed.load(Ordering::Acquire) {
            return Err(Error::Closed);
        }
        if !self.inner.key.matches(request.id)
            || self.inner.key.source_generation != request.source_generation
        {
            return Err(Error::Stale);
        }
        if !request.upload.belongs_to(&self.inner.backend.quota) {
            return Err(Error::Invalid("upload backend ownership"));
        }
        let url = parse_http(&request.url)?;
        if !self.inner.allow_cross_origin && url.origin().ascii_serialization() != self.inner.origin
        {
            return Err(Error::Invalid("cross-origin policy"));
        }
        let method = reqwest::Method::from_bytes(request.method.as_bytes())
            .map_err(|_| Error::Invalid("HTTP method"))?;
        if method == reqwest::Method::CONNECT
            || request
                .headers
                .iter()
                .any(|h| h.name.eq_ignore_ascii_case("upgrade"))
        {
            return Err(Error::UnsupportedUpgrade);
        }
        let clean = headers::clean(
            &request.headers,
            self.inner.backend.limits.header_bytes,
            true,
        )?;
        let mut digest = Sha256::new();
        for bytes in [
            method.as_str().as_bytes(),
            url.as_str().as_bytes(),
            request.upload.identity().sha256.as_slice(),
        ] {
            digest.update(bytes.len().to_le_bytes());
            digest.update(bytes);
        }
        digest.update(request.upload.identity().bytes.to_le_bytes());
        for h in &clean {
            digest.update(h.name.len().to_le_bytes());
            digest.update(h.name.to_ascii_lowercase().as_bytes());
            digest.update(h.value.len().to_le_bytes());
            digest.update(&h.value);
        }
        let fingerprint = digest.finalize().into();
        let mut entries = self.inner.entries();
        if self.inner.closed.load(Ordering::Acquire) {
            return Err(Error::Closed);
        }
        if let Some(entry) = entries.get(&request.id) {
            if entry.fingerprint != fingerprint {
                return Err(Error::DuplicateMismatch);
            }
            return match &entry.state {
                State::Pending(_) => Err(Error::InProgress),
                State::Ready(r) => Ok(Opened::Reused(r.clone())),
                State::Retired => Err(Error::AlreadyUsed),
            };
        }
        if entries.len() >= self.inner.backend.limits.requests_per_document {
            return Err(Error::Capacity);
        }
        let permit = self
            .inner
            .backend
            .slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Capacity)?;
        let serial = self
            .inner
            .backend
            .serial
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_add(1))
            .map_err(|_| Error::Capacity)?;
        let id = ResponseId {
            request: request.id,
            source_generation: request.source_generation,
            serial,
        };
        let cancel = Cancellation::new();
        entries.insert(
            request.id,
            Entry {
                fingerprint,
                state: State::Pending(cancel.clone()),
            },
        );
        drop(entries);
        let (credit_tx, credits) = mpsc::channel(1);
        let (events, event_rx) = mpsc::channel(1);
        let inner = self.inner.clone();
        let worker_cancel = cancel.clone();
        let join = self.inner.backend.runtime.spawn(async move {
            let _permit = permit;
            let mut result = transfer::run(
                &inner,
                id,
                method,
                url,
                clean,
                request.upload,
                credits,
                &events,
                &worker_cancel,
            )
            .await;
            if worker_cancel.flag.load(Ordering::Acquire) {
                result = Err(Error::Cancelled);
            }
            {
                let mut entries = inner.entries();
                if let Some(entry) = entries.get_mut(&id.request) {
                    entry.state = match &result {
                        Ok(r) if !inner.closed.load(Ordering::Acquire) => State::Ready(r.clone()),
                        _ => State::Retired,
                    };
                }
            }
            let event = match &result {
                Ok(r) => Event::Complete(r.clone()),
                Err(error) => Event::Failed {
                    id,
                    error: error.clone(),
                },
            };
            if transfer::send(&events, event, &worker_cancel)
                .await
                .is_err()
            {
                result = Err(Error::Cancelled);
                {
                    let mut entries = inner.entries();
                    if let Some(entry) = entries.get_mut(&id.request) {
                        entry.state = State::Retired;
                    }
                }
            }
            result
        });
        Ok(Opened::Fresh(Operation {
            events: event_rx,
            credits: credit_tx,
            cancel,
            join: Some(join),
        }))
    }
    pub fn reuse(&self, id: ResponseId) -> Result<Arc<StoredResponse>> {
        if self.inner.closed.load(Ordering::Acquire) {
            return Err(Error::Closed);
        }
        let entries = self.inner.entries();
        match entries.get(&id.request).map(|e| &e.state) {
            Some(State::Ready(r)) if r.head.id == id => Ok(r.clone()),
            _ => Err(Error::Stale),
        }
    }
    pub fn release(&self, id: ResponseId) -> Result<()> {
        let mut entries = self.inner.entries();
        if let Some(e) = entries.get_mut(&id.request)
            && matches!(&e.state,State::Ready(r)if r.head.id==id)
        {
            e.state = State::Retired;
            Ok(())
        } else {
            Err(Error::Stale)
        }
    }
    pub fn cancel(&self, id: TransferId) -> Result<()> {
        if !self.inner.key.matches(id) {
            return Err(Error::Stale);
        }
        let mut entries = self.inner.entries();
        if let Some(e) = entries.get_mut(&id) {
            if let State::Pending(c) = &e.state {
                c.cancel()
            } else {
                e.state = State::Retired;
            }
            Ok(())
        } else {
            Err(Error::Stale)
        }
    }
    pub fn close(&self) {
        self.inner.closed.store(true, Ordering::Release);
        {
            let mut entries = self.inner.entries();
            for e in entries.values_mut() {
                if let State::Pending(c) = &e.state {
                    c.cancel()
                } else {
                    e.state = State::Retired;
                }
            }
        }
    }
}
impl Drop for Document {
    fn drop(&mut self) {
        self.close();
    }
}
pub struct Operation {
    pub events: mpsc::Receiver<Event>,
    credits: mpsc::Sender<Credit>,
    cancel: Cancellation,
    join: Option<tokio::task::JoinHandle<Result<Arc<StoredResponse>>>>,
}
impl Operation {
    pub fn credit(&self, credit: Credit) -> Result<()> {
        self.credits
            .try_send(credit)
            .map_err(|_| Error::Invalid("credit queue or closed response"))
    }
    pub fn cancel(&self) {
        self.cancel.cancel();
    }
    pub fn is_finished(&self) -> bool {
        self.join.as_ref().is_none_or(|j| j.is_finished())
    }
    /// Drain events through Complete/Failed, or cancel first, before joining.
    /// Cancelling this future still drops Operation and cancels the worker.
    pub async fn join(mut self) -> Result<Arc<StoredResponse>> {
        let result = self
            .join
            .as_mut()
            .ok_or(Error::Worker)?
            .await
            .map_err(|_| Error::Worker)?;
        drop(self.join.take());
        result
    }
}
impl Drop for Operation {
    fn drop(&mut self) {
        if self.join.is_some() {
            self.cancel.cancel();
        }
    }
}
#[cfg(test)]
mod tests;
