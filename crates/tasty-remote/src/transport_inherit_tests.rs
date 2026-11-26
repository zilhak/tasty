//! 원격 연결의 원본 소켓, control 소켓과 시도 취소용 복제는 상속 플래그가 꺼져 있다(Windows).
//! 원본은 연결 직후 자리(`attempt_seam`)에서 상속을 일부러 켜고, stream open이 다시 끄기 전인 등록 직후
//! 자리에서 본다. 그래서 연결 직후의 끄는 호출이 빠지면 std가 돌려준 값과 상관없이 실패한다.
//! reader·writer 핸들은 `StreamConnection`이 만들고 tasty-ipc 시험이 본다.

use crate::attempt_seam::{self, Stage};
use std::cell::Cell;
use std::io::{BufRead, BufReader};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;
use tasty_ipc::stream::{self, StreamTag};
use tasty_utils::socket_inherit::{is_inheritable, make_inheritable};

#[test]
fn the_attach_sockets_are_not_inheritable() {
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
    let original = Rc::new(Cell::new(None));
    let seen = original.clone();
    attempt_seam::arm(Stage::AfterConnect, |socket| {
        make_inheritable(socket).expect("make the original inheritable");
    });
    attempt_seam::arm_also(Stage::AfterRegister, move |socket| {
        seen.set(Some(is_inheritable(socket).expect("original")));
    });
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
    assert!(attempt_seam::reached(Stage::AfterConnect));
    let control = prepared
        .transport
        .workers
        .control
        .as_ref()
        .expect("control socket");
    let control_inheritable = is_inheritable(control).expect("control");
    let registered = token.registered_sockets_inheritable();
    assert!(!registered.is_empty(), "no cancellation socket registered");
    let original = original
        .get()
        .expect("the connection passed the registration seam");
    assert!(
        !original && !control_inheritable && registered.iter().all(|inheritable| !inheritable),
        "inheritable handles remain: original {original}, control {control_inheritable}, \
         cancellation clones {registered:?}"
    );
    prepared.transport.close_locally();
}
