//! Thread-scoped observation of actual common user-file replacement/deletion operations.
use std::{
    cell::RefCell,
    marker::PhantomData,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};

pub const FILE_OPERATION_BOUNDS_US: [u64; 11] = [
    10, 32, 100, 316, 1_000, 3_162, 10_000, 31_623, 100_000, 316_228, 1_000_000,
];
#[derive(Default, Debug)]
struct Counts {
    attempts: AtomicU64,
    succeeded: AtomicU64,
    failed: AtomicU64,
    unknown: AtomicU64,
    payload_bytes: AtomicU64,
    published_bytes: AtomicU64,
    duration_us: AtomicU64,
    max_duration_us: AtomicU64,
    durations: [AtomicU64; 12],
    errors: [AtomicU64; 5],
}
#[derive(Default, Debug)]
pub struct FileOperationMetrics {
    replace: Counts,
    delete: Counts,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OperationSnapshot {
    pub attempts: u64,
    pub succeeded: u64,
    pub failed: u64,
    pub unknown: u64,
    pub payload_bytes: u64,
    pub published_bytes: u64,
    pub duration_us: u64,
    pub max_duration_us: u64,
    pub durations: [u64; 12],
    /// NotFound, PermissionDenied, AlreadyExists, InvalidData/InvalidInput, Other.
    pub errors: [u64; 5],
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FileOperationSnapshot {
    pub replace: OperationSnapshot,
    pub delete: OperationSnapshot,
}
impl Counts {
    fn snapshot(&self) -> OperationSnapshot {
        let read = |value: &AtomicU64| value.load(Ordering::Relaxed);
        OperationSnapshot {
            attempts: read(&self.attempts),
            succeeded: read(&self.succeeded),
            failed: read(&self.failed),
            unknown: read(&self.unknown),
            payload_bytes: read(&self.payload_bytes),
            published_bytes: read(&self.published_bytes),
            duration_us: read(&self.duration_us),
            max_duration_us: read(&self.max_duration_us),
            durations: std::array::from_fn(|i| read(&self.durations[i])),
            errors: std::array::from_fn(|i| read(&self.errors[i])),
        }
    }
}
impl FileOperationMetrics {
    /// Observational fixed aggregates; no receipt is consumed and no file is read.
    pub fn snapshot(&self) -> FileOperationSnapshot {
        FileOperationSnapshot {
            replace: self.replace.snapshot(),
            delete: self.delete.snapshot(),
        }
    }
}
thread_local! { static CURRENT: RefCell<Option<Arc<FileOperationMetrics>>> = const { RefCell::new(None) }; }
/// Restores the prior observation scope on the same thread, including during unwinding.
pub struct FileOperationScope {
    previous: Option<Arc<FileOperationMetrics>>,
    _thread: PhantomData<Rc<()>>,
}
pub fn observe_operations(metrics: Arc<FileOperationMetrics>) -> FileOperationScope {
    let previous = CURRENT.with(|slot| slot.replace(Some(metrics)));
    FileOperationScope {
        previous,
        _thread: PhantomData,
    }
}
impl Drop for FileOperationScope {
    fn drop(&mut self) {
        CURRENT.with(|slot| {
            slot.replace(self.previous.take());
        });
    }
}
#[derive(Clone, Copy)]
pub(super) enum Kind {
    Replace,
    Delete,
}
struct Attempt {
    metrics: Arc<FileOperationMetrics>,
    kind: Kind,
    bytes: u64,
    started: Instant,
    finished: bool,
}
impl Attempt {
    fn counts(&self) -> &Counts {
        match self.kind {
            Kind::Replace => &self.metrics.replace,
            Kind::Delete => &self.metrics.delete,
        }
    }
    fn finish(&mut self, result: Result<(), std::io::ErrorKind>) {
        let counts = self.counts();
        match result {
            Ok(()) => {
                counts.succeeded.fetch_add(1, Ordering::Relaxed);
                counts
                    .published_bytes
                    .fetch_add(self.bytes, Ordering::Relaxed);
            }
            Err(kind) => {
                counts.failed.fetch_add(1, Ordering::Relaxed);
                let index = match kind {
                    std::io::ErrorKind::NotFound => 0,
                    std::io::ErrorKind::PermissionDenied => 1,
                    std::io::ErrorKind::AlreadyExists => 2,
                    std::io::ErrorKind::InvalidData | std::io::ErrorKind::InvalidInput => 3,
                    _ => 4,
                };
                counts.errors[index].fetch_add(1, Ordering::Relaxed);
            }
        }
        self.finished = true;
    }
}
impl Drop for Attempt {
    fn drop(&mut self) {
        let counts = self.counts();
        if !self.finished {
            counts.unknown.fetch_add(1, Ordering::Relaxed);
        }
        let elapsed = u64::try_from(self.started.elapsed().as_micros()).unwrap_or(u64::MAX);
        counts.duration_us.fetch_add(elapsed, Ordering::Relaxed);
        counts.max_duration_us.fetch_max(elapsed, Ordering::Relaxed);
        let bucket = FILE_OPERATION_BOUNDS_US
            .iter()
            .position(|bound| elapsed <= *bound)
            .unwrap_or(FILE_OPERATION_BOUNDS_US.len());
        counts.durations[bucket].fetch_add(1, Ordering::Relaxed);
    }
}
/// One custom private-file replacement, inert outside an active observation scope.
/// Begin after validation and participant locking, immediately before private-temp creation.
/// Do not wrap the already observed common replacement helpers with this guard.
pub struct FileOperationAttempt {
    attempt: Option<Attempt>,
    _thread: PhantomData<Rc<()>>,
}
impl FileOperationAttempt {
    /// Record the returned result, retaining only a fixed error kind. Unfinished drop is Unknown.
    pub fn finish(mut self, result: Result<(), std::io::ErrorKind>) {
        if let Some(attempt) = &mut self.attempt {
            attempt.finish(result);
        }
    }
}
pub fn observe_replacement(payload_bytes: usize) -> FileOperationAttempt {
    begin(Kind::Replace, payload_bytes)
}
fn begin(kind: Kind, bytes: usize) -> FileOperationAttempt {
    let attempt = CURRENT.with(|slot| slot.borrow().clone()).map(|metrics| {
        let attempt = Attempt {
            metrics,
            kind,
            bytes: u64::try_from(bytes).unwrap_or(u64::MAX),
            started: Instant::now(),
            finished: false,
        };
        attempt.counts().attempts.fetch_add(1, Ordering::Relaxed);
        attempt
            .counts()
            .payload_bytes
            .fetch_add(attempt.bytes, Ordering::Relaxed);
        attempt
    });
    FileOperationAttempt {
        attempt,
        _thread: PhantomData,
    }
}
pub(super) fn measure(
    kind: Kind,
    bytes: usize,
    work: impl FnOnce() -> std::io::Result<()>,
) -> std::io::Result<()> {
    let attempt = begin(kind, bytes);
    let result = work();
    attempt.finish(result.as_ref().map(|()| ()).map_err(std::io::Error::kind));
    result
}

#[cfg(test)]
mod tests;
