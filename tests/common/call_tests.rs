//! 공유 하네스의 요청 교환을 이 시험이 소유한 loopback 서버로 검증한다.
use super::common;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::time::Duration;

/// 서버가 요청 줄을 읽기까지 기다리는 상한. 부하에서 accept와 읽기가 늦어도 도착을 놓치지 않는다.
const ARRIVAL_WAIT: Duration = Duration::from_secs(10);
/// 첫 요청 뒤 추가 요청이 없는지 보는 시간. 재전송하던 이전 동작은 클라이언트가 돌아오기 전에 두 번 더 보냈다.
const EXTRA_WAIT: Duration = Duration::from_secs(2);

/// 요청 줄을 읽을 때마다 채널로 알리고 `delay` 뒤에 성공 응답을 쓰는 서버를 띄운다.
fn counting_server(delay: Duration) -> (u16, mpsc::Receiver<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (arrived, received) = mpsc::channel();
    std::thread::spawn(move || {
        for incoming in listener.incoming() {
            let Ok(mut stream) = incoming else {
                continue;
            };
            let arrived = arrived.clone();
            std::thread::spawn(move || {
                let mut line = String::new();
                if BufReader::new(&stream).read_line(&mut line).unwrap_or(0) == 0 {
                    return;
                }
                if arrived.send(()).is_err() {
                    return; // 시험이 이미 끝났다.
                }
                std::thread::sleep(delay);
                // 클라이언트가 먼저 끊었으면 쓰기가 실패한다. 이 시험은 받은 요청 수만 본다.
                writeln!(
                    stream,
                    r#"{{"jsonrpc":"2.0","id":1,"result":{{"ok":true}}}}"#
                )
                .ok();
            });
        }
    });
    (port, received)
}

/// 첫 요청 도착을 기다린 뒤 추가 요청이 오지 않는지 본다. 도착 수를 반환한다.
fn arrivals(received: &mpsc::Receiver<()>) -> usize {
    if received.recv_timeout(ARRIVAL_WAIT).is_err() {
        return 0;
    }
    1 + std::iter::from_fn(|| received.recv_timeout(EXTRA_WAIT).ok()).count()
}

/// 응답을 못 받은 요청은 실행됐을 수 있다. 다시 보내면 workspace.create 같은 요청이 두 번 실행된다.
#[test]
fn a_request_without_a_response_in_time_is_not_sent_again() {
    let (port, received) = counting_server(Duration::from_secs(5));
    let result = common::exchange(
        port,
        "workspace.create",
        &serde_json::json!({}),
        Duration::from_millis(200),
    );
    assert_eq!(
        arrivals(&received),
        1,
        "보낸 요청은 응답이 늦어도 한 번만 서버에 도착해야 한다"
    );
    let error = result.expect_err("the server answers after the wait");
    assert!(
        error.contains("not sent again"),
        "실패 메시지가 재전송하지 않았다는 사실을 알려야 한다: {error}"
    );
}

#[test]
fn a_late_response_within_the_wait_is_returned() {
    let (port, received) = counting_server(Duration::from_millis(300));
    let response = common::exchange(
        port,
        "workspace.create",
        &serde_json::json!({}),
        Duration::from_secs(10),
    )
    .expect("the response arrives within the wait");
    assert_eq!(response["result"]["ok"], true, "{response}");
    assert_eq!(arrivals(&received), 1);
}
