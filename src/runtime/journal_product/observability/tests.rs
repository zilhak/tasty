use super::super::SubmitError;
use super::*;
use std::time::Duration;
use tasty_utils::batch_writer::ManualClock;

#[test]
fn manual_clock_separates_queue_execution_ack_and_nonconsuming_pending_age() {
    let clock = Arc::new(ManualClock::default());
    let m = Arc::new(JournalMetrics::with_clock(clock.clone()));
    let o = m.prepare(31);
    m.accept(o.id());
    clock.advance(Duration::from_micros(10));
    let queued = m.snapshot();
    assert_eq!(queued.oldest_pending_age_us, Some(10));
    assert_eq!(queued.work.queued_items, 1);
    assert_eq!(queued.work.inflight_items, 0);
    assert_eq!(queued.work.pending_bytes_estimate, 31);
    assert_eq!(queued, m.snapshot());
    o.start();
    clock.advance(Duration::from_micros(22));
    o.execution_finished();
    m.publication(Some(3), None);
    clock.advance(Duration::from_micros(45));
    m.publication(Some(3), Some(1));
    let waiting = m.snapshot();
    assert_eq!(waiting.work.inflight_items, 1);
    assert_eq!(waiting.execution.sum_us, 22);
    assert_eq!(waiting.queue_wait.sum_us, 10);
    assert_eq!(waiting.unpublished_age_us, Some(45));
    o.finish(Outcome::Error);
    drop(o);
    let failed = m.snapshot();
    assert_eq!(failed.work.handled_error, 1);
    assert_eq!(failed.work.pending_items, 0);
    assert_eq!(failed.oldest_pending_age_us, None);
    assert_eq!(failed.completion.sum_us, 77);
    assert_eq!(failed.unpublished_age_us, Some(45));
    clock.advance(Duration::from_micros(5));
    m.publication(Some(3), Some(3));
    assert_eq!(m.snapshot().unpublished_age_us, None);
}

#[test]
fn failed_send_receiver_loss_and_receiver_before_sender_do_not_double_admit() {
    let m = Arc::new(JournalMetrics::default());
    let refused = m.prepare(900);
    m.reject(Some(refused.id()), &SubmitError::Busy);
    drop(refused);
    assert_eq!(m.snapshot().work.admitted, 0);
    assert_eq!(m.snapshot().work.lost, 0);
    assert_eq!(m.snapshot().completion.samples, 0);
    let lost = m.prepare(7);
    let id = lost.id();
    drop(lost);
    m.accept(id);
    assert_eq!(m.snapshot().work.lost, 1);
    assert_eq!(m.snapshot().work.pending_items, 0);
    let quick = m.prepare(11);
    let id = quick.id();
    quick.start();
    quick.finish(Outcome::Ok);
    drop(quick);
    m.accept(id);
    let s = m.snapshot();
    assert_eq!(s.work.admitted, 2);
    assert_eq!(s.work.admitted_bytes_estimate, 18);
    assert_eq!(s.work.handled_ok, 1);
    assert_eq!(s.work.rejected_busy, 1);
    assert_eq!(s.work.pending_items, 0);
    assert_eq!(s.observation_overflow, 0);
}

#[test]
fn fixed_slots_are_released_and_deferred_is_not_dirty_or_pending_work() {
    let clock = Arc::new(ManualClock::default());
    let m = Arc::new(JournalMetrics::with_clock(clock.clone()));
    for _ in 0..1000 {
        let o = m.prepare(3);
        m.accept(o.id());
        o.start();
        o.finish(Outcome::Deferred);
    }
    m.admissions(2, 1, 13);
    let s = m.snapshot();
    assert_eq!(s.work.deferred, 1000);
    assert_eq!(s.work.pending_items, 0);
    assert_eq!(s.oldest_pending_age_us, None);
    assert_eq!(s.open_admissions, 2);
    assert_eq!(s.waiting_admissions, 1);
    assert_eq!(s.admission_bytes_estimate, 13);
    assert_eq!(s.observation_overflow, 0);
    let start = m.batch_start(clock.now());
    clock.advance(Duration::from_micros(32));
    m.batch_finished(start, 3, true, 2, 99);
    let s = m.snapshot();
    assert_eq!(s.batches, 1);
    assert_eq!(s.batch_requests, 3);
    assert_eq!(s.batch_bytes_estimate, 99);
    assert_eq!(s.batch_sizes[2], 1);
    assert_eq!(s.batch_errors, 1);
    assert_eq!(s.batch_execution.sum_us, 32);
    assert_eq!(s.group_revision_retries, 2);
    assert!(s.store.is_none());
}

#[test]
fn unavailable_publication_cut_and_full_credit_wait_are_not_fabricated_success() {
    let clock = Arc::new(ManualClock::default());
    let m = Arc::new(JournalMetrics::with_clock(clock.clone()));
    assert!(!m.snapshot().publication_known);
    m.publication(Some(9), Some(3));
    clock.advance(Duration::from_micros(50));
    assert_eq!(m.snapshot().unpublished_age_us, Some(50));
    m.publication_unknown();
    assert!(!m.snapshot().publication_known);
    assert_eq!(m.snapshot().unpublished_age_us, None);
    let refused = m.prepare(2);
    m.accept(refused.id());
    refused.start();
    refused.finish(Outcome::waiting(64, 64));
    assert_eq!(m.snapshot().work.handled_error, 1);
    assert_eq!(m.snapshot().work.deferred, 0);
}
