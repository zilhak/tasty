//! EOF 검증용 소켓 시험. 같은 바이너리의 포트 재사용 시험과 경합하지 않도록 분리한다.

// 이유: 테스트는 let _ = 사유 주석 정책의 대상이 아니며 제품 lint는 유지한다.
#![allow(clippy::let_underscore_must_use)]

use std::io::{BufRead, BufReader};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::time::Duration;

use tasty_ipc::client::IpcConnection;
use tasty_ipc::protocol::JsonRpcRequest;

/// EOF 뒤 무한 반복이 생겨도 시험이 멈추지 않도록 별도 스레드와 recv_timeout으로 확인한다.
#[test]
fn 응답_없는_eof_는_스핀하지_않고_에러로_끝난다() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        let (sock, _) = listener.accept().expect("accept");
        // 요청 한 줄을 완전히 소비해야 RST 가 아니라 깨끗한 FIN(=EOF)이 간다.
        let mut reader = BufReader::new(sock.try_clone().expect("clone"));
        let mut req = String::new();
        // 읽기 실패는 무시한다 — 이 더미 서버의 목적은 "응답 없이 FIN" 이고,
        // 요청을 못 읽으면 어차피 그 다음 shutdown 이 같은 결과를 만든다.
        let _ = reader.read_line(&mut req);
        drop(reader);
        drop(sock);
    });

    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        let mut conn = IpcConnection::new(stream).expect("conn");
        let request = JsonRpcRequest {
            response_timeout_ms: None,
            idempotency_key: None,
            jsonrpc: "2.0".to_string(),
            method: "system.info".to_string(),
            params: serde_json::Value::Null,
            id: Some(serde_json::json!(1)),
            session_token: None,
        };
        // send 실패는 무시한다 — 수신측이 이미 timeout 으로 판정하고 떠났다는
        // 뜻이고, 그 판정(panic)이 이 스레드의 보고보다 우선한다.
        let _ = tx.send(conn.send(&request).is_err());
    });

    match rx.recv_timeout(Duration::from_secs(5)) {
        Ok(is_err) => assert!(is_err, "EOF 는 에러로 보고돼야 한다"),
        Err(_) => panic!("EOF 후 send 가 반환하지 않았다 — 무한 스핀 회귀"),
    }
}
