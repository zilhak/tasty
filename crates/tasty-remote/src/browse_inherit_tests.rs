//! 원격 JSON-RPC 요청 한 번(`probe_method_bound`)의 원본 소켓, writer 복제, 시도 취소용 복제는 상속
//! 플래그가 꺼져 있다(Windows). 원본은 연결 직후 자리(`attempt_seam`)에서 상속을 일부러 켜서,
//! 끄는 호출이 빠지면 std가 돌려준 값과 상관없이 실패하게 한다.

use crate::attempt_seam::{self, Stage};
use std::cell::Cell;
use std::io::{BufRead, BufReader, Write};
use std::rc::Rc;
use tasty_utils::socket_inherit::{is_inheritable, make_inheritable};

#[test]
fn the_probe_sockets_are_not_inheritable() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let peer = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().expect("accept");
        let mut request = String::new();
        BufReader::new(socket.try_clone().expect("clone"))
            .read_line(&mut request)
            .expect("request");
        writeln!(socket, r#"{{"jsonrpc":"2.0","result":{{}},"id":1}}"#).expect("response");
    });
    let mut remote = crate::outbound::Remote::new();
    let token = remote.begin_attempt(None, None).expect("attempt");
    let original = Rc::new(Cell::new(None));
    let writer = Rc::new(Cell::new(None));
    let (seen_original, seen_writer) = (original.clone(), writer.clone());
    attempt_seam::arm(Stage::AfterConnect, |socket| {
        make_inheritable(socket).expect("make the original inheritable");
    });
    attempt_seam::arm_also(Stage::AfterRegister, move |socket| {
        seen_original.set(Some(is_inheritable(socket).expect("original")));
    });
    attempt_seam::arm_also(Stage::BeforeRequest, move |socket| {
        seen_writer.set(Some(is_inheritable(socket).expect("writer")));
    });
    crate::browse::probe_method_bound(port, "system.info", serde_json::json!({}), Some(&token))
        .expect("probe");
    peer.join().expect("peer");
    assert!(attempt_seam::reached(Stage::AfterConnect));
    let registered = token.registered_sockets_inheritable();
    assert!(!registered.is_empty(), "no cancellation socket registered");
    let original = original
        .get()
        .expect("the probe passed the registration seam");
    let writer = writer.get().expect("the probe passed the request seam");
    assert!(
        !original && !writer && registered.iter().all(|inheritable| !inheritable),
        "inheritable handles remain: original {original}, writer {writer}, \
         cancellation clones {registered:?}"
    );
}
