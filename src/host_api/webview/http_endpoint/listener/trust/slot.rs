use super::*;
use std::sync::Weak;
use tokio::{sync::Notify, time::Instant};
pub(crate) struct TrustObservation {
    pub host: String,
    pub port: u16,
    pub leaf: [u8; 32],
    pub unknown_ca_only: bool,
}
pub(super) struct Slot {
    pub authority: Authority,
    pub certificate: Arc<AuthorityTls>,
    pub token: EndpointToken,
    pub deadline: Instant,
    pub owner: Weak<State>,
    gate: Weak<Gate>,
    proxy: String,
    live: AtomicBool,
    verdict: Mutex<Option<bool>>,
    pub flight: AtomicBool,
    pub finished: AtomicBool,
    pub stopped: AtomicBool,
    pub failed: AtomicBool,
    pub changed: Notify,
    _permit: tokio::sync::OwnedSemaphorePermit,
}
impl Slot {
    pub(super) fn new(
        gate: &Arc<Gate>,
        owner: &Arc<State>,
        authority: Authority,
        certificate: Arc<AuthorityTls>,
        port: u16,
        timeout: Duration,
        permit: tokio::sync::OwnedSemaphorePermit,
    ) -> Result<Arc<Self>, EndpointError> {
        let mut entropy = [0; 32];
        use rand::TryRngCore;
        rand::rngs::OsRng
            .try_fill_bytes(&mut entropy)
            .map_err(|_| EndpointError::Worker)?;
        let token = EndpointToken::from_random_bytes(entropy);
        let proxy = token.proxy_uri(port);
        Ok(Arc::new(Self {
            authority,
            certificate,
            token,
            proxy,
            deadline: Instant::now() + timeout,
            owner: Arc::downgrade(owner),
            gate: Arc::downgrade(gate),
            live: AtomicBool::new(true),
            verdict: Mutex::new(None),
            flight: AtomicBool::new(false),
            finished: AtomicBool::new(false),
            stopped: AtomicBool::new(false),
            failed: AtomicBool::new(false),
            changed: Notify::new(),
            _permit: permit,
        }))
    }
    pub(super) fn current(&self) -> bool {
        self.live.load(Ordering::Acquire)
            && Instant::now() < self.deadline
            && self.gate.upgrade().is_some_and(|g| g.current())
            && self.owner.upgrade().is_some_and(|s| (s.current)())
    }
    pub(super) fn cancel(&self) {
        self.live.store(false, Ordering::Release);
        self.changed.notify_waiters();
    }
    pub(super) fn verdict(&self) -> Option<bool> {
        match self.verdict.lock() {
            Ok(value) => *value,
            Err(_) => {
                tracing::error!("native trust verdict poisoned; refused");
                self.cancel();
                Some(false)
            }
        }
    }
    pub(super) async fn wait(&self) -> bool {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if !self.current() {
                return false;
            }
            match self.verdict() {
                Some(false) => return false,
                Some(true) if self.finished.load(Ordering::Acquire) => return true,
                _ => {}
            }
            // Source predicates are memory-only and may retire without an explicit wake.
            tokio::select! { _=changed=>{}, _=tokio::time::sleep(Duration::from_millis(10))=>{} }
        }
    }
    pub(super) async fn stopped(&self) {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if self.stopped.load(Ordering::Acquire) {
                return;
            }
            changed.await;
        }
    }
}
pub(super) struct Attempt(pub Arc<Slot>);
impl Drop for Attempt {
    fn drop(&mut self) {
        self.0.cancel();
    }
}
/// Only the native inbox can mint this capability; it has no wire/page representation.
pub(crate) struct TrustRequest(pub(super) Arc<Slot>);
impl TrustRequest {
    pub(super) fn new(slot: Arc<Slot>) -> Self {
        Self(slot)
    }
    pub(crate) fn identity(&self) -> Option<DocumentIdentity> {
        self.0.owner.upgrade().map(|s| s.config.identity)
    }
    pub(crate) fn host(&self) -> &str {
        self.0.authority.host()
    }
    pub(crate) fn port(&self) -> u16 {
        self.0.authority.port()
    }
    pub(crate) fn leaf(&self) -> [u8; 32] {
        self.0.certificate.leaf()
    }
    pub(crate) fn is_current(&self) -> bool {
        self.0.current() && self.0.verdict().is_none()
    }
    /// Secret proxy credentials for an isolated script-disabled native probe only.
    pub(crate) fn probe_proxy_uri(&self) -> Option<String> {
        self.is_current().then(|| self.0.proxy.clone())
    }
    pub(crate) fn ack(&self, observation: TrustObservation) -> bool {
        let Ok(mut verdict) = self.0.verdict.lock() else {
            self.0.cancel();
            tracing::error!("native trust ACK poisoned; refused");
            return false;
        };
        if verdict.is_some() || !self.0.current() {
            return false;
        }
        let valid = observation.unknown_ca_only
            && self.0.flight.load(Ordering::Acquire)
            && observation.leaf == self.leaf()
            && Authority::new(&observation.host, observation.port)
                .is_ok_and(|a| a == self.0.authority);
        *verdict = Some(valid);
        if !valid {
            self.0.live.store(false, Ordering::Release);
        }
        self.0.changed.notify_waiters();
        valid
    }
    pub(crate) fn reject(&self) -> bool {
        let Ok(mut verdict) = self.0.verdict.lock() else {
            self.0.cancel();
            tracing::error!("native trust rejection poisoned");
            return false;
        };
        if verdict.is_some() {
            return false;
        }
        *verdict = Some(false);
        self.0.cancel();
        true
    }
}
impl Drop for TrustRequest {
    fn drop(&mut self) {
        if self.0.verdict().is_none() {
            self.0.cancel();
        }
    }
}
