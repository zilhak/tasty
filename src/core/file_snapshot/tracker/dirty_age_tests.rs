use super::*;
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
fn fixture() -> (
    tempfile::TempDir,
    Tracker,
    Arc<ManualClock>,
    Arc<Mutex<Vec<Receipt>>>,
) {
    let root = tempfile::tempdir().unwrap();
    let (_, document) =
        Document::load_with(
            root.path().join("registry.toml"),
            |_| Ok(toml::Table::new()),
        );
    let clock = Arc::new(ManualClock::default());
    let tracker = Tracker::loaded_with_clock("test:dirty-age", document, 7, clock.clone());
    let receipts = Arc::new(Mutex::new(Vec::new()));
    let ready = receipts.clone();
    tracker.install_sink(Arc::new(move |_| {
        let receipt = Receipt::default();
        ready.lock().unwrap().push(receipt.clone());
        Ok(Box::new(receipt))
    }));
    (root, tracker, clock, receipts)
}
fn finish(receipts: &Mutex<Vec<Receipt>>, index: usize, result: Result<(), String>) {
    *receipts.lock().unwrap()[index].result.lock().unwrap() = Some(result);
}
#[test]
fn terminal_failure_and_serialization_error_preserve_continuous_dirty_age() {
    let (_root, tracker, clock, receipts) = fixture();
    assert_eq!(tracker.status(), (7, 7, 0));
    assert_eq!(tracker.dirty_age_us(), None);
    let first = tracker.update_with_receipt(Ok(vec![1]), true);
    assert_eq!(tracker.dirty_age_us(), Some(0));
    clock.advance(Duration::from_micros(17));
    finish(&receipts, 0, Err("save outcome unknown".into()));
    assert_eq!(tracker.dirty_age_us(), Some(17));
    assert_eq!(tracker.dirty_age_us(), Some(17));
    assert_eq!(receipts.lock().unwrap()[0].polls.load(Ordering::Relaxed), 0);
    assert!(first.poll().is_none());
    assert_eq!(tracker.poll().len(), 1);
    assert!(first.poll().unwrap().is_err());
    assert_eq!(tracker.status(), (8, 7, 0));
    clock.advance(Duration::from_micros(23));
    let error = tracker.update_with_receipt(Err("serialization refused".into()), true);
    assert!(error.poll().unwrap().is_err());
    assert_eq!(tracker.dirty_age_us(), Some(40));
    tracker.retry_latest_before_shutdown();
    assert_eq!(tracker.status(), (9, 7, 0));
    assert_eq!(tracker.dirty_age_us(), Some(40));
    tracker.update_with_receipt(Ok(vec![2]), true);
    clock.advance(Duration::from_micros(5));
    finish(&receipts, 1, Ok(()));
    assert_eq!(tracker.dirty_age_us(), Some(45));
    tracker.poll();
    assert_eq!(tracker.status(), (10, 10, 0));
    assert_eq!(tracker.dirty_age_us(), None);
    clock.advance(Duration::from_micros(99));
    tracker.update_with_receipt(Ok(vec![3]), true);
    assert_eq!(tracker.dirty_age_us(), Some(0));
}
#[test]
fn older_success_does_not_reset_age_and_latest_retry_can_clear_it() {
    let (_root, tracker, clock, receipts) = fixture();
    tracker.update_with_receipt(Ok(vec![1]), false);
    clock.advance(Duration::from_micros(5));
    tracker.update_with_receipt(Ok(vec![2]), false);
    clock.advance(Duration::from_micros(7));
    finish(&receipts, 0, Ok(()));
    tracker.poll();
    assert_eq!(tracker.status(), (9, 8, 1));
    assert_eq!(tracker.dirty_age_us(), Some(12));
    clock.advance(Duration::from_micros(11));
    finish(&receipts, 1, Err("replacement failed".into()));
    assert!(tracker.poll().is_empty());
    assert_eq!(tracker.status(), (9, 8, 0));
    assert_eq!(tracker.dirty_age_us(), Some(23));
    tracker.retry_latest_before_shutdown();
    assert_eq!(tracker.dirty_age_us(), Some(23));
    clock.advance(Duration::from_micros(3));
    finish(&receipts, 2, Ok(()));
    assert_eq!(tracker.dirty_age_us(), Some(26));
    tracker.poll();
    assert_eq!(tracker.dirty_age_us(), None);
    assert_eq!(tracker.status(), (9, 9, 0));
}
#[test]
fn a_late_old_receipt_cannot_clear_a_new_dirty_period_after_latest_confirmation() {
    let (_root, tracker, clock, receipts) = fixture();
    tracker.update_with_receipt(Ok(vec![1]), true);
    tracker.update_with_receipt(Ok(vec![2]), true);
    finish(&receipts, 1, Ok(()));
    tracker.poll();
    assert_eq!(tracker.status(), (9, 9, 1));
    assert_eq!(tracker.dirty_age_us(), None);
    clock.advance(Duration::from_micros(40));
    tracker.update_with_receipt(Ok(vec![3]), true);
    clock.advance(Duration::from_micros(13));
    finish(&receipts, 0, Ok(()));
    tracker.poll();
    assert_eq!(tracker.status(), (10, 9, 1));
    assert_eq!(tracker.dirty_age_us(), Some(13));
    finish(&receipts, 2, Err("known failure".into()));
    tracker.poll();
    assert_eq!(tracker.status(), (10, 9, 0));
    assert_eq!(tracker.finish_shutdown(), Some((10, 9)));
    assert_eq!(tracker.dirty_age_us(), Some(13));
}
