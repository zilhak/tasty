//! 클라이언트 IPC 연결([`tasty_ipc::client::IpcConnection`])의 EOF 회귀 격리.
//!
//! **통합 테스트(별도 바이너리)로 둔 이유**: 이 테스트는 ephemeral 포트를 잡고
//! 소켓에서 블록한다. 같은 바이너리에 방금 해제한 ephemeral 포트를 다시 bind 해
//! 보는 유닛 테스트가 있으면 병렬 실행 시 서로 포트를 가로챌 수 있다. cargo 는
//! 테스트 바이너리를 하나씩 돌리므로 분리하면 그 경합이 성립하지 않는다.

// 테스트 본문은 `let _ =` 사유 주석 정책의 범위 밖이다 — 전수 가드
// (`tests/let_underscore_documented.rs`)가 테스트 본문을 제외하므로, 여기서 나는
// `let_underscore_must_use` 경고는 정책상 조치 대상이 될 수 없다. 끄지 않으면
// 프로덕션의 진짜 신호가 그 안에 묻힌다 — `docs/dev-guide/error-handling.md`.
#![allow(clippy::let_underscore_must_use)]

use std::io::{BufRead, BufReader};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::time::Duration;

use tasty_ipc::client::IpcConnection;
use tasty_ipc::protocol::JsonRpcRequest;

/// 상대가 응답 없이 연결을 닫으면(EOF) **즉시 에러로 끝나야 한다.**
///
/// 회귀 시 증상이 "실패" 가 아니라 "코어 하나를 태우는 무한 스핀"(유저스페이스
/// 스핀이라 겉보기 hang 과 구분도 어렵다)이라 일반 assert 로는 잡히지 않는다 —
/// 별도 스레드에서 돌리고 `recv_timeout` 으로 판정한다. 호스트가 종료 중이거나
/// 크래시/SIGKILL 로 죽으면 실제로 밟는 경로다.
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
