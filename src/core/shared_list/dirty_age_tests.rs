use super::*;
use crate::core::port_favorites::PortFavorites;
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};
use tasty_utils::batch_writer::ManualClock;

#[derive(Clone, Default)]
struct Receipt {
    result: Arc<Mutex<Option<Result<(), String>>>>,
    polls: Arc<AtomicUsize>,
}
impl PendingSave for Receipt {
    fn poll(&self) -> Option<Result<(), String>> {
        self.polls.fetch_add(1, Ordering::Relaxed);
        self.result.lock().unwrap().take()
    }
}
#[test]
fn favorites_dirty_age_survives_updates_failure_and_older_success_without_consuming_receipts() {
    let root = tempfile::tempdir().unwrap();
    let clock = Arc::new(ManualClock::default());
    let source = SharedList::<PortFavorites>::load_at_with_clock(
        root.path().join("favorites.toml"),
        clock.clone(),
    );
    let receipts = Arc::new(Mutex::new(Vec::new()));
    let ready = receipts.clone();
    source.install_sink(Arc::new(move |_| {
        let receipt = Receipt::default();
        ready.lock().unwrap().push(receipt.clone());
        Ok(Box::new(receipt))
    }));
    let update =
        |port| source.update(|list| list.add("127.0.0.1".parse().unwrap(), port, "age".into()));
    let finish = |index: usize, result| {
        *receipts.lock().unwrap()[index].result.lock().unwrap() = Some(result)
    };
    assert_eq!(source.dirty_age_us(), None);
    update(1234);
    assert_eq!(source.dirty_age_us(), Some(0));
    clock.advance(Duration::from_micros(8));
    update(1235);
    clock.advance(Duration::from_micros(7));
    finish(0, Ok(()));
    assert_eq!(source.dirty_age_us(), Some(15));
    assert_eq!(source.dirty_age_us(), Some(15));
    assert_eq!(receipts.lock().unwrap()[0].polls.load(Ordering::Relaxed), 0);
    source.poll();
    assert_eq!(source.save_status(), (2, 1, 1));
    assert_eq!(source.dirty_age_us(), Some(15));
    finish(1, Err("save outcome unknown".into()));
    source.poll();
    assert_eq!(source.save_status(), (2, 1, 0));
    clock.advance(Duration::from_micros(5));
    assert_eq!(source.dirty_age_us(), Some(20));
    source.retry_latest_before_shutdown();
    assert_eq!(source.dirty_age_us(), Some(20));
    finish(2, Ok(()));
    source.poll();
    assert_eq!(source.dirty_age_us(), None);
    assert_eq!(source.save_status(), (2, 2, 0));
    clock.advance(Duration::from_micros(100));
    update(1236);
    assert_eq!(source.dirty_age_us(), Some(0));
}
