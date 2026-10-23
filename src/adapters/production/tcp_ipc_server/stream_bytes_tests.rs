use super::*;
use tasty_ipc::{
    stream::StreamFrame,
    stream_hub::{PublicationFence, PushResult, StreamHub},
};
const FULL: usize = stream::MAX_FRAME_LEN as usize + stream::FRAME_HEADER_LEN;

fn sockets() -> (std::net::TcpStream, std::net::TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let client = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let (server, _) = listener.accept().unwrap();
    (client, server)
}
#[test]
fn actual_socket_writer_skips_retired_document_and_writes_live_frame() {
    let (mut client, server) = sockets();
    let hub = StreamHub::new();
    let rx = hub.register(5);
    let binding = hub.client_binding(5).unwrap();
    let owner = PublicationFence::default();
    let reservation = hub
        .reserve_frame(5, &binding, FULL)
        .unwrap()
        .for_owner(&owner);
    assert_eq!(
        hub.push_reserved(
            reservation,
            StreamFrame::new(StreamTag::Control, b"retired".to_vec())
        ),
        PushResult::Sent
    );
    drop(owner);
    assert_eq!(
        hub.push(5, StreamFrame::new(StreamTag::Control, b"live".to_vec())),
        PushResult::Sent
    );
    assert_eq!(
        hub.push(5, StreamFrame::new(StreamTag::Detach, vec![])),
        PushResult::Sent
    );
    let writer = TcpIpcServer::spawn_stream_write_thread(server, rx);
    assert_eq!(stream::read_frame(&mut client).unwrap().payload, b"live");
    assert_eq!(
        stream::read_frame(&mut client).unwrap().tag,
        StreamTag::Detach
    );
    writer.join().unwrap();
    assert!(stream::read_frame(&mut client).is_err());
    assert!(hub.reserve_frame(5, &binding, FULL).is_some());
}
#[test]
fn actual_socket_writer_discards_old_registration_without_publishing_to_replacement() {
    let (mut client, server) = sockets();
    let hub = StreamHub::new();
    let rx = hub.register(5);
    assert_eq!(
        hub.push(5, StreamFrame::new(StreamTag::Control, b"old".to_vec())),
        PushResult::Sent
    );
    let replacement = hub.register(5);
    let writer = TcpIpcServer::spawn_stream_write_thread(server, rx);
    writer.join().unwrap();
    assert!(stream::read_frame(&mut client).is_err());
    assert!(replacement.try_recv().is_err());
    assert!(
        hub.reserve_frame(5, &hub.client_binding(5).unwrap(), FULL)
            .is_some()
    );
}
