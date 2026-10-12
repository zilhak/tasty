//! 수집 창과 재시도 대기에 쓰는 시계. 시험은 수동 시계로 시간을 직접 움직인다.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::poison::recover_mutex;

/// worker가 읽는 시계.
pub trait Clock: Send + Sync + 'static {
    fn now(&self) -> Instant;
    /// `deadline`을 기다리는 동안 한 번에 잠들 수 있는 최대 시간. 그 뒤 다시 확인한다.
    fn wait_for(&self, deadline: Instant) -> Duration;
    /// 재시도 사이의 대기.
    fn sleep(&self, d: Duration);
}

/// 실제 시계.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
    fn wait_for(&self, deadline: Instant) -> Duration {
        deadline.saturating_duration_since(Instant::now())
    }
    fn sleep(&self, d: Duration) {
        std::thread::sleep(d);
    }
}

static MANUAL_CLOCK_POISONED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// 시험용 시계. `advance`로만 시간이 흐르고 `sleep`은 그만큼 시간을 앞당긴다.
/// worker는 실제 시간 1ms마다 다시 확인하므로 `advance` 뒤 곧 새 시각을 본다.
#[derive(Debug)]
pub struct ManualClock {
    origin: Instant,
    offset: Mutex<Duration>,
}

impl Default for ManualClock {
    fn default() -> Self {
        Self {
            origin: Instant::now(),
            offset: Mutex::new(Duration::ZERO),
        }
    }
}

impl ManualClock {
    pub fn advance(&self, d: Duration) {
        *recover_mutex(self.offset.lock(), "manual clock", &MANUAL_CLOCK_POISONED) += d;
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Instant {
        self.origin + *recover_mutex(self.offset.lock(), "manual clock", &MANUAL_CLOCK_POISONED)
    }
    fn wait_for(&self, _deadline: Instant) -> Duration {
        Duration::from_millis(1)
    }
    fn sleep(&self, d: Duration) {
        self.advance(d);
    }
}
