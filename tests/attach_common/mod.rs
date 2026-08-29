//! attach 스트림 프로토콜의 frame/handshake 헬퍼 — `attach_*` test binary 들이 공유한다.
//!
//! attach client 는 실제 `tasty` GUI 앱이 아니라 raw `TcpStream` 으로 직접
//! 핸드셰이크한다. 서버는 transport 를 모르고 항상 loopback 으로만 받으므로
//! (`docs/dev-guide/attach-behavior.md`), 실제 client(`AttachClientSession`,
//! `src/app/attach_client.rs`)와 동일한 프로토콜 바이트만 흉내내면 충분하다.
//!
//! `tests/common/mod.rs`(인스턴스 하네스)·`tests/webhook_common/mod.rs`(웹훅 하네스)와
//! 같은 층위의 세 번째 공유 test 모듈이다. 개별 `#[test]` 파일끼리는 서로 `mod` 할 수
//! 없지만, 디렉토리 모듈은 여러 test binary 가 각자 `mod attach_common;` 으로 가져갈 수
//! 있다 — 파일마다 헬퍼를 복제할 이유가 없다.
//!
//! **여기에 "첫 workspace 를 집는" 헬퍼는 두지 않는다.** 공유 인스턴스
//! (`common::shared()`) 위에서는 테스트마다 `create_workspace()` 로 자기 workspace 를
//! 만들어 점유해야 한다 — attach 점유는 workspace/surface 단위 lock 이라
//! (`src/core/attach.rs` 의 `OccupancyRegistry`) 서로 다른 workspace 를 잡는 테스트끼리는
//! 한 인스턴스 위에서 병렬 공존한다. 전부 `workspace.list[0]` 을 집으면 그 성질이 깨진다.

// test binary 마다 쓰는 부분집합이 달라 개별 binary 기준 dead_code 판정이 무의미하다
// (의도된 superset API) — `tests/common/mod.rs` 와 같은 이유.
#![allow(dead_code)]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use serde_json::{Value, json};

/// mux `Data` 프레임 태그 (surface 출력 바이트).
pub const TAG_DATA: u8 = 0;
/// control 프레임 태그 (JSON 이벤트).
pub const TAG_CONTROL: u8 = 1;

/// handshake 이후 프레임을 기다리는 상한. 서버가 조용해도 테스트가 영원히 매달리지
/// 않도록 `read_exact` 에 걸어 둔다.
const FRAME_READ_TIMEOUT: Duration = Duration::from_secs(15);

/// 프레임 하나를 읽어 `(tag, payload)` 로 돌려준다. 헤더는 `tag(1) + len(4, BE)`.
pub fn read_frame(stream: &mut TcpStream) -> (u8, Vec<u8>) {
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

/// control 프레임 하나를 보낸다.
pub fn write_control_frame(stream: &mut TcpStream, payload: &Value) {
    let bytes = serde_json::to_vec(payload).unwrap();
    let mut hdr = [0u8; 5];
    hdr[0] = TAG_CONTROL;
    hdr[1..5].copy_from_slice(&(bytes.len() as u32).to_be_bytes());
    stream.write_all(&hdr).expect("write frame header");
    stream.write_all(&bytes).expect("write frame payload");
}

/// `stream.open` 핸드셰이크 요청까지 보낸 연결을 만든다. 응답(ack/attach 이벤트)은
/// 호출자가 목적에 맞게 읽는다.
fn open_stream(port: u16, params: Value) -> TcpStream {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect to server");
    stream
        .set_read_timeout(Some(FRAME_READ_TIMEOUT))
        .expect("set read timeout");

    let req = json!({
        "jsonrpc": "2.0",
        "method": "stream.open",
        "params": params,
        "id": 1,
    });
    let mut msg = serde_json::to_string(&req).unwrap();
    msg.push('\n');
    stream.write_all(msg.as_bytes()).expect("send handshake");
    stream
}

/// `stream.open{target_workspace}` 핸드셰이크. attach 성공을 나타내는
/// `{"event":"attached_workspace",...}` control 프레임까지 읽고 연결을 반환한다
/// (ack·터미널 초기 스냅샷 등 무관한 프레임은 건너뛴다).
pub fn open_workspace_attach(port: u16, workspace_id: u64) -> TcpStream {
    let mut stream = open_stream(port, json!({"proto": 1, "target_workspace": workspace_id}));

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

/// `stream.open{target}` — **surface 단위** attach. ack 를 확인한 뒤 뒤따르는 attach
/// 결과 control 프레임을 그대로 돌려준다(`attached` / `attach_error` 양쪽을 관측해야
/// 하는 테스트가 있으므로 여기서 성공을 단정하지 않는다).
///
/// 연결은 살려서 반환한다 — drop 하면 FIN 이 나가 silent disconnect 가 아니라 EOF
/// 케이스가 돼버린다.
pub fn open_surface_attach(port: u16, surface_id: u64) -> (TcpStream, Value) {
    let mut stream = open_stream(port, json!({"proto": 1, "target": surface_id}));

    let (tag, payload) = read_frame(&mut stream);
    assert_eq!(tag, TAG_CONTROL, "expected control ack frame");
    let ack: Value = serde_json::from_slice(&payload).unwrap();
    assert_eq!(ack["ok"], true, "handshake rejected: {ack:?}");

    let (tag, payload) = read_frame(&mut stream);
    assert_eq!(tag, TAG_CONTROL, "expected attach control frame");
    let ctrl: Value = serde_json::from_slice(&payload).unwrap();
    (stream, ctrl)
}

/// `stream.open` 을 target/target_workspace 없이 열어(단순 upgrade — client_id 는
/// 할당되지만 어떤 workspace 도 점유하지 않은 채) ack 프레임까지만 읽고 반환한다.
/// "attach 점유 없는 client" 를 재현하는 용도(하이브리드 신뢰 모델, ADR-0053 결정 3).
pub fn open_stream_without_attach(port: u16) -> TcpStream {
    let mut stream = open_stream(port, json!({"proto": 1}));

    let (tag, payload) = read_frame(&mut stream);
    assert_eq!(tag, TAG_CONTROL, "expected control ack frame");
    let ack: Value = serde_json::from_slice(&payload).unwrap();
    assert_eq!(ack["ok"], true, "handshake rejected: {ack:?}");
    stream
}

/// 주어진 `event` 이름의 control 프레임이 올 때까지 읽는다. 터미널 스냅샷·구조 델타 등
/// 무관한 프레임은 건너뛴다.
pub fn wait_for_control_event(stream: &mut TcpStream, event: &str) -> Value {
    loop {
        let (tag, payload) = read_frame(stream);
        if tag != TAG_CONTROL {
            continue;
        }
        let v: Value = serde_json::from_slice(&payload).unwrap();
        if v.get("event").and_then(|e| e.as_str()) == Some(event) {
            return v;
        }
    }
}
