//! A nonpersistent receipt shared by admission, transport and a requesting View.
use std::sync::atomic::{AtomicU8, AtomicU32, AtomicU64, Ordering};
use std::time::Instant;

pub struct PendingRequest {
    pub admitted: Instant,
    op: AtomicU64,
    phase: AtomicU8,
    created: AtomicU32,
}
impl PendingRequest {
    pub fn new(admitted: Instant) -> Self {
        Self {
            admitted,
            op: AtomicU64::new(0),
            phase: AtomicU8::new(0),
            created: AtomicU32::new(0),
        }
    }
    pub fn bind(&self, op: u64) {
        self.op.store(
            op.checked_add(1)
                .expect("reserved op sequence cannot reach u64::MAX"),
            Ordering::Release,
        );
    }
    pub fn op(&self) -> Option<u64> {
        match self.op.load(Ordering::Acquire) {
            0 => None,
            op => Some(op - 1),
        }
    }
    pub fn result(&self, created: Option<u32>) {
        if self.phase.load(Ordering::Acquire) != 0 {
            return;
        }
        self.created.store(created.unwrap_or(0), Ordering::Release);
        self.phase.store(1, Ordering::Release);
    }
    pub fn has_result(&self) -> bool {
        self.phase.load(Ordering::Acquire) == 1
    }
    pub fn finish(&self) {
        self.phase.store(2, Ordering::Release);
    }
    pub fn finished(&self) -> bool {
        self.phase.load(Ordering::Acquire) == 2
    }
    pub fn created(&self) -> Option<u32> {
        match self.created.load(Ordering::Acquire) {
            0 => None,
            id => Some(id),
        }
    }
}

/// One visible workspace's unresolved requests, evaluated with an explicit clock.
#[derive(Debug, PartialEq, Eq)]
pub struct WaitSummary {
    pub count: usize,
    pub elapsed: std::time::Duration,
    pub visible: bool,
    pub next_update: std::time::Duration,
}
impl WaitSummary {
    pub fn at(
        times: impl IntoIterator<Item = Instant>,
        now: Instant,
        threshold: std::time::Duration,
    ) -> Option<Self> {
        let mut oldest = None;
        let mut count = 0;
        for time in times {
            oldest = Some(oldest.map_or(time, |old: Instant| old.min(time)));
            count += 1;
        }
        let elapsed = now.saturating_duration_since(oldest?);
        let visible = elapsed >= threshold;
        let next_update = if visible {
            std::time::Duration::from_secs(1)
                - std::time::Duration::from_nanos(u64::from(elapsed.subsec_nanos()))
        } else {
            threshold - elapsed
        };
        Some(Self {
            count,
            elapsed,
            visible,
            next_update,
        })
    }
}
#[cfg(test)]
mod wait_tests {
    use super::*;
    use std::time::Duration;
    #[test]
    fn explicit_threshold_and_oldest_request_control_visibility_without_reset() {
        let start = Instant::now();
        let delay = Duration::from_millis(500);
        assert!(WaitSummary::at([], start, delay).is_none());
        let before = WaitSummary::at([start], start + Duration::from_millis(300), delay).unwrap();
        assert!(!before.visible);
        assert_eq!(before.next_update, Duration::from_millis(200));
        let boundary = WaitSummary::at([start], start + delay, delay).unwrap();
        assert!(boundary.visible);
        assert_eq!(boundary.elapsed.as_secs(), 0);
        assert_eq!(boundary.next_update, delay);
        let second = start + Duration::from_secs(2);
        let both =
            WaitSummary::at([start, second], start + Duration::from_millis(3200), delay).unwrap();
        assert_eq!(both.count, 2);
        assert_eq!(both.elapsed.as_secs(), 3);
        let successor = WaitSummary::at([second], start + Duration::from_secs(3), delay).unwrap();
        assert_eq!(successor.count, 1);
        assert_eq!(successor.elapsed.as_secs(), 1);
        assert!(WaitSummary::at([], start + Duration::from_secs(10), delay).is_none());
    }
}
