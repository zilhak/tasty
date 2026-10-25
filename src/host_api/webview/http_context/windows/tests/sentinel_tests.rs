use super::*;
use std::{
    io::{Read, Write},
    net::Shutdown,
};
fn discard(_: fmt::Arguments<'_>) {}
fn valid() -> Peers {
    let mut observed = Observed::default();
    observed.greeting(5, vec![0]);
    Peers {
        stop: Default::default(),
        threads: Vec::new(),
        observed: Arc::new(Mutex::new(observed)),
        record: discard,
        drain: Arc::new(Drain::new()),
    }
}
fn wait_for_stop(stop: &AtomicBool) {
    let deadline = Instant::now() + IO_WAIT;
    while !stop.load(Ordering::Acquire) {
        assert!(
            Instant::now() < deadline,
            "finalization failed to signal stop"
        );
        std::thread::yield_now();
    }
}
#[test]
fn late_direct_observation_is_joined_before_verdict() {
    let mut peers = valid();
    let stop = peers.stop.clone();
    let observed = peers.observed.clone();
    peers.threads.push(std::thread::spawn(move || {
        wait_for_stop(&stop);
        observed.lock().unwrap().direct = true;
        Ok(())
    }));
    let state = peers.observed.clone();
    assert!(peers.finish().is_err());
    assert!(state.lock().unwrap().direct);
}
#[test]
fn late_rejected_method_is_joined_before_verdict() {
    let mut peers = valid();
    let stop = peers.stop.clone();
    let observed = peers.observed.clone();
    peers.threads.push(std::thread::spawn(move || {
        wait_for_stop(&stop);
        observed.lock().unwrap().greeting(5, vec![2]);
        Ok(())
    }));
    assert!(peers.finish().is_err());
}
#[test]
fn panic_refuses_success_and_does_not_skip_sibling_join() {
    let mut peers = valid();
    let stop = peers.stop.clone();
    peers.threads.push(std::thread::spawn(move || {
        wait_for_stop(&stop);
        panic!("owned observer panic")
    }));
    let done = Arc::new(AtomicBool::new(false));
    let joined = done.clone();
    let stop = peers.stop.clone();
    peers.threads.push(std::thread::spawn(move || {
        wait_for_stop(&stop);
        done.store(true, Ordering::Release);
        Ok(())
    }));
    assert!(peers.finish().unwrap_err().contains("panicked"));
    assert!(joined.load(Ordering::Acquire));
}
#[test]
fn observer_error_refuses_success_and_joins_sibling() {
    let mut peers = valid();
    let stop = peers.stop.clone();
    peers.threads.push(std::thread::spawn(move || {
        wait_for_stop(&stop);
        Err("owned observer I/O failure".into())
    }));
    let done = Arc::new(AtomicBool::new(false));
    let joined = done.clone();
    let stop = peers.stop.clone();
    peers.threads.push(std::thread::spawn(move || {
        wait_for_stop(&stop);
        done.store(true, Ordering::Release);
        Ok(())
    }));
    assert!(peers.finish().unwrap_err().contains("I/O failure"));
    assert!(joined.load(Ordering::Acquire));
}
#[test]
fn drop_on_an_earlier_error_stops_and_joins_all_observers() {
    let mut peers = valid();
    let done = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    for _ in 0..2 {
        let stop = peers.stop.clone();
        let done = done.clone();
        peers.threads.push(std::thread::spawn(move || {
            wait_for_stop(&stop);
            done.fetch_add(1, Ordering::AcqRel);
            Ok(())
        }));
    }
    drop(peers);
    assert_eq!(done.load(Ordering::Acquire), 2);
}
fn listeners() -> (Peers, std::net::SocketAddr, std::net::SocketAddr) {
    let socks = TcpListener::bind("127.0.0.1:0").unwrap();
    let target = TcpListener::bind("127.0.0.1:0").unwrap();
    let proxy = socks.local_addr().unwrap();
    let direct = target.local_addr().unwrap();
    (Peers::start(socks, target, discard).unwrap(), proxy, direct)
}
fn client(proxy: std::net::SocketAddr) -> TcpStream {
    let socket = TcpStream::connect_timeout(&proxy, IO_WAIT).unwrap();
    socket.set_read_timeout(Some(IO_WAIT)).unwrap();
    socket.set_write_timeout(Some(IO_WAIT)).unwrap();
    socket
}
fn hello(proxy: std::net::SocketAddr, method: u8) {
    let mut socket = client(proxy);
    socket.write_all(&[5, 1, method]).unwrap();
    let mut reply = [0; 2];
    socket.read_exact(&mut reply).unwrap();
    assert_eq!(reply, [5, 2]);
    socket.shutdown(Shutdown::Both).unwrap();
}
#[test]
fn actual_normal_valid_observation_finishes_successfully() {
    let (peers, proxy, _) = listeners();
    hello(proxy, 0);
    assert!(peers.finish().is_ok());
}
#[test]
fn actual_empty_observation_is_not_a_success() {
    let (peers, _, _) = listeners();
    assert!(peers.finish().is_err());
}
#[test]
fn actual_queued_direct_connection_is_not_a_success() {
    let (peers, proxy, direct) = listeners();
    hello(proxy, 0);
    let _connection = client(direct);
    assert!(peers.finish().is_err());
}
#[test]
fn actual_overflow_keeps_eight_records_and_refuses_success() {
    let (peers, proxy, _) = listeners();
    for _ in 0..9 {
        hello(proxy, 0);
    }
    let state = peers.observed.clone();
    assert!(peers.finish().is_err());
    let state = state.lock().unwrap();
    assert_eq!(state.greetings.len(), 8);
    assert!(state.overflow);
}
#[test]
fn actual_rejected_method_refuses_success() {
    let (peers, proxy, _) = listeners();
    hello(proxy, 2);
    assert!(peers.finish().is_err());
}
#[test]
fn truncated_methods_after_valid_greeting_propagate_observer_io_error() {
    let (peers, proxy, _) = listeners();
    hello(proxy, 0);
    let mut socket = client(proxy);
    socket.write_all(&[5, 2, 0]).unwrap();
    socket.shutdown(Shutdown::Write).unwrap();
    let mut byte = [0];
    assert_eq!(socket.read(&mut byte).unwrap(), 0);
    assert!(peers.finish().unwrap_err().contains("SOCKS methods"));
}
#[test]
fn unexpected_auth_reply_after_valid_greeting_refuses_success() {
    let (peers, proxy, _) = listeners();
    let mut socket = client(proxy);
    socket.write_all(&[5, 1, 0]).unwrap();
    let mut reply = [0; 2];
    socket.read_exact(&mut reply).unwrap();
    socket.write_all(&[1, 0]).unwrap();
    let mut byte = [0];
    assert_eq!(socket.read(&mut byte).unwrap(), 0);
    assert!(peers.finish().unwrap_err().contains("unsupported SOCKS"));
}
