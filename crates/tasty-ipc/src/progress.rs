//! Cached queued main-loop obligations, separate from admission, execution and reply ownership.
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, LazyLock, Mutex, Weak};
use std::time::Instant;

#[derive(Default)]
struct State {
    count: u64,
    since: Option<Instant>,
    generation: u64,
}
#[derive(Default)]
struct Ledger {
    state: Mutex<State>,
    poisoned: AtomicBool,
}
static QUEUED: LazyLock<Arc<Ledger>> = LazyLock::new(|| Arc::new(Ledger::default()));

/// A continuous nonempty queue-obligation period, not the oldest individual request.
#[derive(Clone, Copy, Debug)]
pub struct PendingWork {
    pub count: u64,
    pub since: Instant,
    pub generation: u64,
}
impl Ledger {
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        tasty_utils::poison::recover_mutex(self.state.lock(), "IPC queued progress", &self.poisoned)
    }
    fn snapshot(&self) -> Option<PendingWork> {
        let state = self.lock();
        state.since.map(|since| PendingWork {
            count: state.count,
            since,
            generation: state.generation,
        })
    }
}
/// Copies fixed metadata only; never consumes a request or waits for an executor/store.
pub fn pending_main_loop_work() -> Option<PendingWork> {
    QUEUED.snapshot()
}

pub(super) struct Work {
    state: AtomicU8,
    ledger: Arc<Ledger>,
}
impl std::fmt::Debug for Work {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QueuedWork")
            .field("active", &self.state)
            .finish_non_exhaustive()
    }
}
impl Work {
    pub(super) fn start(&self) {
        self.start_at(Instant::now());
    }
    fn start_at(&self, now: Instant) {
        let mut state = self.ledger.lock();
        if self.state.load(Ordering::Relaxed) != 0 {
            return;
        }
        if state.count == 0 {
            state.since = Some(now);
            state.generation = state.generation.wrapping_add(1);
        }
        state.count += 1;
        self.state.store(1, Ordering::Release);
    }
    pub(super) fn finish(&self) {
        if self.state.load(Ordering::Acquire) == 2 {
            return;
        }
        let mut state = self.ledger.lock();
        if self.state.swap(2, Ordering::AcqRel) != 1 {
            return;
        }
        state.count -= 1;
        if state.count == 0 {
            state.since = None;
        }
    }
}
#[derive(Debug)]
pub(super) struct Ticket(Arc<Work>);
impl Ticket {
    pub(super) fn new() -> Self {
        Self::inactive(QUEUED.clone())
    }
    fn inactive(ledger: Arc<Ledger>) -> Self {
        Self(Arc::new(Work {
            state: AtomicU8::new(0),
            ledger,
        }))
    }
    #[cfg(test)]
    fn on(ledger: Arc<Ledger>, now: Instant) -> Self {
        let ticket = Self::inactive(ledger);
        ticket.0.start_at(now);
        ticket
    }
    pub(super) fn weak(&self) -> Weak<Work> {
        Arc::downgrade(&self.0)
    }
    pub(super) fn finish(&self) {
        self.0.finish();
    }
    #[cfg(test)]
    pub(super) fn is_pending(&self) -> bool {
        self.0.state.load(Ordering::Acquire) == 1
    }
}
impl Drop for Ticket {
    fn drop(&mut self) {
        self.finish();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    #[test]
    fn concurrent_work_preserves_busy_period_until_last_obligation_settles() {
        let ledger = Arc::new(Ledger::default());
        let first_at = Instant::now();
        let a = Ticket::on(ledger.clone(), first_at);
        let first = ledger.snapshot().unwrap();
        let b = Ticket::on(ledger.clone(), first_at + Duration::from_secs(10));
        a.finish();
        drop(a);
        let later = ledger.snapshot().unwrap();
        assert_eq!(later.count, 1);
        assert_eq!(later.since, first_at);
        assert_eq!(later.generation, first.generation);
        drop(b);
        assert!(ledger.snapshot().is_none());
        let c = Ticket::on(ledger.clone(), first_at + Duration::from_secs(20));
        assert_ne!(ledger.snapshot().unwrap().generation, first.generation);
        assert_eq!(
            ledger.snapshot().unwrap().since,
            first_at + Duration::from_secs(20)
        );
        drop(c);
        assert!(ledger.snapshot().is_none());
    }
    #[test]
    fn weak_waiter_does_not_keep_dropped_command_pending() {
        let ledger = Arc::new(Ledger::default());
        let ticket = Ticket::on(ledger.clone(), Instant::now());
        let waiter = ticket.weak();
        drop(ticket);
        assert!(waiter.upgrade().is_none());
        assert!(ledger.snapshot().is_none());
    }
    #[test]
    fn dequeue_or_withdraw_before_post_send_marker_never_revives_work() {
        let ledger = Arc::new(Ledger::default());
        let ticket = Ticket::inactive(ledger.clone());
        assert!(ledger.snapshot().is_none());
        ticket.finish();
        ticket.0.start();
        assert!(ledger.snapshot().is_none());
        let ticket = Ticket::inactive(ledger.clone());
        ticket.0.start();
        assert_eq!(ledger.snapshot().unwrap().count, 1);
        ticket.finish();
        ticket.0.start();
        assert!(ledger.snapshot().is_none());
    }
    #[test]
    fn racing_completion_and_drop_settle_once() {
        let ledger = Arc::new(Ledger::default());
        let ticket = Ticket::on(ledger.clone(), Instant::now());
        let work = ticket.0.clone();
        let completion = std::thread::spawn(move || {
            for _ in 0..100 {
                work.finish();
            }
        });
        drop(ticket);
        completion.join().unwrap();
        assert!(ledger.snapshot().is_none());
    }
}
