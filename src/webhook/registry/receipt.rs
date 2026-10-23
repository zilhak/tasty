//! Bounded ownership extends through response consumption and the HTTP execution lifetime.
use super::{MatchResult, RegisterOutcome};
use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use tasty_utils::batch_writer::Limits;

#[derive(Debug)]
pub(crate) struct Failure {
    pub(crate) message: String,
    pub(crate) unknown: bool,
}
impl Failure {
    pub(super) fn refused(message: &str) -> Self {
        Self {
            message: message.into(),
            unknown: false,
        }
    }
}
pub(crate) enum Value {
    Registered(RegisterOutcome),
    Removed(bool),
    Swept(Vec<String>),
    Matched(MatchResult),
    Persisted,
}
pub(crate) struct Completed {
    pub(crate) value: Value,
    pub(crate) saved: Option<bool>,
}

pub(super) struct Budget {
    state: Mutex<(usize, usize)>,
    count: usize,
    bytes: usize,
}
impl Budget {
    pub(super) fn new(limits: Limits) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new((0, 0)),
            count: limits.queue_items,
            bytes: limits.queue_bytes,
        })
    }
    pub(super) fn admit(self: &Arc<Self>, bytes: usize) -> Result<Lease, Failure> {
        let mut s = self.state.lock().unwrap();
        if s.0 >= self.count || bytes > self.bytes.saturating_sub(s.1) {
            return Err(Failure::refused("webhook receipt capacity exceeded"));
        }
        s.0 += 1;
        s.1 += bytes;
        Ok(Lease(Arc::new(Charge {
            budget: self.clone(),
            bytes: AtomicUsize::new(bytes),
        })))
    }
    pub(super) fn usage(&self) -> (usize, usize) {
        *self.state.lock().unwrap()
    }
}
struct Charge {
    budget: Arc<Budget>,
    bytes: AtomicUsize,
}
impl Drop for Charge {
    fn drop(&mut self) {
        let mut s = self.budget.state.lock().unwrap();
        s.0 -= 1;
        s.1 -= self.bytes.load(Ordering::Relaxed);
    }
}
#[derive(Clone)]
pub(crate) struct Lease(Arc<Charge>);
impl Lease {
    pub(super) fn grow(&self, bytes: usize) -> Result<(), Failure> {
        let mut s = self.0.budget.state.lock().unwrap();
        if bytes > self.0.budget.bytes.saturating_sub(s.1) {
            return Err(Failure::refused(
                "webhook retained section capacity exceeded",
            ));
        }
        s.1 += bytes;
        self.0.bytes.fetch_add(bytes, Ordering::Relaxed);
        Ok(())
    }
}
pub(super) struct Reply {
    result: Mutex<Option<Result<Completed, Failure>>>,
    ready: Condvar,
    lease: Lease,
}
impl Reply {
    pub(super) fn finish(&self, mut result: Result<Completed, Failure>) {
        if let Err(error) = &mut result
            && self.lease.grow(error.message.len()).is_err()
        {
            error.message =
                "webhook operation failed; diagnostic details exceed receipt capacity".into();
        }
        *self.result.lock().unwrap() = Some(result);
        self.ready.notify_all();
    }
    pub(super) fn grow(&self, bytes: usize) -> Result<(), Failure> {
        self.lease.grow(bytes)
    }
}
pub(crate) struct Receipt {
    inner: Inner,
}
enum Inner {
    Pending(Arc<Reply>),
    Ready(Option<Result<Completed, Failure>>),
}
impl Receipt {
    pub(super) fn new(lease: Lease) -> (Self, Arc<Reply>) {
        let reply = Arc::new(Reply {
            result: Mutex::new(None),
            ready: Condvar::new(),
            lease,
        });
        (
            Self {
                inner: Inner::Pending(reply.clone()),
            },
            reply,
        )
    }
    pub(super) fn refused(error: Failure) -> Self {
        Self {
            inner: Inner::Ready(Some(Err(error))),
        }
    }
    pub(crate) fn poll(&mut self) -> Option<Result<Completed, Failure>> {
        match &mut self.inner {
            Inner::Ready(value) => value.take(),
            Inner::Pending(reply) => reply.result.lock().unwrap().take(),
        }
    }
    /// Only the existing HTTP request thread waits; callers never hold the registry guard here.
    pub(super) fn wait(mut self) -> (Result<Completed, Failure>, Option<Lease>) {
        if let Inner::Pending(reply) = self.inner {
            let mut result = reply.result.lock().unwrap();
            while result.is_none() {
                result = reply.ready.wait(result).unwrap();
            }
            (
                result.take().expect("ready receipt"),
                Some(reply.lease.clone()),
            )
        } else {
            (self.poll().expect("ready refusal"), None)
        }
    }
}
