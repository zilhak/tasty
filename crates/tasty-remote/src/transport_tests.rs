use std::io::Read;
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

use super::*;

#[test]
fn closing_a_dead_connection_lets_the_peer_see_the_end_at_once() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let ours = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (mut peer, _) = listener.accept().unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let workers = ConnectionWorkers::new(ours, Vec::new());

    workers.close_socket();
    let mut buf = [0u8; 1];
    assert_eq!(
        peer.read(&mut buf)
            .expect("the peer reads the end, not a timeout"),
        0
    );
    drop(workers);
}
