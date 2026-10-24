use super::{NativeError, Result};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
struct Quota {
    used: AtomicUsize,
    limit: usize,
}
#[derive(Clone)]
pub struct NativeBudget(Arc<Quota>);
impl NativeBudget {
    pub fn new(limit: usize) -> Self {
        Self(Arc::new(Quota {
            used: 0.into(),
            limit,
        }))
    }
    pub fn used(&self) -> usize {
        self.0.used.load(Ordering::Acquire)
    }
    pub(super) fn reserve(&self, bytes: usize) -> Result<Lease> {
        self.0
            .used
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(bytes).filter(|n| *n <= self.0.limit)
            })
            .map_err(|_| NativeError::Quota)?;
        Ok(Lease {
            quota: self.0.clone(),
            bytes,
        })
    }
}
pub(super) struct Lease {
    quota: Arc<Quota>,
    bytes: usize,
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.quota.used.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}

impl Lease {
    pub(super) fn grow(&mut self, extra: usize) -> Result<()> {
        self.quota
            .used
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(extra).filter(|n| *n <= self.quota.limit)
            })
            .map_err(|_| NativeError::Quota)?;
        self.bytes += extra;
        Ok(())
    }
}
