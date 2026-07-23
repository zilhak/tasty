//! (04) Native file picker — 원격 attach 채널의 `list_dir_request`/`list_dir_result`
//! 왕복을 loopback `TcpStream` 으로 실제 실행 중인 서버 인스턴스에 대해 검증한다.
//!
//! `tests/attach_silent_disconnect.rs` 와 동일한 접근: attach client 는 실제
//! `tasty` GUI 앱이 아니라 raw `TcpStream` 으로 직접 핸드셰이크한다 — 서버는
//! transport 를 모르고 항상 loopback 으로만 받으므로, 실제 client(`AttachClientSession`,
//! `src/app/attach_client.rs`)와 동일한 프로토콜 바이트만 흉내내면 충분하다.
//!
//! **GUI 두 인스턴스를 실제로 attach 하는 e2e**(`tasty tool attach --ssh
//! 127.0.0.1:<port>` 로 mirror workspace 를 만들고 popup 을 열어 눈으로 확인하는 것)는
//! 이 headless 작업 환경(GPU 디스플레이 없음)에서 실행할 수 없다 — 이 test 는 그
//! 대체로, 서버가 실제로 띄운 워크스페이스에 대해 (1) attach 점유 획득 →
//! (2) `list_dir_request` 전송 → (3) 서버측 `handle_list_dir_request` 가 실제
//! 디스크의 임시 디렉토리를 읽어 → (4) `list_dir_result` 로 정확히 회신하는 전체
//! 왕복을 프로토콜 레벨에서 실행한다. `docs/features/native-file-picker/index.md` 의
//! "검증 한계" 절 참고.

mod common;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use common::TastyInstance;
use serde_json::{Value, json};

const TAG_CONTROL: u8 = 1;

fn read_frame(stream: &mut TcpStream) -> (u8, Vec<u8>) {
    let mut hdr = [0u8; 5];
    stream.read_exact(&mut hdr).expect("read frame header");
    let tag = hdr[0];
    let len = u32::from_be_bytes([hdr[1], hdr[2], hdr[3], hdr[4]]) as usize;
    let mut payload = vec![0u8; len];
    if len > 0 {
        stream.read_exact(&mut payload).expect("read frame payload");
    }
    (tag, payload)
}

fn write_control_frame(stream: &mut TcpStream, payload: &Value) {
    let bytes = serde_json::to_vec(payload).unwrap();
    let mut hdr = [0u8; 5];
    hdr[0] = TAG_CONTROL;
    hdr[1..5].copy_from_slice(&(bytes.len() as u32).to_be_bytes());
    stream.write_all(&hdr).expect("write frame header");
    stream.write_all(&bytes).expect("write frame payload");
}

/// `stream.open{target_workspace}` 핸드셰이크. attach 성공을 나타내는
/// `{"event":"attached_workspace",...}` control 프레임까지 읽고 연결을 반환한다
/// (다른 control 프레임 — ack, 터미널 스냅샷 등 — 은 건너뛴다).
fn open_workspace_attach(port: u16, workspace_id: u64) -> TcpStream {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect to server");
    stream
        .set_read_timeout(Some(Duration::from_secs(15)))
        .expect("set read timeout");

    let req = json!({
        "jsonrpc": "2.0",
        "method": "stream.open",
        "params": {"proto": 1, "target_workspace": workspace_id},
        "id": 1,
    });
    let mut msg = serde_json::to_string(&req).unwrap();
    msg.push('\n');
    stream.write_all(msg.as_bytes()).expect("send handshake");

    loop {
        let (tag, payload) = read_frame(&mut stream);
        if tag != TAG_CONTROL {
            continue;
        }
        let v: Value = serde_json::from_slice(&payload).unwrap();
        if let Some(ok) = v.get("ok") {
            assert_eq!(ok, true, "handshake rejected: {v:?}");
            continue;
        }
        match v.get("event").and_then(|e| e.as_str()) {
            Some("attached_workspace") => return stream,
            Some("attach_error") => panic!("workspace attach rejected: {v:?}"),
            _ => continue, // 터미널 스냅샷 등 무관한 control 프레임 — 계속 대기.
        }
    }
}

fn first_workspace_id(instance: &TastyInstance) -> u64 {
    let workspaces = instance.call("workspace.list", json!({}));
    workspaces.as_array().unwrap()[0]["id"].as_u64().unwrap()
}

/// `stream.open` 을 target/target_workspace 없이 열어(단순 upgrade, client_id 는
/// 할당되지만 어떤 workspace 도 점유하지 않은 채) ack 프레임까지만 읽고 반환한다 —
/// "attach 점유 없는 client" 를 재현하는 용도(하이브리드 신뢰 모델의 원격 브랜치
/// 검증, ADR-0053 결정 3).
fn open_stream_without_attach(port: u16) -> TcpStream {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect to server");
    stream
        .set_read_timeout(Some(Duration::from_secs(15)))
        .expect("set read timeout");

    let req = json!({
        "jsonrpc": "2.0",
        "method": "stream.open",
        "params": {"proto": 1},
        "id": 1,
    });
    let mut msg = serde_json::to_string(&req).unwrap();
    msg.push('\n');
    stream.write_all(msg.as_bytes()).expect("send handshake");

    let (tag, payload) = read_frame(&mut stream);
    assert_eq!(tag, TAG_CONTROL, "expected control ack frame");
    let ack: Value = serde_json::from_slice(&payload).unwrap();
    assert_eq!(ack["ok"], true, "handshake rejected: {ack:?}");
    stream
}

