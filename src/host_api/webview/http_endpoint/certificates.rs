//! Fresh keys and exact leaf pins are reserved per document/authority, never a global trust store.
mod budget;
mod stream;
use crate::remote::html_http::local_tls::LocalTls;
pub(crate) use budget::CertificateBudget;
use rcgen::PublicKeyData;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
pub(crate) use stream::AuthorityStream;
use tokio::{runtime::Handle, sync::Notify};
use tokio_rustls::rustls::pki_types::PrivatePkcs8KeyDer;
#[derive(Clone, PartialEq, Eq, Hash)]
pub(crate) struct Authority {
    host: String,
    port: u16,
}
impl Authority {
    pub(crate) fn new(host: &str, port: u16) -> Result<Self, CertificateError> {
        let host = host
            .strip_prefix('[')
            .and_then(|h| h.strip_suffix(']'))
            .unwrap_or(host);
        if port == 0 || host.is_empty() || host.len() > 253 || host.contains('*') {
            return Err(CertificateError::Invalid);
        }
        let host = if let Ok(ip) = host.parse::<std::net::IpAddr>() {
            ip.to_string()
        } else {
            match url::Host::parse(host).map_err(|_| CertificateError::Invalid)? {
                url::Host::Domain(s) => s,
                url::Host::Ipv4(ip) => ip.to_string(),
                url::Host::Ipv6(ip) => ip.to_string(),
            }
        };
        if host.len() > 253 {
            return Err(CertificateError::Invalid);
        }
        Ok(Self { host, port })
    }
    pub(crate) fn host(&self) -> &str {
        &self.host
    }
    pub(crate) fn port(&self) -> u16 {
        self.port
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CertificateError {
    Invalid,
    Retired,
    Capacity,
    Worker,
    Tls,
}
pub(crate) struct AuthorityTls {
    tls: LocalTls,
    map: Weak<Certificates>,
    leaf: [u8; 32],
    spki: [u8; 32],
    _lease: Arc<budget::Lease>,
    #[cfg(test)]
    der: Vec<u8>,
}
impl AuthorityTls {
    pub(crate) fn leaf(&self) -> [u8; 32] {
        self.leaf
    }
    pub(crate) fn spki(&self) -> [u8; 32] {
        self.spki
    }
}
const CERTIFICATE_BYTES: usize = 16_384;
const MAX_AUTHORITIES: usize = 256;
const MAX_GENERATORS: usize = 8;
enum State {
    Pending,
    Ready(Arc<AuthorityTls>),
    Failed(CertificateError),
}
struct Slot {
    state: Mutex<State>,
    ready: Notify,
    lease: Arc<budget::Lease>,
}
struct Jobs {
    active: AtomicUsize,
    ready: Notify,
}
pub(crate) struct Certificates {
    current: Arc<dyn Fn() -> bool + Send + Sync>,
    live: AtomicBool,
    slots: Mutex<HashMap<Authority, Arc<Slot>>>,
    budget: CertificateBudget,
    jobs: Arc<Jobs>,
    runtime: Handle,
}
impl Certificates {
    /// Capture the actual native document/source/connection fence before any protocol request ID
    /// is allocated. The first browser request must allocate its ID only at actual ordered send.
    pub(crate) fn new(
        current: Arc<dyn Fn() -> bool + Send + Sync>,
        budget: CertificateBudget,
        runtime: Handle,
    ) -> Arc<Self> {
        Arc::new(Self {
            current,
            live: AtomicBool::new(true),
            slots: Mutex::new(HashMap::new()),
            budget,
            jobs: Arc::new(Jobs {
                active: AtomicUsize::new(0),
                ready: Notify::new(),
            }),
            runtime,
        })
    }
    pub(crate) fn current(&self) -> bool {
        self.live.load(Ordering::Acquire) && (self.current)()
    }
    pub(crate) fn expected_leaf(&self, host: &str, port: u16) -> Option<[u8; 32]> {
        if !self.current() {
            return None;
        }
        let authority = Authority::new(host, port).ok()?;
        let slots = match self.slots.lock() {
            Ok(slots) => slots,
            Err(_) => {
                self.live.store(false, Ordering::Release);
                tracing::error!("native certificate map poisoned; pins refused");
                return None;
            }
        };
        let slot = slots.get(&authority)?;
        let state = match slot.state.lock() {
            Ok(state) => state,
            Err(_) => {
                self.live.store(false, Ordering::Release);
                tracing::error!("native certificate slot poisoned; pins refused");
                return None;
            }
        };
        if !self.current() {
            return None;
        }
        if let State::Ready(tls) = &*state {
            Some(tls.leaf)
        } else {
            None
        }
    }
    pub(crate) async fn certificate(
        self: &Arc<Self>,
        authority: Authority,
    ) -> Result<Arc<AuthorityTls>, CertificateError> {
        if !self.current() {
            return Err(CertificateError::Retired);
        }
        let slot = self.reserve(authority)?;
        loop {
            let ready = slot.ready.notified();
            tokio::pin!(ready);
            ready.as_mut().enable();
            if !self.current() {
                return Err(CertificateError::Retired);
            }
            let result = match &*slot.state.lock().map_err(|_| CertificateError::Worker)? {
                State::Pending => None,
                State::Ready(tls) => Some(Ok(tls.clone())),
                State::Failed(error) => Some(Err(*error)),
            };
            if let Some(result) = result {
                return if self.current() {
                    result
                } else {
                    Err(CertificateError::Retired)
                };
            }
            ready.await;
        }
    }
    fn reserve(self: &Arc<Self>, authority: Authority) -> Result<Arc<Slot>, CertificateError> {
        let mut slots = self.slots.lock().map_err(|_| CertificateError::Worker)?;
        if !self.current() {
            return Err(CertificateError::Retired);
        }
        if let Some(slot) = slots.get(&authority) {
            return Ok(slot.clone());
        }
        if slots.len() >= MAX_AUTHORITIES {
            return Err(CertificateError::Capacity);
        }
        let lease = self.budget.reserve()?;
        let permit = self.budget.generator()?;
        let slot = Arc::new(Slot {
            state: Mutex::new(State::Pending),
            ready: Notify::new(),
            lease,
        });
        slots.insert(authority.clone(), slot.clone());
        let work = Generate {
            map: Arc::downgrade(self),
            slot: Some(slot.clone()),
            authority,
            permit: Some(permit),
            jobs: self.jobs.clone(),
            done: false,
        };
        self.jobs.active.fetch_add(1, Ordering::AcqRel);
        drop(slots);
        // The injected runtime bounds blocking workers; the permit lives until actual job Drop.
        // A dropped JoinHandle does not detach quota ownership from running key generation.
        std::mem::drop(self.runtime.spawn_blocking(move || work.run()));
        Ok(slot)
    }
    pub(crate) async fn settled(&self) {
        loop {
            let ready = self.jobs.ready.notified();
            tokio::pin!(ready);
            ready.as_mut().enable();
            if self.jobs.active.load(Ordering::Acquire) == 0 {
                return;
            }
            ready.await;
        }
    }
    pub(crate) fn retire(&self) {
        self.live.store(false, Ordering::Release);
        if let Ok(slots) = self.slots.lock() {
            for slot in slots.values() {
                if let Ok(mut state) = slot.state.lock() {
                    *state = State::Failed(CertificateError::Retired);
                } else {
                    tracing::error!("native certificate slot poisoned during retirement");
                }
                slot.ready.notify_waiters();
            }
        } else {
            tracing::error!("native certificate map poisoned during retirement");
        }
    }
}
struct Generate {
    map: Weak<Certificates>,
    slot: Option<Arc<Slot>>,
    authority: Authority,
    permit: Option<budget::Permit>,
    jobs: Arc<Jobs>,
    done: bool,
}
impl Generate {
    fn run(mut self) {
        let result = self.generate();
        let current = self.map.upgrade().is_some_and(|map| map.current());
        if let Ok(mut state) = self
            .slot
            .as_ref()
            .expect("owned certificate slot")
            .state
            .lock()
        {
            *state = if current {
                result.map(State::Ready).unwrap_or_else(State::Failed)
            } else {
                State::Failed(CertificateError::Retired)
            };
        } else {
            tracing::error!("native certificate generation result poisoned");
        }
        self.done = true;
        self.slot
            .as_ref()
            .expect("owned certificate slot")
            .ready
            .notify_waiters();
    }
    fn generate(&self) -> Result<Arc<AuthorityTls>, CertificateError> {
        if !self.map.upgrade().is_some_and(|map| map.current()) {
            return Err(CertificateError::Retired);
        }
        let rcgen::CertifiedKey { cert, signing_key } =
            rcgen::generate_simple_self_signed(vec![self.authority.host.clone()])
                .map_err(|_| CertificateError::Tls)?;
        let spki = Sha256::digest(signing_key.subject_public_key_info()).into();
        let key = PrivatePkcs8KeyDer::from(signing_key.serialize_der());
        let tls = LocalTls::new(vec![cert.der().clone()], key.into(), CERTIFICATE_BYTES)
            .map_err(|_| CertificateError::Tls)?;
        let leaf = *tls.leaf_sha256();
        Ok(Arc::new(AuthorityTls {
            tls,
            map: self.map.clone(),
            leaf,
            spki,
            _lease: self
                .slot
                .as_ref()
                .expect("owned certificate slot")
                .lease
                .clone(),
            #[cfg(test)]
            der: cert.der().to_vec(),
        }))
    }
}
impl Drop for Generate {
    fn drop(&mut self) {
        if let Some(slot) = self.slot.take() {
            if !self.done {
                if let Ok(mut state) = slot.state.lock() {
                    *state = State::Failed(CertificateError::Worker);
                } else {
                    tracing::error!("native certificate worker cancellation poisoned");
                }
                slot.ready.notify_waiters();
            }
            drop(slot);
        }
        drop(self.permit.take());
        self.jobs.active.fetch_sub(1, Ordering::AcqRel);
        self.jobs.ready.notify_waiters();
    }
}
#[cfg(test)]
mod tests;
