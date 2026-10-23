use super::*;

#[test]
fn query_is_nonconsuming_before_during_and_after_actual_join() {
    let mut rig = rig(limits(), Gate::closed());
    assert_eq!(
        rig.writer.shutdown_observation(),
        ShutdownObservation::NotStarted
    );
    let receipt = rig.handle.submit(req(1)).unwrap();
    rig.writer.begin_shutdown();
    assert!(!rig.writer.worker_ended_unexpectedly());
    assert_eq!(
        rig.writer.shutdown_observation(),
        ShutdownObservation::Draining
    );
    assert_eq!(rig.writer.poll_shutdown(), 1);
    assert_eq!(
        rig.writer.shutdown_observation(),
        ShutdownObservation::Draining
    );
    rig.gate.open();
    eventually("join", || rig.writer.poll_shutdown() == 0);
    assert!(!rig.writer.worker_ended_unexpectedly());
    let observed = rig.writer.shutdown_observation();
    assert!(matches!(
        observed,
        ShutdownObservation::Joined {
            panicked: false,
            flush: Ok(_)
        }
    ));
    assert!(matches!(receipt.try_take(), Some(Completion::Done(Ok(1)))));
    for _ in 0..3 {
        assert_eq!(rig.writer.shutdown_observation(), observed);
        assert_eq!(rig.writer.poll_shutdown(), 0);
    }
}

#[test]
fn failed_and_unknown_requests_preserve_unclean_final_flush_after_join() {
    for script in [Script::Fail, Script::Unknown] {
        let mut rig = rig(limits(), Gate::closed());
        rig.probe.script.lock().unwrap().push_back(script);
        let receipt = rig.handle.submit(req(1)).unwrap();
        rig.writer.begin_shutdown();
        rig.gate.open();
        eventually("join", || rig.writer.poll_shutdown() == 0);
        assert!(matches!(
            rig.writer.shutdown_observation(),
            ShutdownObservation::Joined {
                panicked: false,
                flush: Err(FlushFailure::NotClean { failed: 1, .. }),
            }
        ));
        assert!(matches!(
            receipt.try_take(),
            Some(Completion::Failed(_) | Completion::Unknown(_))
        ));
    }
}

fn panicking_notifier() -> Rig {
    let mut rig = rig(limits(), Gate::closed());
    // Dispose the unused real worker, then own a worker that panics outside executor recovery.
    rig.writer.begin_shutdown();
    eventually("unused worker", || rig.writer.poll_shutdown() == 0);
    rig.writer = Writer::spawn(
        "owned-panic",
        Exec {
            probe: rig.probe.clone(),
            gate: rig.gate.clone(),
        },
        limits(),
        rig.clock.clone(),
        Arc::new(|| panic!("owned notifier worker fault")),
    )
    .unwrap();
    rig.handle = rig.writer.handle();
    rig
}

#[test]
fn actual_worker_panic_before_final_flush_is_joined_with_lost_flush() {
    let mut rig = panicking_notifier();
    let receipt = rig.handle.submit(req(1)).unwrap();
    rig.writer.begin_shutdown();
    rig.gate.open();
    eventually("panic join", || rig.writer.poll_shutdown() == 0);
    assert_eq!(
        rig.writer.shutdown_observation(),
        ShutdownObservation::Joined {
            panicked: true,
            flush: Err(FlushFailure::Lost),
        }
    );
    assert!(matches!(receipt.try_take(), Some(Completion::Done(Ok(1)))));
}

#[test]
fn actual_worker_panic_after_successful_flush_is_still_panicked() {
    let mut rig = panicking_notifier();
    rig.writer.begin_shutdown();
    eventually("panic join", || rig.writer.poll_shutdown() == 0);
    assert!(matches!(
        rig.writer.shutdown_observation(),
        ShutdownObservation::Joined {
            panicked: true,
            flush: Ok(_),
        }
    ));
}

#[test]
fn missing_final_receipt_is_unknown_never_clean_success() {
    let mut rig = rig(limits(), Gate::default().into());
    rig.writer.begin_shutdown();
    eventually("worker finished", || {
        rig.writer.worker.as_ref().unwrap().is_finished()
    });
    drop(rig.writer.final_flush.take());
    assert_eq!(rig.writer.poll_shutdown(), 0);
    assert_eq!(
        rig.writer.shutdown_observation(),
        ShutdownObservation::Joined {
            panicked: false,
            flush: Err(FlushFailure::Lost),
        }
    );
}

#[test]
fn worker_ending_before_shutdown_is_distinct_from_requested_join() {
    let mut rig = panicking_notifier();
    let external_flush = rig.handle.flush(FlushPoint::START).unwrap();
    eventually("unexpected worker panic", || {
        rig.writer.worker.as_ref().unwrap().is_finished()
    });
    assert!(rig.writer.worker_ended_unexpectedly());
    assert_eq!(
        rig.writer.shutdown_observation(),
        ShutdownObservation::NotStarted
    );
    assert!(external_flush.try_take().unwrap().is_ok());
    assert_eq!(rig.writer.poll_shutdown(), 0);
    assert_eq!(
        rig.writer.shutdown_observation(),
        ShutdownObservation::Joined {
            panicked: true,
            flush: Err(FlushFailure::Lost),
        }
    );
    assert!(!rig.writer.worker_ended_unexpectedly());
}
