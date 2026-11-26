//! 원격 연결의 control 소켓과 시도 취소용 복제는 자식 프로세스가 물려받지 않는다(Windows).
//! reader·writer 핸들은 `StreamConnection`이 만들고 tasty-ipc 시험이 본다.

use std::io::{BufRead, BufReader};
use std::sync::Arc;
use std::time::Duration;
use tasty_ipc::stream::{self, StreamTag};
use tasty_utils::socket_inherit::is_inheritable;

#[test]
fn the_control_socket_and_the_cancellation_clone_are_not_inheritable() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let peer = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().expect("accept");
        let mut request = String::new();
        BufReader::new(socket.try_clone().expect("clone"))
            .read_line(&mut request)
            .expect("request");
        for control in [
            serde_json::json!({"ok": true, "proto": 1, "client_id": 7}),
            serde_json::json!({"event": "attached_workspace", "name": "remote", "tree": null, "surfaces": []}),
        ] {
            stream::write_frame(
                &mut socket,
                StreamTag::Control,
                &serde_json::to_vec(&control).expect("control"),
            )
            .expect("write control");
        }
        socket
    });
    let mut remote = crate::outbound::Remote::new();
    let token = remote.begin_attempt(None, None).expect("attempt");
    let prepared = crate::transport::PreparedConnection::connect(
        token.clone(),
        port,
        3,
        None,
        Arc::new(|| {}),
        |_| None,
        Duration::from_secs(30),
    )
    .expect("connect");
    let _peer_socket = peer.join().expect("peer");
    let control = prepared
        .transport
        .workers
        .control
        .as_ref()
        .expect("control socket");
    let control_inheritable = is_inheritable(control).expect("control");
    let registered = token.registered_sockets_inheritable();
    assert!(!registered.is_empty(), "no cancellation socket registered");
    assert!(
        !control_inheritable && registered.iter().all(|inheritable| !inheritable),
        "inheritable handles remain: control {control_inheritable}, cancellation clones {registered:?}"
    );
    prepared.transport.close_locally();
}
