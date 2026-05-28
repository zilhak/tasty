//! FakeClock — test 시 시각을 *수동으로 advance*. deterministic.

use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::ports::clock::Clock;

pub struct FakeClock {
    base_instant: Instant,
    base_system: SystemTime,
    elapsed: Mutex<Duration>,
}

impl FakeClock {
    pub fn new() -> Self {
        Self {
            base_instant: Instant::now(),
            base_system: SystemTime::now(),
            elapsed: Mutex::new(Duration::ZERO),
        }
    }

    /// 시각을 `dur` 만큼 앞으로 이동. test 시 호출.
    pub fn advance(&self, dur: Duration) {
        let mut e = self.elapsed.lock().expect("FakeClock poisoned");
        *e += dur;
    }
}

impl Default for FakeClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for FakeClock {
    fn now_instant(&self) -> Instant {
        let e = *self.elapsed.lock().expect("FakeClock poisoned");
        self.base_instant + e
    }

    fn now_system(&self) -> SystemTime {
        let e = *self.elapsed.lock().expect("FakeClock poisoned");
        self.base_system + e
    }

    fn now_unix_millis(&self) -> i64 {
        let now = self.now_system();
        now.duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    }
}
