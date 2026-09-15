//! Exercise real poison acquisition and Condvar reacquisition with a private waker.

use super::*;
use std::io::Write;
use std::sync::atomic::Ordering;
use std::time::Duration;

#[derive(Clone)]
struct LogCapture(Arc<Mutex<Vec<u8>>>);

impl Write for LogCapture {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .expect("capture lock")
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn waker() -> Arc<TimerWaker> {
    Arc::new(TimerWaker {
        state: Mutex::new(State {
            deadline: None,
            stopped: false,
        }),
        cv: Condvar::new(),
        poison_reported: AtomicBool::new(false),
    })
}

fn poison(waker: &TimerWaker) {
    let result = std::panic::catch_unwind(|| {
        let mut guard = waker.state.lock().expect("initially healthy state");
        guard.deadline = Some(Instant::now());
        panic!("synthetic state poison");
    });
    assert!(result.is_err());
    assert!(waker.state.is_poisoned());
    waker.cv.notify_one();
}

fn with_log(test: impl FnOnce()) -> String {
    let capture = LogCapture(Arc::new(Mutex::new(Vec::new())));
    let writer = capture.clone();
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    tracing::subscriber::with_default(subscriber, test);
    let bytes = capture.0.lock().expect("capture lock").clone();
    String::from_utf8(bytes).expect("UTF-8 log")
}

fn assert_reported_once(log: &str) {
    assert_eq!(
        log.matches("timer waker state lock poisoned").count(),
        1,
        "{log}"
    );
    assert_eq!(log.matches("ERROR").count(), 1, "{log}");
}

fn still_fires_and_stops(waker: &TimerWaker) {
    assert!(waker.poison_reported.load(Ordering::Relaxed));
    // The deadline set before the panic survived; firing clears it as usual.
    assert!(wait_for_deadline(waker));
    assert!(waker.lock().deadline.is_none());
    waker.set_deadline(Some(Instant::now()));
    assert!(wait_for_deadline(waker));
    waker.stop();
    assert!(!wait_for_deadline(waker));
}

#[test]
fn poisoned_acquisition_reports_once_and_keeps_firing() {
    let waker = waker();
    poison(&waker);
    let log = with_log(|| {
        drop(waker.lock());
        still_fires_and_stops(&waker);
    });
    assert_reported_once(&log);
}

fn reacquisition_reports(timed: bool) {
    let waker = waker();
    // Holding the healthy guard makes the poisoner wait until Condvar releases it.
    // No sleep or scheduling assumption determines whether poison happens in the wait.
    let mut guard = waker.state.lock().expect("healthy state");
    if timed {
        guard.deadline = Some(Instant::now() + Duration::from_secs(5));
    }
    let poisoner = Arc::clone(&waker);
    let thread = std::thread::spawn(move || poison(&poisoner));
    let log = with_log(|| {
        // Enter the same loop as the real worker, with its initial healthy acquisition.
        assert!(wait_for_deadline_with_state(&waker, guard));
        assert!(waker.poison_reported.load(Ordering::Relaxed));
        assert!(waker.lock().deadline.is_none());
        waker.set_deadline(Some(Instant::now()));
        still_fires_and_stops(&waker);
    });
    thread.join().expect("poisoner caught its synthetic panic");
    assert_reported_once(&log);
}

#[test]
fn poison_during_wait_is_reported_on_reacquisition() {
    reacquisition_reports(false);
}

#[test]
fn poison_during_timed_wait_is_reported_on_reacquisition() {
    reacquisition_reports(true);
}
