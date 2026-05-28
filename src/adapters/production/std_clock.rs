//! SystemClock — `std::time` 기반 production Clock.

use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::ports::clock::Clock;

#[derive(Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_instant(&self) -> Instant {
        Instant::now()
    }

    fn now_system(&self) -> SystemTime {
        SystemTime::now()
    }

    fn now_unix_millis(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    }
}
