//! Runtime thresholds; the settings crate stays independent of telemetry.
use crate::anomaly::{
    CALL_BURST_THRESHOLD, CALL_BURST_WINDOW_MS, RSS_SURGE_MIN_SAMPLES, SLOW_LOOP_THRESHOLD,
    SLOW_LOOP_WINDOW_MS,
};

pub const MIB: u64 = 1024 * 1024;
pub const RSS_SURGE_MIN_BYTES: u64 = 256 * MIB;
pub const RSS_SAMPLES_RANGE: (usize, usize) = (3, 20);
pub const RSS_MIN_BYTES_RANGE: (u64, u64) = (0, 65536 * MIB);
pub const SLOW_WINDOW_MS_RANGE: (u64, u64) = (30_000, 3_600_000);
pub const SLOW_THRESHOLD_RANGE: (usize, usize) = (2, 10000);
pub const BURST_WINDOW_MS_RANGE: (u64, u64) = (1000, 600_000);
pub const BURST_THRESHOLD_RANGE: (usize, usize) = (10, 100000);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnomalyThresholds {
    pub call_burst_window_ms: u64,
    pub call_burst_threshold: usize,
    pub slow_loop_window_ms: u64,
    pub slow_loop_threshold: usize,
    pub rss_surge_samples: usize,
    pub rss_surge_min_bytes: u64,
}
impl Default for AnomalyThresholds {
    fn default() -> Self {
        Self {
            call_burst_window_ms: CALL_BURST_WINDOW_MS,
            call_burst_threshold: CALL_BURST_THRESHOLD,
            slow_loop_window_ms: SLOW_LOOP_WINDOW_MS,
            slow_loop_threshold: SLOW_LOOP_THRESHOLD,
            rss_surge_samples: RSS_SURGE_MIN_SAMPLES,
            rss_surge_min_bytes: RSS_SURGE_MIN_BYTES,
        }
    }
}
impl AnomalyThresholds {
    pub fn validate(&self) -> Result<(), String> {
        for (key, value, (lo, hi)) in [
            (
                "rss_surge_samples",
                self.rss_surge_samples as u64,
                (RSS_SAMPLES_RANGE.0 as u64, RSS_SAMPLES_RANGE.1 as u64),
            ),
            (
                "rss_surge_min_bytes",
                self.rss_surge_min_bytes,
                RSS_MIN_BYTES_RANGE,
            ),
            (
                "slow_loop_window_ms",
                self.slow_loop_window_ms,
                SLOW_WINDOW_MS_RANGE,
            ),
            (
                "slow_loop_threshold",
                self.slow_loop_threshold as u64,
                (SLOW_THRESHOLD_RANGE.0 as u64, SLOW_THRESHOLD_RANGE.1 as u64),
            ),
            (
                "call_burst_window_ms",
                self.call_burst_window_ms,
                BURST_WINDOW_MS_RANGE,
            ),
            (
                "call_burst_threshold",
                self.call_burst_threshold as u64,
                (
                    BURST_THRESHOLD_RANGE.0 as u64,
                    BURST_THRESHOLD_RANGE.1 as u64,
                ),
            ),
        ] {
            if !(lo..=hi).contains(&value) {
                return Err(format!(
                    "anomaly threshold {key} {value} is outside {lo}..={hi}"
                ));
            }
        }
        Ok(())
    }
}
