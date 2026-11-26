//! 서버가 accept한 연결의 소켓과 reader 복제는 자식 프로세스가 물려받지 않는다(Windows).
//! 입력 소켓의 상속을 일부러 켜서, 끄는 호출이 빠지면 std가 돌려준 값과 상관없이 실패하게 한다.

use super::TcpIpcServer;
use std::net::{TcpListener, TcpStream};
use tasty_utils::socket_inherit::{is_inheritable, make_inheritable};

#[test]
fn an_accepted_connection_keeps_its_sockets_out_of_child_processes() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let _client = TcpStream::connect(listener.local_addr().expect("addr")).expect("connect");
    let (accepted, _) = listener.accept().expect("accept");
    make_inheritable(&accepted).expect("make inheritable");
    assert!(
        TcpIpcServer::keep_out_of_children(&accepted),
        "the accept step failed"
    );
    let (reader, writer, _) = TcpIpcServer::prepare_stream(accepted).expect("prepared");
    let flags: Vec<(&str, bool)> = [("reader", reader.get_ref()), ("writer", &writer)]
        .into_iter()
        .map(|(name, socket)| (name, is_inheritable(socket).expect(name)))
        .collect();
    assert!(
        flags.iter().all(|(_, inheritable)| !inheritable),
        "inheritable handles remain: {flags:?}"
    );
}
