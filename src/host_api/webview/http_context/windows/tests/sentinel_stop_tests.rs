//! Actual queued TCP at successful stop, including the two independent-review regressions.
use super::*;
use sentinel_io::{MAX_BYTES, MAX_CONNECTIONS};
use std::{io::Write, net::Shutdown};
fn discard(_: fmt::Arguments<'_>) {}
fn listener() -> TcpListener {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    listener
}
fn queued(listener: &TcpListener, bytes: &[u8]) -> TcpStream {
    let mut socket = TcpStream::connect_timeout(&listener.local_addr().unwrap(), IO_WAIT).unwrap();
    socket.set_write_timeout(Some(IO_WAIT)).unwrap();
    socket.write_all(bytes).unwrap();
    socket.shutdown(Shutdown::Write).unwrap();
    socket
}
fn owner(listener: TcpListener, socks: bool) -> Peers {
    let mut initial = Observed::default();
    initial.greeting(5, vec![0]);
    let observed = Arc::new(Mutex::new(initial));
    let stop = Arc::new(AtomicBool::new(false));
    let drain = Arc::new(Drain::new());
    let (s, o, d) = (stop.clone(), observed.clone(), drain.clone());
    let thread = std::thread::spawn(move || {
        let until = Instant::now() + IO_WAIT;
        while !s.load(Ordering::Acquire) {
            assert!(Instant::now() < until);
            std::thread::yield_now();
        }
        observe(listener, socks, s, o, discard, d)
    });
    Peers {
        stop,
        threads: vec![thread],
        observed,
        record: discard,
        drain,
    }
}
#[test]
fn all_already_queued_methods_must_be_validated_before_success() {
    let listener = listener();
    let _first = queued(&listener, &[5, 1, 0]);
    let _second = queued(&listener, &[5, 1, 2]);
    let peers = owner(listener, true);
    assert!(
        peers.finish().is_err(),
        "queued rejected authentication method was ignored after stop; final receipt remains successful"
    );
}
#[test]
fn queued_truncated_methods_must_not_inherit_prior_success() {
    let listener = listener();
    let _partial = queued(&listener, &[5, 2, 0]);
    let peers = owner(listener, true);
    assert!(
        peers.finish().is_err(),
        "truncated accepted greeting I/O was suppressed by stop after a valid observation"
    );
}
#[test]
fn successful_stop_drains_all_queued_valid_greetings() {
    let listener = listener();
    let _first = queued(&listener, &[5, 1, 0]);
    let _second = queued(&listener, &[5, 1, 0]);
    let peers = owner(listener, true);
    let state = peers.observed.clone();
    assert!(peers.finish().is_ok());
    assert_eq!(state.lock().unwrap().greetings.len(), 3);
}
#[test]
fn queued_truncated_header_is_an_error_after_successful_stop() {
    let listener = listener();
    let _partial = queued(&listener, &[5]);
    assert!(
        owner(listener, true)
            .finish()
            .unwrap_err()
            .contains("SOCKS header")
    );
}
#[test]
fn queued_overflow_remains_bounded_and_refuses_success() {
    let listener = listener();
    let sockets: Vec<_> = (0..8).map(|_| queued(&listener, &[5, 1, 0])).collect();
    let peers = owner(listener, true);
    let state = peers.observed.clone();
    assert!(peers.finish().is_err());
    assert_eq!(state.lock().unwrap().greetings.len(), 8);
    assert!(state.lock().unwrap().overflow);
    drop(sockets);
}
#[test]
fn queued_connections_exhaust_actual_shared_accept_budget() {
    let listener = listener();
    let sockets: Vec<_> = (0..MAX_CONNECTIONS)
        .map(|_| queued(&listener, &[]))
        .collect();
    let peers = owner(listener, false);
    let drain = peers.drain.clone();
    assert!(peers.finish().unwrap_err().contains("connection budget"));
    assert_eq!(drain.connections.load(Ordering::Relaxed), MAX_CONNECTIONS);
    drop(sockets);
}
#[test]
fn queued_socket_cannot_read_beyond_remaining_byte_budget() {
    let listener = listener();
    let _socket = queued(&listener, &[5, 1, 0]);
    let peers = owner(listener, true);
    let drain = peers.drain.clone();
    drain.bytes.store(MAX_BYTES - 1, Ordering::Relaxed);
    assert!(peers.finish().unwrap_err().contains("byte budget"));
    assert_eq!(drain.bytes.load(Ordering::Relaxed), MAX_BYTES);
}
#[test]
fn incomplete_open_socket_obeys_absolute_shared_deadline() {
    let listener = listener();
    let mut socket = TcpStream::connect_timeout(&listener.local_addr().unwrap(), IO_WAIT).unwrap();
    socket.write_all(&[5]).unwrap();
    let peers = owner(listener, true);
    *peers.drain.deadline.lock().unwrap() = Instant::now() + Duration::from_millis(100);
    let started = Instant::now();
    assert!(peers.finish().unwrap_err().contains("absolute deadline"));
    // A held partial socket must not fall back to a fresh two-second read timeout.
    assert!(started.elapsed() < IO_WAIT);
    drop(socket);
}
#[test]
fn failure_cleanup_skips_partial_queued_socket_and_joins() {
    let listener = listener();
    let _socket = queued(&listener, &[5, 2, 0]);
    let peers = owner(listener, true);
    let drain = peers.drain.clone();
    drop(peers);
    assert!(drain.cleanup());
    assert_eq!(drain.connections.load(Ordering::Relaxed), 0);
}

#[test]
fn slow_partial_progress_cannot_restart_finalization_deadline() {
    let listener = listener();
    let mut socket = TcpStream::connect_timeout(&listener.local_addr().unwrap(), IO_WAIT).unwrap();
    socket.set_write_timeout(Some(IO_WAIT)).unwrap();
    socket.write_all(&[5, 255]).unwrap();
    let peers = owner(listener, true);
    let stop = peers.stop.clone();
    let drain = peers.drain.clone();
    *drain.deadline.lock().unwrap() = Instant::now() + Duration::from_millis(100);
    let writer = std::thread::spawn(move || {
        let until = Instant::now() + IO_WAIT;
        while !stop.load(Ordering::Acquire) {
            assert!(Instant::now() < until);
            std::thread::yield_now();
        }
        for _ in 0..10 {
            if socket.write_all(&[0]).is_err() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    });
    let started = Instant::now();
    assert!(peers.finish().unwrap_err().contains("absolute deadline"));
    let elapsed = started.elapsed();
    writer.join().unwrap();
    assert!(drain.bytes.load(Ordering::Relaxed) > 2);
    assert!(elapsed < IO_WAIT);
}
