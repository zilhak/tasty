use super::*;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

fn delayed() -> (ObserverRouter, mpsc::Sender<()>, mpsc::Receiver<()>) {
    let (release, wait) = mpsc::channel();
    let (done, finished) = mpsc::channel();
    let join = thread::spawn(move || {
        wait.recv().unwrap();
        done.send(()).unwrap();
    });
    let mut router = ObserverRouter::new();
    router.retired.push(RetiredSink {
        id: 1,
        join,
        tally: Arc::new(memory_sink::SinkTally::default()),
        dropped: 0,
    });
    (router, release, finished)
}

fn joined(router: &mut ObserverRouter) -> ShutdownProgress {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let progress = router.poll_shutdown();
        if progress.pending == 0 {
            return progress;
        }
        assert!(Instant::now() < deadline, "worker never joined");
        thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn a_delayed_sink_stays_owned_until_its_actual_nonblocking_join() {
    let (mut router, release, done) = delayed();
    assert_eq!(router.poll_shutdown().pending, 1);
    assert_eq!(router.poll_shutdown().joined, 0);
    assert_eq!(router.retired.len(), 1);
    release.send(()).unwrap();
    done.recv_timeout(Duration::from_secs(2)).unwrap();
    let progress = joined(&mut router);
    assert_eq!(progress.joined, 1);
    assert!(progress.clean());
    assert_eq!(router.poll_shutdown(), progress);
}

#[test]
fn an_observer_worker_panic_is_joined_but_not_clean_completion() {
    let mut router = ObserverRouter::new();
    router.retired.push(RetiredSink {
        id: 2,
        join: thread::spawn(|| panic!("owned observer worker fault")),
        tally: Arc::new(memory_sink::SinkTally::default()),
        dropped: 0,
    });
    let progress = joined(&mut router);
    assert_eq!(progress.joined, 1);
    assert_eq!(progress.panicked, 1);
    assert!(!progress.clean());
    assert_eq!(router.poll_shutdown(), progress);
}

#[test]
fn timeout_keeps_the_handle_and_drop_does_not_restart_an_unbounded_wait() {
    let (mut router, release, done) = delayed();
    let start = Instant::now();
    let mut drain = crate::app::writer_drain::WriterDrain::default();
    assert!(drain.keep_waiting(router.poll_shutdown().pending, start, "owned timeout"));
    assert!(!drain.keep_waiting(
        router.poll_shutdown().pending,
        start + crate::app::writer_drain::WRITER_DRAIN_TIMEOUT,
        "owned timeout",
    ));
    router.fence_submissions();
    assert_eq!(router.retired.len(), 1);
    assert_eq!(router.poll_shutdown().joined, 0);
    let (dropped, seen) = mpsc::channel();
    let owner = thread::spawn(move || {
        drop(router);
        dropped.send(()).unwrap();
    });
    let observation = seen.recv_timeout(Duration::from_secs(1));
    release.send(()).unwrap();
    done.recv_timeout(Duration::from_secs(2)).unwrap();
    owner.join().unwrap();
    assert!(observation.is_ok(), "Drop blocked on an unfinished sink");
}

#[test]
fn fenced_memory_sink_cannot_submit_after_writer_drain_begins() {
    let store: Arc<Mutex<dyn tasty_memory::MemoryStorage>> = Arc::new(Mutex::new(
        tasty_memory::MemoryStore::open_in_memory().unwrap(),
    ));
    let tally = Arc::new(memory_sink::SinkTally::default());
    tally.close_submissions();
    let (send, receive) = mpsc::sync_channel(1);
    send.send(ParsedItem {
        kind: "path",
        line: 0,
        byte_start: 0,
        byte_end: 1,
        data: json!({"path":"owned"}),
    })
    .unwrap();
    drop(send);
    memory_sink::run(
        7,
        0,
        receive,
        tasty_memory::writer::MemoryAccess::inline(store.clone()),
        tally.clone(),
    );
    assert_eq!(
        tally
            .submit_rejected
            .load(std::sync::atomic::Ordering::Relaxed),
        1
    );
    assert_eq!(
        store
            .lock()
            .unwrap()
            .count(&tasty_memory::Scope::Global, None)
            .unwrap(),
        0
    );
}

#[cfg(target_os = "linux")]
#[test]
fn a_failed_file_sink_is_not_reported_as_clean_just_because_it_joined() {
    let mut router = ObserverRouter::new();
    let store: Arc<Mutex<dyn tasty_memory::MemoryStorage>> = Arc::new(Mutex::new(
        tasty_memory::MemoryStore::open_in_memory().unwrap(),
    ));
    router
        .register(
            ObserverSpec {
                surface_id: Some(3),
                parsers: vec!["url".into()],
                kinds: None,
                sink: SinkSpec::File {
                    path: Some("/dev/full".into()),
                },
            },
            tasty_memory::writer::MemoryAccess::inline(store),
        )
        .unwrap();
    router.dispatch_text(3, "https://owned.invalid/observer\n");
    let progress = joined(&mut router);
    assert_eq!(progress.joined, 1);
    assert_eq!(progress.failed, 1);
    assert!(!progress.clean());
}
