//! worker의 완료 알림을 소유자의 깨우기 하나로 묶는다.
//!
//! worker는 묶음·flush마다 알림을 부른다. 소유자가 깨어나 결과를 거두기 전에 온 알림은 이미 보낸
//! 깨우기에 합친다. 소유자는 결과를 거두기 직전에 [`WakeLatch::acknowledge`]를 불러 그 뒤의 알림이
//! 새 깨우기를 보내게 한다. 결과를 놓치는 경우는 없다: 확인 뒤 끝난 묶음은 다시 깨우고, 확인과
//! 거두기 사이에 끝난 묶음은 이번에 함께 거둔 뒤 남는 깨우기 하나가 빈 회차가 된다.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

pub struct WakeLatch {
    pending: AtomicBool,
    wake: Arc<dyn Fn() + Send + Sync>,
    notified: AtomicU64,
    woken: AtomicU64,
}

/// 알림 수와 실제로 보낸 깨우기 수의 누계.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WakeCounts {
    pub notified: u64,
    pub woken: u64,
}

impl WakeLatch {
    pub fn new(wake: Arc<dyn Fn() + Send + Sync>) -> Arc<Self> {
        Arc::new(Self {
            pending: AtomicBool::new(false),
            wake,
            notified: AtomicU64::new(0),
            woken: AtomicU64::new(0),
        })
    }

    /// worker에 넘길 알림 함수.
    pub fn notifier(self: &Arc<Self>) -> Arc<dyn Fn() + Send + Sync> {
        let latch = self.clone();
        Arc::new(move || latch.notify())
    }

    fn notify(&self) {
        self.notified.fetch_add(1, Ordering::Relaxed);
        if !self.pending.swap(true, Ordering::AcqRel) {
            self.woken.fetch_add(1, Ordering::Relaxed);
            (self.wake)();
        }
    }

    /// 소유자가 결과를 거두기 직전에 부른다.
    pub fn acknowledge(&self) {
        self.pending.store(false, Ordering::Release);
    }

    pub fn counts(&self) -> WakeCounts {
        WakeCounts {
            notified: self.notified.load(Ordering::Relaxed),
            woken: self.woken.load(Ordering::Relaxed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notices_before_the_owner_acknowledges_share_one_wake() {
        let wakes = Arc::new(AtomicU64::new(0));
        let seen = wakes.clone();
        let latch = WakeLatch::new(Arc::new(move || {
            seen.fetch_add(1, Ordering::Relaxed);
        }));
        let notify = latch.notifier();
        for _ in 0..5 {
            notify();
        }
        assert_eq!(wakes.load(Ordering::Relaxed), 1);
        latch.acknowledge();
        notify();
        notify();
        assert_eq!(wakes.load(Ordering::Relaxed), 2);
        assert_eq!(
            latch.counts(),
            WakeCounts {
                notified: 7,
                woken: 2
            }
        );
    }
}