#[test]
fn list_dir_request_round_trips_over_attach_channel() {
    let server = TastyInstance::spawn();
    let ws_id = first_workspace_id(&server);

    // 실제 디스크에 검증용 디렉토리를 만든다 — read_dir_entries 가 실제 파일시스템을
    // 읽는지(mock 아님) 확인하기 위함.
    let dir = std::env::temp_dir().join(format!(
        "tasty_list_dir_loopback_{}_{}",
        std::process::id(),
        server.pid()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("sub_folder")).unwrap();
    std::fs::write(dir.join("hello.txt"), b"hello world").unwrap();

    let mut stream = open_workspace_attach(server.port(), ws_id);

    write_control_frame(
        &mut stream,
        &json!({
            "event": "list_dir_request",
            "request_id": 1,
            "dir": dir.to_string_lossy(),
        }),
    );

    let result = loop {
        let (tag, payload) = read_frame(&mut stream);
        if tag != TAG_CONTROL {
            continue;
        }
        let v: Value = serde_json::from_slice(&payload).unwrap();
        if v.get("event").and_then(|e| e.as_str()) == Some("list_dir_result") {
            break v;
        }
        // 터미널 스냅샷/구조 델타 등 무관한 control 프레임 — 계속 대기.
    };

    assert_eq!(result["request_id"], 1);
    assert_eq!(result["ok"], true, "expected ok reply: {result:?}");
    assert_eq!(result["dir"], dir.to_string_lossy().as_ref());
    let entries = result["entries"].as_array().expect("entries array");
    assert_eq!(entries.len(), 2, "expected 2 entries: {entries:?}");
    let names: Vec<&str> = entries
        .iter()
        .map(|e| e["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"sub_folder"));
    assert!(names.contains(&"hello.txt"));
    let sub = entries.iter().find(|e| e["name"] == "sub_folder").unwrap();
    assert_eq!(sub["is_dir"], true);
    let file = entries.iter().find(|e| e["name"] == "hello.txt").unwrap();
    assert_eq!(file["is_dir"], false);
    assert_eq!(file["size"], 11);
    assert!(
        file["modified_unix"].as_u64().unwrap() > 0,
        "modified_unix should be a positive epoch: {file:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn list_dir_request_reports_permission_error_for_missing_dir() {
    let server = TastyInstance::spawn();
    let ws_id = first_workspace_id(&server);
    let mut stream = open_workspace_attach(server.port(), ws_id);

    let missing = std::env::temp_dir().join(format!(
        "tasty_list_dir_loopback_missing_{}_{}",
        std::process::id(),
        server.pid()
    ));
    let _ = std::fs::remove_dir_all(&missing);

    write_control_frame(
        &mut stream,
        &json!({
            "event": "list_dir_request",
            "request_id": 2,
            "dir": missing.to_string_lossy(),
        }),
    );

    let result = loop {
        let (tag, payload) = read_frame(&mut stream);
        if tag != TAG_CONTROL {
            continue;
        }
        let v: Value = serde_json::from_slice(&payload).unwrap();
        if v.get("event").and_then(|e| e.as_str()) == Some("list_dir_result") {
            break v;
        }
    };

    assert_eq!(result["request_id"], 2);
    assert_eq!(result["ok"], false, "expected error reply: {result:?}");
    assert!(result["reason"].as_str().is_some_and(|r| !r.is_empty()));
}

#[test]
fn list_dir_request_rejected_without_workspace_occupancy() {
    // 하이브리드 신뢰 모델(ADR-0053 결정 3): attach 점유가 유일한 인가 조건이다.
    // 이 client 는 stream 을 upgrade 했을 뿐 어떤 workspace 도 점유하지 않았으므로
    // `client_holds_workspace` 가 false 여야 하고, 서버는 실제 파일시스템을 읽지
    // 않은 채 즉시 거부해야 한다.
    let server = TastyInstance::spawn();
    let mut stream = open_stream_without_attach(server.port());

    write_control_frame(
        &mut stream,
        &json!({
            "event": "list_dir_request",
            "request_id": 3,
            "dir": "/",
        }),
    );

    let result = loop {
        let (tag, payload) = read_frame(&mut stream);
        if tag != TAG_CONTROL {
            continue;
        }
        let v: Value = serde_json::from_slice(&payload).unwrap();
        if v.get("event").and_then(|e| e.as_str()) == Some("list_dir_result") {
            break v;
        }
    };

    assert_eq!(result["request_id"], 3);
    assert_eq!(
        result["ok"], false,
        "unattached client must be rejected: {result:?}"
    );
    assert!(result["entries"].is_null(), "no entries on rejection");
}
