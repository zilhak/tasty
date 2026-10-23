//! Fixed-space service-time histograms. The executor's clock also drives deterministic tests.
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use super::Clock;

/// Inclusive microsecond bounds, matching telemetry latency buckets without an upward dependency.
pub const WRITER_LATENCY_BOUNDS_US: [u64; 11] = [
    10, 32, 100, 316, 1_000, 3_162, 10_000, 31_623, 100_000, 316_228, 1_000_000,
];

pub(super) fn micros(duration: Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}

#[derive(Debug, Default)]
pub(super) struct DurationStats {
    samples: AtomicU64,
    sum_us: AtomicU64,
    max_us: AtomicU64,
    counts: [AtomicU64; WRITER_LATENCY_BOUNDS_US.len() + 1],
}

impl DurationStats {
    pub(super) fn record(&self, elapsed: Duration) {
        let us = micros(elapsed);
        self.samples.fetch_add(1, Ordering::Relaxed);
        self.sum_us.fetch_add(us, Ordering::Relaxed);
        self.max_us.fetch_max(us, Ordering::Relaxed);
        let bucket = WRITER_LATENCY_BOUNDS_US
            .iter()
            .position(|&bound| us <= bound)
            .unwrap_or(WRITER_LATENCY_BOUNDS_US.len());
        self.counts[bucket].fetch_add(1, Ordering::Relaxed);
    }

    pub(super) fn snapshot(&self) -> DurationStatsSnapshot {
        DurationStatsSnapshot {
            samples: self.samples.load(Ordering::Relaxed),
            sum_us: self.sum_us.load(Ordering::Relaxed),
            max_us: self.max_us.load(Ordering::Relaxed),
            counts: std::array::from_fn(|i| self.counts[i].load(Ordering::Relaxed)),
        }
    }
}

/// Aggregate observations, not retained raw samples or exact percentiles. Zero samples means unobserved.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DurationStatsSnapshot {
    pub samples: u64,
    pub sum_us: u64,
    pub max_us: u64,
    pub counts: [u64; WRITER_LATENCY_BOUNDS_US.len() + 1],
}

pub(super) struct Timer<'a> {
    histogram: &'a DurationStats,
    clock: &'a dyn Clock,
    started: std::time::Instant,
}

impl<'a> Timer<'a> {
    pub(super) fn new(histogram: &'a DurationStats, clock: &'a dyn Clock) -> Self {
        Self {
            histogram,
            clock,
            started: clock.now(),
        }
    }
}

impl Drop for Timer<'_> {
    fn drop(&mut self) {
        self.histogram
            .record(self.clock.now().saturating_duration_since(self.started));
    }
}
