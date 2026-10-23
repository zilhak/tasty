use super::*;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone)]
struct LogBuffer(Arc<Mutex<Vec<u8>>>);
impl std::io::Write for LogBuffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn explorer_app_shutdown_timeout_expires_without_rearming_or_claiming_a_join() {
    let buffer = LogBuffer(Arc::new(Mutex::new(Vec::new())));
    let output = buffer.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .without_time()
        .with_writer(move || output.clone())
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    let start = Instant::now();
    let until = start + Duration::from_secs(5);
    let mut deadline = Some(until);
    assert!(keep_waiting(1, &mut deadline, start));
    assert!(keep_waiting(
        4,
        &mut deadline,
        until - Duration::from_nanos(1)
    ));
    assert_eq!(deadline, Some(until));
    assert!(!keep_waiting(4, &mut deadline, until));
    assert_eq!(deadline, None);
    assert!(!keep_waiting(4, &mut deadline, start));
    let log = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
    assert_eq!(log.matches("View I/O shutdown timed out").count(), 1);
    assert!(log.contains("worker remains unjoined"));
}

#[test]
fn explorer_app_shutdown_with_joined_workers_never_waits_for_the_deadline() {
    let now = Instant::now();
    let mut deadline = Some(now + Duration::from_secs(5));
    assert!(!keep_waiting(0, &mut deadline, now));
    assert!(deadline.is_some());
    assert!(!keep_waiting(
        0,
        &mut deadline,
        now + Duration::from_secs(10)
    ));
}

#[test]
fn explorer_app_shutdown_without_a_deadline_does_not_block_on_an_unjoined_worker() {
    assert!(!keep_waiting(1, &mut None, Instant::now()));
}
