//! Ordered stream contracts; raw tap tests do not exercise this receiver's loss state.
use super::*;
use std::sync::Barrier;
use std::time::{Duration, Instant};

#[test]
fn ordered_output_and_resize_drain_before_one_loss_and_disconnect() {
    let mut terminal = Terminal::new_detached(80, 24);
    let mut subscription = terminal.snapshot_and_stream();
    assert_eq!((subscription.cols, subscription.rows), (80, 24));
    terminal.feed_bytes(b"first");
    terminal.resize(90, 30);
    for _ in 2..ATTACH_STREAM_MAX_EVENTS {
        terminal.feed_bytes(b"x");
    }
    terminal.feed_bytes(b"overflow");
    terminal.feed_bytes(b"must not resume");
    terminal.resize(100, 40);
    assert!(
        matches!(subscription.events.try_recv(), Ok(AttachEvent::Output(bytes)) if bytes == b"first")
    );
    assert!(matches!(
        subscription.events.try_recv(),
        Ok(AttachEvent::Resize { cols: 90, rows: 30 })
    ));
    for _ in 2..ATTACH_STREAM_MAX_EVENTS {
        assert!(
            matches!(subscription.events.try_recv(), Ok(AttachEvent::Output(bytes)) if bytes == b"x")
        );
    }
    assert!(matches!(
        subscription.events.try_recv(),
        Ok(AttachEvent::Loss)
    ));
    for _ in 0..2 {
        assert!(matches!(
            subscription.events.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
    }
    assert_eq!(subscription.events.state.bytes.load(Ordering::Acquire), 0);
}

#[test]
fn byte_limit_preserves_exact_prefix_and_receiver_drop_releases_subscription() {
    let mut terminal = Terminal::new_detached(80, 24);
    let mut subscription = terminal.snapshot_and_stream();
    let prefix = vec![b'x'; ATTACH_STREAM_MAX_BYTES - EVENT_CHARGE];
    // Use the actual producer under its parser lock without parsing a megabyte of glyphs.
    {
        let mut state = terminal.lock_state();
        state.fan_out_attach_output(&prefix);
        state.fan_out_attach_resize(81, 24);
        assert!(state.attach_streams.is_empty());
    }
    assert!(
        matches!(subscription.events.try_recv(), Ok(AttachEvent::Output(bytes)) if bytes == prefix)
    );
    assert!(matches!(
        subscription.events.try_recv(),
        Ok(AttachEvent::Loss)
    ));
    assert!(matches!(
        subscription.events.try_recv(),
        Err(mpsc::TryRecvError::Disconnected)
    ));
    let fresh = terminal.snapshot_and_stream();
    drop(fresh);
    terminal.feed_bytes(b"still live");
    assert!(terminal.lock_state().attach_streams.is_empty());
    assert!(terminal.screen_text(false).contains("still live"));
}

#[test]
fn ordered_snapshot_subscription_accounts_for_concurrent_parser_ingest() {
    // Same parser-lock ingestion pattern as snapshot_and_tap's existing test. Keep the
    // entire workload below this channel's count bound so Loss cannot hide a cut error.
    const CHUNKS: usize = 128;
    const CHUNK: &[u8] = b"xxxxxxxx";
    for _ in 0..128 {
        let mut terminal = Terminal::new_detached(80, 40);
        let state = Arc::clone(&terminal.state);
        let fed = Arc::new(AtomicUsize::new(0));
        let worker_fed = Arc::clone(&fed);
        let feeder = std::thread::spawn(move || {
            for _ in 0..CHUNKS {
                state.lock().expect("parser state").ingest(CHUNK);
                worker_fed.fetch_add(1, Ordering::Release);
                std::thread::yield_now();
            }
        });
        while fed.load(Ordering::Acquire) < CHUNKS / 4 {
            std::thread::yield_now();
        }
        let mut subscription = terminal.snapshot_and_stream();
        feeder.join().expect("feeder");
        let mut observed = subscription
            .snapshot
            .iter()
            .filter(|&&byte| byte == b'x')
            .count();
        loop {
            match subscription.events.try_recv() {
                Ok(AttachEvent::Output(bytes)) => {
                    assert_eq!(bytes, CHUNK);
                    observed += bytes.len();
                }
                Err(mpsc::TryRecvError::Empty) => break,
                other => panic!("under-budget subscription lost its cut: {other:?}"),
            }
        }
        assert_eq!(observed, CHUNKS * CHUNK.len());
    }
}

#[test]
fn empty_poll_racing_final_enqueue_and_overflow_never_overtakes_prefix() {
    // Stress the real receiver with a producer racing its Empty poll. No test hook forces
    // the two internal receives: this supplements, rather than proves, every interleaving.
    let oversized = Arc::new(vec![0u8; ATTACH_STREAM_MAX_BYTES]);
    for _ in 0..1024 {
        let (sender, receiver) = mpsc::sync_channel(ATTACH_STREAM_MAX_EVENTS);
        let state = Arc::new(QueueState::default());
        let tap = AttachStreamTap {
            sender,
            state: state.clone(),
        };
        let mut events = AttachEventReceiver {
            receiver,
            state,
            ended: false,
        };
        assert!(matches!(events.try_recv(), Err(mpsc::TryRecvError::Empty)));
        let start = Arc::new(Barrier::new(2));
        let worker_start = start.clone();
        let oversized = oversized.clone();
        let producer = std::thread::spawn(move || {
            worker_start.wait();
            assert!(tap.send(Some(b"retained"), 0, 0));
            // Even if the consumer drains concurrently, this event exceeds the byte cap.
            assert!(!tap.send(Some(&oversized), 0, 0));
        });
        start.wait();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut prefix = Vec::new();
        loop {
            match events.try_recv() {
                Ok(AttachEvent::Output(bytes)) => prefix.extend(bytes),
                Ok(AttachEvent::Loss) => break,
                Err(mpsc::TryRecvError::Empty) => {
                    assert!(Instant::now() < deadline, "producer stalled");
                    std::thread::yield_now();
                }
                other => panic!("unexpected event before loss: {other:?}"),
            }
        }
        producer.join().expect("producer");
        assert_eq!(
            prefix, b"retained",
            "Loss overtook the final accepted output"
        );
        assert!(matches!(
            events.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
    }
}
