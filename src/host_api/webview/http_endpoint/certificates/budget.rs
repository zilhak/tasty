//! One runtime/connection budget survives old document retirement and queued/running jobs.
use super::{CERTIFICATE_BYTES, CertificateError, MAX_GENERATORS};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
struct Resource {
    generators: AtomicUsize,
    reserved: AtomicUsize,
}
#[derive(Clone)]
pub(crate) struct CertificateBudget(Arc<Resource>);
impl CertificateBudget {
    pub(crate) fn new() -> Self {
        Self(Arc::new(Resource {
            generators: AtomicUsize::new(0),
            reserved: AtomicUsize::new(0),
        }))
    }
    #[cfg(test)]
    pub(crate) fn reserved(&self) -> usize {
        self.0.reserved.load(Ordering::Acquire)
    }
    #[cfg(test)]
    pub(crate) fn generators(&self) -> usize {
        self.0.generators.load(Ordering::Acquire)
    }
    pub(super) fn reserve(&self) -> Result<Arc<Lease>, CertificateError> {
        self.0
            .reserved
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                n.checked_add(CERTIFICATE_BYTES)
                    .filter(|n| *n <= tasty_ipc::admission::QUEUED_BYTES_LIMIT)
            })
            .map_err(|_| CertificateError::Capacity)?;
        Ok(Arc::new(Lease(self.0.clone())))
    }
    pub(super) fn generator(&self) -> Result<Permit, CertificateError> {
        self.0
            .generators
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < MAX_GENERATORS).then_some(n + 1)
            })
            .map_err(|_| CertificateError::Capacity)?;
        Ok(Permit(self.0.clone()))
    }
}
pub(super) struct Lease(Arc<Resource>);
impl Drop for Lease {
    fn drop(&mut self) {
        self.0
            .reserved
            .fetch_sub(CERTIFICATE_BYTES, Ordering::AcqRel);
    }
}
pub(super) struct Permit(Arc<Resource>);
impl Drop for Permit {
    fn drop(&mut self) {
        self.0.generators.fetch_sub(1, Ordering::AcqRel);
    }
}
