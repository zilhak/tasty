use super::*;
use std::time::Duration;

#[test]
fn bounded_raw_samples_count_every_overflow_and_keep_metric_populations_separate() {
    let start = Instant::now();
    let mut c = Capture::new(start, 2);
    c.record(1, false, start, start + Duration::from_micros(3), None);
    c.record(0, true, start, start, Some(400));
    let json = serde_json::to_value(&c).unwrap();
    assert_eq!(json["samples"][0]["end_ns"], 3000);
    assert!(json["samples"][0]["wake_lateness_ns"].is_null());
    assert_eq!(json["samples"][1]["wake_lateness_ns"], 400);
    c.record(4, true, start, start, None);
    c.record(5, true, start, start, None);
    assert_eq!(c.dropped, 2);
    assert_eq!(c.seen, 4);
    assert_eq!(c.samples.len(), 2);
    assert_eq!(c.samples[0].seq, 3);
    assert_eq!(c.samples[1].seq, 4);
    assert!(c.samples.iter().all(|sample| sample.shutdown));
}

#[test]
fn owned_guards_cover_nested_callbacks_and_early_return_without_clamping_outer_start() {
    CAPTURE.with(|c| *c.borrow_mut() = None);
    let outer = Guard::started(1, false, Instant::now());
    let inner = Guard::started(4, true, Instant::now());
    drop(inner);
    drop(outer);
    let capture = CAPTURE.with(|c| c.borrow_mut().take().unwrap());
    assert_eq!(capture.samples.len(), 2);
    assert_eq!(capture.samples[1].start_ns, 0);
    assert!(capture.samples[0].shutdown);
    assert!(capture.samples[1].end_ns >= capture.samples[0].end_ns);
    fn returns_early(early: bool) {
        let _guard = Guard::started(5, true, Instant::now());
        if early {
            return;
        }
        panic!("expected the early return");
    }
    returns_early(true);
    assert_eq!(
        CAPTURE.with(|c| c.borrow_mut().take().unwrap().samples.len()),
        1
    );
}

#[test]
fn disabled_guard_does_not_allocate_or_record() {
    CAPTURE.with(|c| *c.borrow_mut() = None);
    drop(Guard::when_enabled(false, 1, false));
    assert!(CAPTURE.with(|c| c.borrow().is_none()));
}
