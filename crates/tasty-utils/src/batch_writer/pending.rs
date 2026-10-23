//! Accepted work lifetime, independent of receipt retention and physical commit semantics.
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use super::measurement::micros;

static POISONED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy)]
pub(super) enum Outcome {
    Ok,
    Rejected,
    Failed,
    Unknown,
    Lost,
}

#[derive(Debug, Default)]
struct Work {
    // Existing admission budget bounds this map by queue_items, including abandoned receipts.
    entries: BTreeMap<u64, (Instant, usize)>,
    bytes: usize,
}

#[derive(Debug, Default)]
pub(super) struct PendingStats {
    work: Mutex<Work>,
    outcomes: [AtomicU64; 5],
}

impl PendingStats {
    fn lock(&self) -> std::sync::MutexGuard<'_, Work> {
        crate::poison::recover_mutex(self.work.lock(), "writer pending measurements", &POISONED)
    }

    pub(super) fn track(self: &Arc<Self>, seq: u64, at: Instant, bytes: usize) -> RequestMetric {
        let mut work = self.lock();
        work.entries.insert(seq, (at, bytes));
        work.bytes += bytes;
        RequestMetric {
            stats: self.clone(),
            seq: Some(seq),
        }
    }

    pub(super) fn snapshot(&self, now: Instant) -> (u64, u64, Option<u64>, [u64; 5]) {
        let work = self.lock();
        let oldest = work
            .entries
            .first_key_value()
            .map(|(_, (at, _))| micros(now.saturating_duration_since(*at)));
        (
            work.entries.len() as u64,
            work.bytes as u64,
            oldest,
            std::array::from_fn(|i| self.outcomes[i].load(Ordering::Relaxed)),
        )
    }
}

pub(super) struct RequestMetric {
    stats: Arc<PendingStats>,
    seq: Option<u64>,
}

impl RequestMetric {
    pub(super) fn finish(mut self, outcome: Outcome) {
        self.close(outcome);
    }

    fn close(&mut self, outcome: Outcome) {
        if let Some(seq) = self.seq.take() {
            let mut work = self.stats.lock();
            if let Some((_, bytes)) = work.entries.remove(&seq) {
                work.bytes -= bytes;
                self.stats.outcomes[outcome as usize].fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

impl Drop for RequestMetric {
    fn drop(&mut self) {
        self.close(Outcome::Lost);
    }
}
