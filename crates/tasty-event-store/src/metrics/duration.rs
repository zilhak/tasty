use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Inclusive microsecond bounds followed by an unbounded overflow bin.
pub const JOURNAL_LATENCY_BOUNDS_US: [u64; 11] = [
    10, 32, 100, 316, 1_000, 3_162, 10_000, 31_623, 100_000, 316_228, 1_000_000,
];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct JournalDurationSnapshot {
    pub samples: u64,
    pub sum_us: u64,
    pub max_us: u64,
    pub counts: [u64; 12],
}

/// Fixed aggregate storage. No request data or raw timing history is retained.
#[derive(Default)]
pub struct JournalDuration {
    samples: AtomicU64,
    sum: AtomicU64,
    max: AtomicU64,
    counts: [AtomicU64; 12],
}
impl JournalDuration {
    pub fn record(&self, elapsed: Duration) {
        let us = u64::try_from(elapsed.as_micros()).unwrap_or(u64::MAX);
        let bin = JOURNAL_LATENCY_BOUNDS_US.partition_point(|bound| *bound < us);
        self.counts[bin].fetch_add(1, Ordering::Relaxed);
        self.sum.fetch_add(us, Ordering::Relaxed);
        self.max.fetch_max(us, Ordering::Relaxed);
        self.samples.fetch_add(1, Ordering::Relaxed);
    }
    pub fn snapshot(&self) -> JournalDurationSnapshot {
        JournalDurationSnapshot {
            samples: self.samples.load(Ordering::Relaxed),
            sum_us: self.sum.load(Ordering::Relaxed),
            max_us: self.max.load(Ordering::Relaxed),
            counts: std::array::from_fn(|i| self.counts[i].load(Ordering::Relaxed)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inclusive_bins_overflow_and_zero_observations_are_distinct() {
        let stats = JournalDuration::default();
        assert_eq!(stats.snapshot(), JournalDurationSnapshot::default());
        for us in [0, 10, 11, 32, 1_000_001] {
            stats.record(Duration::from_micros(us));
        }
        let s = stats.snapshot();
        assert_eq!(s.samples, 5);
        assert_eq!(s.counts[0], 2);
        assert_eq!(s.counts[1], 2);
        assert_eq!(s.counts[11], 1);
        assert_eq!(s.counts.iter().sum::<u64>(), s.samples);
        assert_eq!(s.sum_us, 1_000_054);
        assert_eq!(s.max_us, 1_000_001);
        assert_eq!(stats.snapshot(), s);
    }
}
