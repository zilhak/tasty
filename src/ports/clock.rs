//! Clock port — 시각 source. test 시 deterministic.

use std::time::{Instant, SystemTime};

#[allow(dead_code)]
pub trait Clock: Send + Sync {
    fn now_instant(&self) -> Instant;
    fn now_system(&self) -> SystemTime;
    /// Unix ms (telemetry / audit / memory entry timestamp 용).
    fn now_unix_millis(&self) -> i64;
}
