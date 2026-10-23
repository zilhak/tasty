//! Actual credit rejection leaf and ordered admission path under blocked Finished delivery.
use super::*;
use crate::runtime::journal_product::{JournalMetrics, Request, observability::Outcome};
use std::time::Duration;
use tasty_utils::batch_writer::ManualClock;

fn admission() -> Admission {
    Admission {
        key: None,
        original_digest: vec![1],
        actor: "test".into(),
        origin: "test".into(),
        causation_id: None,
    }
}

fn credit_state(waiters: usize) -> (HashMap<u64, Pending>, VecDeque<(u64, Admission)>) {
    let pending = HashMap::from([(
        0,
        Pending {
            disk_credit: 1,
            admission: admission(),
            followers: vec![],
            reservations: vec![],
            inputs: vec![],
        },
    )]);
    let waiting = (1..=waiters as u64)
        .map(|ticket| (ticket, admission()))
        .collect();
    (pending, waiting)
}

fn rejection(ticket: u64) -> Completion {
    Completion::Finished {
        ticket,
        result: Err(JournalError::QueueFull("occupied")),
    }
}

fn assert_rejection(completion: Completion) {
    assert!(matches!(
        completion,
        Completion::Finished {
            ticket: 99,
            result: Err(JournalError::QueueFull(
                "journal admission capacity exhausted"
            )),
        }
    ));
}

fn blocked_rejection(ordered: bool, disconnect: bool) {
    let clock = Arc::new(ManualClock::default());
    let metrics = Arc::new(JournalMetrics::with_clock(clock.clone()));
    let observation = metrics.prepare(17);
    metrics.accept(observation.id());
    observation.start();
    if !ordered {
        observation.execution_finished();
    }
    let (sender, receiver) = mpsc::sync_channel(1);
    sender.send(rejection(0)).unwrap();
    let (entered, ready) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        let (pending, mut waiting) = credit_state(QUEUE_CAPACITY - 1);
        let send = |completion| {
            entered.send(()).unwrap();
            sender.send(completion).is_ok()
        };
        let connected = if ordered {
            matches!(
                admission::ordered(
                    Request {
                        ticket: 99,
                        work: Work::Admit(admission())
                    },
                    &pending,
                    &mut waiting,
                    false,
                    &send,
                    &observation
                ),
                admission::Order::Deferred
            )
        } else {
            wait_for_credit(&pending, &mut waiting, 99, admission(), &send, &observation)
        };
        if connected {
            observation.finish(Outcome::waiting(QUEUE_CAPACITY - 1, waiting.len()));
        }
        (connected, waiting.len())
    });
    ready.recv_timeout(Duration::from_secs(30)).unwrap();
    clock.advance(Duration::from_micros(1000));
    let blocked = metrics.snapshot();
    if !disconnect {
        receiver.recv().unwrap();
        assert_rejection(receiver.recv().unwrap());
    }
    drop(receiver);
    let (connected, waiting) = thread.join().unwrap();
    let done = metrics.snapshot();
    // Always drain/disconnect and join before assertions, including on the RED implementation.
    assert_eq!(connected, !disconnect);
    assert_eq!(waiting, QUEUE_CAPACITY - 1);
    assert_eq!(
        blocked.work.pending_items, 0,
        "a determined rejection must settle before Finished send"
    );
    assert_eq!(blocked.work.inflight_items, 0);
    assert_eq!(blocked.work.pending_bytes_estimate, 0);
    assert_eq!(blocked.oldest_pending_age_us, None);
    assert_eq!(blocked.work.handled_error, 1);
    assert_eq!(blocked.work.deferred, 0);
    assert_eq!(blocked.work.lost, 0);
    assert_eq!(blocked.execution.samples, 1);
    assert_eq!(blocked.execution.sum_us, 0);
    assert_eq!(blocked.completion.samples, 1);
    assert_eq!(blocked.completion.sum_us, 0);
    assert_eq!(done.work, blocked.work);
    assert_eq!(done.completion, blocked.completion);
    assert_eq!(done.execution, blocked.execution);
}

#[test]
fn credit_full_finished_backpressure_is_not_pending_or_completion_time() {
    blocked_rejection(false, false);
}

#[test]
fn ordered_credit_full_finished_backpressure_is_not_pending_or_completion_time() {
    blocked_rejection(true, false);
}

#[test]
fn credit_full_finished_disconnect_is_still_a_handled_rejection() {
    blocked_rejection(false, true);
}

#[test]
fn ordered_credit_full_finished_disconnect_is_still_a_handled_rejection() {
    blocked_rejection(true, true);
}

#[test]
fn available_credit_wait_keeps_fifo_and_defers_without_a_reply() {
    for ordered in [false, true] {
        let metrics = Arc::new(JournalMetrics::default());
        let observation = metrics.prepare(17);
        observation.start();
        let (pending, mut waiting) = credit_state(QUEUE_CAPACITY - 2);
        let send = |_| panic!("a queued credit waiter must not emit Finished");
        if ordered {
            assert!(matches!(
                admission::ordered(
                    Request {
                        ticket: 99,
                        work: Work::Admit(admission())
                    },
                    &pending,
                    &mut waiting,
                    false,
                    &send,
                    &observation
                ),
                admission::Order::Deferred
            ));
        } else {
            assert!(wait_for_credit(
                &pending,
                &mut waiting,
                99,
                admission(),
                &send,
                &observation
            ));
        }
        observation.finish(Outcome::waiting(QUEUE_CAPACITY - 2, waiting.len()));
        assert_eq!(waiting.front().unwrap().0, 1);
        assert_eq!(waiting.back().unwrap().0, 99);
        assert_eq!(waiting.len(), QUEUE_CAPACITY - 1);
        let s = metrics.snapshot();
        assert_eq!(s.work.deferred, 1);
        assert_eq!(s.work.handled_error, 0);
        assert_eq!(s.work.pending_items, 0);
    }
}
