use super::*;

#[test]
fn waits_until_the_deadline_then_gives_up_once() {
    let mut drain = WriterDrain::default();
    let start = Instant::now();
    assert!(drain.keep_waiting(1, start, "x"));
    assert!(drain.keep_waiting(1, start + WRITER_DRAIN_TIMEOUT / 2, "x"));
    // 기한은 첫 대기에서 정해지고 뒤의 대기가 늘리지 않는다.
    assert!(!drain.keep_waiting(1, start + WRITER_DRAIN_TIMEOUT, "x"));
    assert!(
        !drain.keep_waiting(1, start, "x"),
        "a given-up drain never waits again"
    );
}

#[test]
fn a_joined_writer_never_waits() {
    let mut drain = WriterDrain::default();
    assert!(!drain.keep_waiting(0, Instant::now(), "x"));
}

fn report(
    observation: tasty_utils::batch_writer::ShutdownObservation,
    elapsed: Duration,
    joined: bool,
) -> tasty_utils::batch_writer::ShutdownReport {
    tasty_utils::batch_writer::ShutdownReport {
        observation,
        timing: Some(tasty_utils::batch_writer::ShutdownTiming {
            elapsed,
            joined_after: joined.then_some(elapsed),
        }),
    }
}

#[test]
fn observation_does_not_restart_deadline_and_keeps_timeout_distinct_from_late_join() {
    use tasty_utils::batch_writer::{FlushPoint, ShutdownObservation};
    let start = Instant::now();
    let mut drain = WriterDrain::default();
    for seconds in [0, 2, 4] {
        assert!(drain.observe_writer(
            1,
            Some(report(
                ShutdownObservation::Draining,
                Duration::from_secs(seconds),
                false
            )),
            "test",
            start + Duration::from_secs(seconds),
            "timeout"
        ));
    }
    assert!(!drain.observe_writer(
        1,
        Some(report(
            ShutdownObservation::Draining,
            Duration::from_secs(5),
            false
        )),
        "test",
        start + Duration::from_secs(5),
        "timeout"
    ));
    assert!(drain.gave_up && drain.reported);
    let deadline = drain.deadline;
    assert!(!drain.observe_writer(
        0,
        Some(report(
            ShutdownObservation::Joined {
                panicked: false,
                flush: Ok(FlushPoint::START)
            },
            Duration::from_secs(6),
            true
        )),
        "test",
        start + Duration::from_secs(6),
        "timeout"
    ));
    assert_eq!(drain.deadline, deadline);
    assert!(drain.gave_up);
}

#[test]
fn configured_immediate_and_unconfigured_never_start_a_timeout() {
    use tasty_utils::batch_writer::{FlushPoint, ShutdownObservation};
    for observation in [
        None,
        Some(report(
            ShutdownObservation::Joined {
                panicked: false,
                flush: Ok(FlushPoint::START),
            },
            Duration::ZERO,
            true,
        )),
    ] {
        let mut drain = WriterDrain::default();
        assert!(!drain.observe_writer(0, observation, "test", Instant::now(), "timeout"));
        assert!(drain.deadline.is_none());
        assert!(drain.reported && !drain.gave_up);
    }
}
