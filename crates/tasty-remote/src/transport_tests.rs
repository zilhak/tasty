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

#[test]
fn reader_receipt_finishes_while_delta_is_restored_ahead_of_data() {
    use std::io::{BufRead, BufReader};
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let ours = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let handshake = std::thread::spawn(move || {
        let (mut peer, _) = listener.accept().unwrap();
        let mut request = String::new();
        BufReader::new(peer.try_clone().unwrap())
            .read_line(&mut request)
            .unwrap();
        stream::write_frame(
            &mut peer,
            StreamTag::Control,
            &serde_json::to_vec(&serde_json::json!({"ok":true,"proto":1,"client_id":7})).unwrap(),
        )
        .unwrap();
        peer
    });
    let (conn, _) = StreamConnection::open(ours, 1).unwrap();
    let mut peer = handshake.join().unwrap();
    let (sender, _queue) = super::super::connection::channel();
    sender.enable_input_receipts(true);
    sender.user_input_dispatched(std::time::Instant::now());
    let output = MirrorOutbox::new(sender.epoch());
    let disconnected = Arc::new(AtomicBool::new(false));
    let (wake_tx, wake_rx) = std::sync::mpsc::channel();
    let wake = Arc::new(move || {
        wake_tx.send(()).expect("test receiver remains alive");
    });
    let reader = spawn_attach_reader_thread(
        conn,
        output.clone(),
        sender.clone(),
        disconnected,
        wake,
        1,
        |_| {
            Some(MirrorEvent::StructuralDelta {
                workspace_id: 1,
                tree: serde_json::json!({}),
                surfaces: Vec::new(),
            })
        },
    );
    stream::write_frame(&mut peer, StreamTag::Control, b"{}").unwrap();
    wake_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    // The application cannot yet reserve IDs, so its Delta remains at the front.
    output.restore_front(output.drain());
    stream::write_frame(
        &mut peer,
        StreamTag::Control,
        &serde_json::to_vec(&StreamControl::InputProcessed { sequence: 1 }).unwrap(),
    )
    .unwrap();
    wake_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(sender.oldest_unprocessed_input().is_none());
    stream::write_frame(&mut peer, StreamTag::Data, &stream::encode_mux(9, b"after")).unwrap();
    wake_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let events = output.drain();
    assert!(
        matches!(&events[..], [MirrorEvent::StructuralDelta { .. }, MirrorEvent::Data(9, data)] if data == b"after")
    );
    peer.shutdown(std::net::Shutdown::Both).unwrap();
    reader.join().unwrap();
}
