//! Admission precedes binding/reading auth; old owners retain capacity until actual task Drop.
use super::super::certificates::CertificateBudget;
use super::EndpointError;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
struct Resource {
    owners: AtomicUsize,
    sockets: Arc<Semaphore>,
    preparations: Arc<Semaphore>,
    certificates: CertificateBudget,
}
#[derive(Clone)]
pub(crate) struct EndpointBudget(Arc<Resource>);
impl EndpointBudget {
    pub(crate) fn new() -> Self {
        Self(Arc::new(Resource {
            owners: AtomicUsize::new(0),
            sockets: Arc::new(Semaphore::new(8)),
            preparations: Arc::new(Semaphore::new(8)),
            certificates: CertificateBudget::new(),
        }))
    }
    pub(super) fn owner(&self) -> Result<Owner, EndpointError> {
        self.0
            .owners
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < 8).then_some(n + 1)
            })
            .map_err(|_| EndpointError::Capacity)?;
        Ok(Owner(self.0.clone()))
    }
    pub(super) fn socket(&self) -> Result<OwnedSemaphorePermit, EndpointError> {
        self.0
            .sockets
            .clone()
            .try_acquire_owned()
            .map_err(|_| EndpointError::Capacity)
    }
    pub(super) fn certificates(&self) -> CertificateBudget {
        self.0.certificates.clone()
    }
    pub(super) fn preparation(&self) -> Result<OwnedSemaphorePermit, EndpointError> {
        self.0
            .preparations
            .clone()
            .try_acquire_owned()
            .map_err(|_| EndpointError::Capacity)
    }
    #[cfg(test)]
    pub(crate) fn active_preparations(&self) -> usize {
        8 - self.0.preparations.available_permits()
    }
    pub(crate) fn active_connections(&self) -> usize {
        8 - self.0.sockets.available_permits()
    }
    pub(crate) fn active_owners(&self) -> usize {
        self.0.owners.load(Ordering::Acquire)
    }
}
pub(super) struct Owner(Arc<Resource>);
impl Drop for Owner {
    fn drop(&mut self) {
        self.0.owners.fetch_sub(1, Ordering::AcqRel);
    }
}
