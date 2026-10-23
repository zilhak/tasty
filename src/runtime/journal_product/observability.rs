//! Bounded transport observations. No storage, payloads, receipts or scheduling decisions.
mod lifecycle;
#[cfg(test)]
mod tests;

pub(super) use lifecycle::{Observation, Outcome};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;
use tasty_event_store::{
    CommitMetricsReader, JournalDuration, JournalDurationSnapshot, StoreMetricsSnapshot,
};
use tasty_utils::batch_writer::{Clock, SystemClock};

const SLOTS: usize = super::QUEUE_CAPACITY * 2 + 2;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct WorkCounts {
    pub admitted: u64,
    pub admitted_bytes_estimate: u64,
    pub rejected_busy: u64,
    pub rejected_too_large: u64,
    pub rejected_stopped: u64,
    pub handled_ok: u64,
    pub handled_error: u64,
    pub deferred: u64,
    pub lost: u64,
    pub queued_items: u64,
    pub queued_bytes_estimate: u64,
    pub inflight_items: u64,
    pub inflight_bytes_estimate: u64,
    pub pending_items: u64,
    pub pending_bytes_estimate: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct JournalMetricsSnapshot {
    pub work: WorkCounts,
    pub store: Option<StoreMetricsSnapshot>,
    pub queue_wait: JournalDurationSnapshot,
    pub execution: JournalDurationSnapshot,
    pub completion: JournalDurationSnapshot,
    pub batch_wait: JournalDurationSnapshot,
    pub batch_execution: JournalDurationSnapshot,
    pub batches: u64,
    pub batch_requests: u64,
    pub batch_size_max: u64,
    pub batch_sizes: [u64; 8],
    pub batch_bytes_estimate: u64,
    pub batch_errors: u64,
    pub publication_errors: u64,
    pub group_revision_retries: u64,
    pub scalar_revision_retries: Option<u64>,
    pub oldest_pending_age_us: Option<u64>,
    pub unpublished_age_us: Option<u64>,
    pub publication_known: bool,
    pub open_admissions: u64,
    pub waiting_admissions: u64,
    pub admission_bytes_estimate: u64,
    pub observation_overflow: u64,
}

struct State {
    next: u64,
    entries: [Option<lifecycle::Entry>; SLOTS],
    counts: WorkCounts,
    unpublished_since: Option<Instant>,
    publication_known: bool,
    open_admissions: u64,
    waiting_admissions: u64,
    admission_bytes_estimate: u64,
}

impl Default for State {
    fn default() -> Self {
        Self {
            next: 0,
            entries: std::array::from_fn(|_| None),
            counts: WorkCounts::default(),
            unpublished_since: None,
            publication_known: false,
            open_admissions: 0,
            waiting_admissions: 0,
            admission_bytes_estimate: 0,
        }
    }
}

pub(crate) struct JournalMetrics {
    state: Mutex<State>,
    poisoned: AtomicBool,
    clock: Arc<dyn Clock>,
    store: OnceLock<CommitMetricsReader>,
    scalar_retries: OnceLock<Arc<AtomicU64>>,
    queue_wait: JournalDuration,
    execution: JournalDuration,
    completion: JournalDuration,
    batch_wait: JournalDuration,
    batch_execution: JournalDuration,
    batches: AtomicU64,
    batch_requests: AtomicU64,
    batch_size_max: AtomicU64,
    batch_sizes: [AtomicU64; 8],
    batch_bytes: AtomicU64,
    batch_errors: AtomicU64,
    publication_errors: AtomicU64,
    retries: AtomicU64,
    overflow: AtomicU64,
}
impl Default for JournalMetrics {
    fn default() -> Self {
        Self::with_clock(Arc::new(SystemClock))
    }
}
impl JournalMetrics {
    pub(super) fn with_clock(clock: Arc<dyn Clock>) -> Self {
        Self {
            state: Mutex::new(State::default()),
            poisoned: AtomicBool::new(false),
            clock,
            store: OnceLock::new(),
            scalar_retries: OnceLock::new(),
            queue_wait: Default::default(),
            execution: Default::default(),
            completion: Default::default(),
            batch_wait: Default::default(),
            batch_execution: Default::default(),
            batches: AtomicU64::new(0),
            batch_requests: AtomicU64::new(0),
            batch_size_max: AtomicU64::new(0),
            batch_sizes: std::array::from_fn(|_| AtomicU64::new(0)),
            batch_bytes: AtomicU64::new(0),
            batch_errors: AtomicU64::new(0),
            publication_errors: AtomicU64::new(0),
            retries: AtomicU64::new(0),
            overflow: AtomicU64::new(0),
        }
    }
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        tasty_utils::poison::recover_mutex(
            self.state.lock(),
            "journal observations",
            &self.poisoned,
        )
    }
    pub(super) fn attach_scalar_retries(&self, reader: Arc<AtomicU64>) {
        if self.scalar_retries.set(reader).is_err() {
            tracing::warn!("journal retry metrics already attached");
        }
    }
    pub(super) fn attach_store(&self, reader: CommitMetricsReader) {
        if self.store.set(reader).is_err() {
            tracing::warn!("journal metrics store already attached");
        }
    }
    pub(crate) fn snapshot(&self) -> JournalMetricsSnapshot {
        let state = self.lock();
        let mut work = state.counts;
        let mut oldest = None;
        for e in state
            .entries
            .iter()
            .flatten()
            .filter(|e| e.accepted && e.outcome.is_none())
        {
            work.pending_items += 1;
            work.pending_bytes_estimate += e.bytes;
            if e.started.is_some() {
                work.inflight_items += 1;
                work.inflight_bytes_estimate += e.bytes;
            } else {
                work.queued_items += 1;
                work.queued_bytes_estimate += e.bytes;
            }
            oldest = Some(oldest.map_or(e.submitted, |old: Instant| old.min(e.submitted)));
        }
        let age = |then: Instant| micros(self.clock.now().saturating_duration_since(then));
        JournalMetricsSnapshot {
            work,
            store: self.store.get().map(CommitMetricsReader::snapshot),
            queue_wait: self.queue_wait.snapshot(),
            execution: self.execution.snapshot(),
            completion: self.completion.snapshot(),
            batch_wait: self.batch_wait.snapshot(),
            batch_execution: self.batch_execution.snapshot(),
            batches: self.batches.load(Ordering::Relaxed),
            batch_requests: self.batch_requests.load(Ordering::Relaxed),
            batch_size_max: self.batch_size_max.load(Ordering::Relaxed),
            batch_sizes: std::array::from_fn(|i| self.batch_sizes[i].load(Ordering::Relaxed)),
            batch_bytes_estimate: self.batch_bytes.load(Ordering::Relaxed),
            batch_errors: self.batch_errors.load(Ordering::Relaxed),
            publication_errors: self.publication_errors.load(Ordering::Relaxed),
            group_revision_retries: self.retries.load(Ordering::Relaxed),
            scalar_revision_retries: self.scalar_retries.get().map(|r| r.load(Ordering::Relaxed)),
            oldest_pending_age_us: oldest.map(age),
            unpublished_age_us: state
                .publication_known
                .then(|| state.unpublished_since.map(age))
                .flatten(),
            publication_known: state.publication_known,
            open_admissions: state.open_admissions,
            waiting_admissions: state.waiting_admissions,
            admission_bytes_estimate: state.admission_bytes_estimate,
            observation_overflow: self.overflow.load(Ordering::Relaxed),
        }
    }
    pub(super) fn publication_error(&self) {
        self.publication_errors.fetch_add(1, Ordering::Relaxed);
    }

    pub(super) fn publication_unknown(&self) {
        self.lock().publication_known = false;
    }

    pub(super) fn publication(&self, durable: Option<u64>, acknowledged: Option<u64>) {
        let mut state = self.lock();
        state.publication_known = true;
        if durable > acknowledged {
            state
                .unpublished_since
                .get_or_insert_with(|| self.clock.now());
        } else {
            state.unpublished_since = None;
        }
    }
    pub(super) fn admissions(&self, open: usize, waiting: usize, bytes: usize) {
        let mut s = self.lock();
        s.open_admissions = open as u64;
        s.waiting_admissions = waiting as u64;
        s.admission_bytes_estimate = bytes as u64;
    }
    pub(super) fn batch_start(&self, oldest: Instant) -> Instant {
        let now = self.clock.now();
        self.batch_wait
            .record(now.saturating_duration_since(oldest));
        now
    }
    pub(super) fn batch_finished(
        &self,
        start: Instant,
        requests: usize,
        halted: bool,
        retries: u64,
        bytes: u64,
    ) {
        self.batch_execution
            .record(self.clock.now().saturating_duration_since(start));
        self.batches.fetch_add(1, Ordering::Relaxed);
        self.batch_requests
            .fetch_add(requests as u64, Ordering::Relaxed);
        self.batch_size_max
            .fetch_max(requests as u64, Ordering::Relaxed);
        let bin = [1, 2, 4, 8, 16, 32, 64].partition_point(|bound| *bound < requests);
        self.batch_sizes[bin].fetch_add(1, Ordering::Relaxed);
        self.batch_bytes.fetch_add(bytes, Ordering::Relaxed);
        self.batch_errors
            .fetch_add(u64::from(halted), Ordering::Relaxed);
        self.retries.fetch_add(retries, Ordering::Relaxed);
    }
}
fn micros(duration: std::time::Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}

impl JournalMetrics {
    pub(super) fn admission_released(&self, count: usize, bytes: usize) {
        let mut state = self.lock();
        state.open_admissions = state.open_admissions.saturating_sub(count as u64);
        state.admission_bytes_estimate =
            state.admission_bytes_estimate.saturating_sub(bytes as u64);
    }
}

pub(super) struct WorkerScope(pub Arc<JournalMetrics>);
impl Drop for WorkerScope {
    fn drop(&mut self) {
        self.0.admissions(0, 0, 0);
    }
}
