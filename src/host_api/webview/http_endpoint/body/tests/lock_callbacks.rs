//! Poison guards are released before actual watch notification and reentrant retirement.
use super::*;
use std::{
    future::Future,
    sync::{TryLockError, Weak},
    task::{Context, Poll, Wake, Waker},
};
struct ReentrantWake {
    source: Weak<BodySource>,
    replay: bool,
    blocked: AtomicBool,
    woke: AtomicBool,
    reentered: AtomicBool,
}
impl Wake for ReentrantWake {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.woke.store(true, Ordering::Release);
        let source = self.source.upgrade().unwrap();
        let blocked = if self.replay {
            matches!(source.replay.try_lock(), Err(TryLockError::WouldBlock))
        } else {
            matches!(source.delivery.try_lock(), Err(TryLockError::WouldBlock))
        };
        if blocked {
            self.blocked.store(true, Ordering::Release);
        } else if !self.reentered.swap(true, Ordering::AcqRel) {
            source.retire();
        }
    }
}
fn poison(source: &Arc<BodySource>, replay: bool) {
    let source = source.clone();
    assert!(
        std::thread::spawn(move || {
            if replay {
                let _guard = source.replay.lock().unwrap();
                panic!("injected replay poison");
            } else {
                let _guard = source.delivery.lock().unwrap();
                panic!("injected delivery poison");
            }
        })
        .join()
        .is_err()
    );
}
fn poison_wake(replay: bool) {
    let (sender, rx) = channel();
    let (mirror, view) = mirror(&sender, "http://remote.example:8080/post");
    let bridge = bridge(sender, &view);
    let source = BodySource::new(bridge.clone(), view, Arc::new(|| true));
    let wake = Arc::new(ReentrantWake {
        source: Arc::downgrade(&source),
        replay,
        blocked: AtomicBool::new(false),
        woke: AtomicBool::new(false),
        reentered: AtomicBool::new(false),
    });
    let waker = Waker::from(wake.clone());
    let mut receiver = source.stop.subscribe();
    let mut changed = Box::pin(receiver.changed());
    assert!(
        changed
            .as_mut()
            .poll(&mut Context::from_waker(&waker))
            .is_pending()
    );
    poison(&source, replay);
    if replay {
        assert!(matches!(source.lock(), Err(EndpointError::Body)));
    } else {
        source.retire_delivery();
    }
    assert!(wake.woke.load(Ordering::Acquire));
    assert!(
        !wake.blocked.load(Ordering::Acquire),
        "notification retained poisoned guard"
    );
    assert!(
        wake.reentered.load(Ordering::Acquire),
        "waker must actually reenter retirement"
    );
    assert!(matches!(
        changed.as_mut().poll(&mut Context::from_waker(&waker)),
        Poll::Ready(Ok(()))
    ));
    assert!(!source.current());
    assert_eq!(source.replay.is_poisoned(), replay);
    assert_eq!(source.delivery.is_poisoned(), !replay);
    bridge.drive(|| panic!("poison wake allocated an identity"), Err);
    assert!(rx.try_recv().is_err());
    drop(changed);
    drop(source);
    drop(bridge);
    drop(mirror);
}
#[test]
fn poisoned_delivery_wakes_reentrant_retirement_after_releasing_guard() {
    poison_wake(false);
}
#[test]
fn poisoned_replay_wakes_reentrant_retirement_after_releasing_guard() {
    poison_wake(true);
}
