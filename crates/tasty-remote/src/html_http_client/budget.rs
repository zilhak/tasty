use super::ClientError;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

/// Shared provisional capacity quota. Independent of encoded consent and verified-body memory.
#[derive(Clone)]
pub struct PendingBudget(Arc<Quota>);
struct Quota {
    used: AtomicUsize,
    limit: usize,
}
pub(super) struct Lease {
    quota: Arc<Quota>,
    bytes: usize,
}
impl PendingBudget {
    pub fn new(limit: usize) -> Self {
        Self(Arc::new(Quota {
            used: AtomicUsize::new(0),
            limit,
        }))
    }
    pub fn used(&self) -> usize {
        self.0.used.load(Ordering::Acquire)
    }
    pub(super) fn reserve(&self, bytes: usize) -> Result<Lease, ClientError> {
        self.0
            .used
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                n.checked_add(bytes).filter(|n| *n <= self.0.limit)
            })
            .map_err(|_| ClientError::Quota)?;
        Ok(Lease {
            quota: self.0.clone(),
            bytes,
        })
    }
}
impl Lease {
    pub(super) fn retain(&mut self, bytes: usize) -> Result<(), ClientError> {
        if bytes >= self.bytes {
            return self.grow(bytes - self.bytes);
        }
        self.quota
            .used
            .fetch_sub(self.bytes - bytes, Ordering::AcqRel);
        self.bytes = bytes;
        Ok(())
    }
    pub(super) fn grow(&mut self, bytes: usize) -> Result<(), ClientError> {
        self.quota
            .used
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                n.checked_add(bytes).filter(|n| *n <= self.quota.limit)
            })
            .map_err(|_| ClientError::Quota)?;
        self.bytes += bytes;
        Ok(())
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.quota.used.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}
