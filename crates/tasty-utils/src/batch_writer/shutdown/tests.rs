use super::*;

#[test]
fn absent_cut_and_join_are_not_zero_duration_success() {
    let mut timer = ShutdownTimer::default();
    let now = Instant::now();
    timer.joined(now);
    assert_eq!(timer.snapshot(now), None);
}

#[test]
fn first_cut_survives_repeated_stop_and_join_freezes_after_actual_join() {
    let mut timer = ShutdownTimer::default();
    let start = Instant::now();
    timer.start(start);
    timer.start(start + Duration::from_secs(4));
    assert_eq!(
        timer.snapshot(start + Duration::from_secs(5)),
        Some(ShutdownTiming {
            elapsed: Duration::from_secs(5),
            joined_after: None,
        })
    );
    timer.joined(start + Duration::from_secs(6));
    timer.start(start + Duration::from_secs(7));
    timer.joined(start + Duration::from_secs(8));
    assert_eq!(
        timer.snapshot(start + Duration::from_secs(20)),
        Some(ShutdownTiming {
            elapsed: Duration::from_secs(6),
            joined_after: Some(Duration::from_secs(6)),
        })
    );
}

#[test]
fn immediate_join_is_measured_zero_and_repeated_reads_do_not_start_or_consume_it() {
    let mut timer = ShutdownTimer::default();
    let start = Instant::now();
    timer.start(start);
    timer.joined(start);
    for _ in 0..3 {
        assert_eq!(
            timer.snapshot(start),
            Some(ShutdownTiming {
                elapsed: Duration::ZERO,
                joined_after: Some(Duration::ZERO),
            })
        );
    }
}
