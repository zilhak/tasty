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
    entries: BTreeMap<u64, (Instant, usize, bool)>,
    queued_items: usize,
    queued_bytes: usize,
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
        work.entries.insert(seq, (at, bytes, true));
        work.queued_items += 1;
        work.queued_bytes += bytes;
        work.bytes += bytes;
        RequestMetric {
            stats: self.clone(),
            seq: Some(seq),
        }
    }

    pub(super) fn start_batch(&self, seqs: impl Iterator<Item = u64>) {
        let mut work = self.lock();
        for seq in seqs {
            let bytes = work
                .entries
                .get_mut(&seq)
                .and_then(|(_, bytes, queued)| std::mem::replace(queued, false).then_some(*bytes));
            if let Some(bytes) = bytes {
                work.queued_items -= 1;
                work.queued_bytes -= bytes;
            }
        }
    }

    pub(super) fn snapshot(&self, now: Instant) -> PendingSnapshot {
        let work = self.lock();
        PendingSnapshot {
            items: work.entries.len() as u64,
            bytes: work.bytes as u64,
            queued_items: work.queued_items as u64,
            queued_bytes: work.queued_bytes as u64,
            oldest: work
                .entries
                .first_key_value()
                .map(|(_, (at, _, _))| micros(now.saturating_duration_since(*at))),
            outcomes: std::array::from_fn(|i| self.outcomes[i].load(Ordering::Relaxed)),
        }
    }
}

pub(super) struct PendingSnapshot {
    pub(super) items: u64,
    pub(super) bytes: u64,
    pub(super) queued_items: u64,
    pub(super) queued_bytes: u64,
    pub(super) oldest: Option<u64>,
    pub(super) outcomes: [u64; 5],
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
            if let Some((_, bytes, queued)) = work.entries.remove(&seq) {
                work.bytes -= bytes;
                if queued {
                    work.queued_items -= 1;
                    work.queued_bytes -= bytes;
                }
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
