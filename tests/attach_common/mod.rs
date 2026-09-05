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
use std::sync::Mutex;
use std::time::Duration;

use serde_json::{Value, json};

/// mux `Data` 프레임 태그 (surface 출력 바이트).
pub const TAG_DATA: u8 = 0;
/// control 프레임 태그 (JSON 이벤트).
pub const TAG_CONTROL: u8 = 1;
/// heartbeat 프레임 태그 (빈 payload) — `tasty_ipc::stream::StreamTag::Ping`.
pub const TAG_PING: u8 = 2;

/// **client 도 살아 있다고 말해야 한다.** 서버는 attach 소켓에 자기 read timeout
/// (`tasty_ipc::stream::HEARTBEAT_TIMEOUT` = 20 s)을 걸고, 그 동안 client 가
/// **아무것도 안 보내면 죽은 peer 로 보고 연결을 닫는다**. 서버가 5 초마다 Ping 을
/// 흘려주는 것은 *client* 의 read timeout 을 갱신할 뿐, 그 반대 방향은 갱신하지 않는다.
///
/// 실제 client 는 양쪽을 다 한다(`src/app/attach_client.rs`·`crates/tasty-cli` 의
/// `StreamTag::Ping` 송신). 이 raw 하네스는 읽는 절반만 흉내내고 있었고, 그래서
/// **한 번의 attach 교환이 20 초를 넘기는 순간** 서버가 닫아 `UnexpectedEof` 가 났다.
/// 실측(4-way 동시 실행, 8/8): 첫 read 이후 20.196 ~ 20.708 s.
///
/// 부하는 교환이 20 초를 넘느냐만 바꾼다 — **기전은 부하 없이도 성립한다.**
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);

/// 프레임 쓰기를 직렬화한다. 헤더와 payload 가 `write_all` **두 번**이라, 그 사이에
/// heartbeat 가 끼어들면 프레임 경계가 깨진다 — 서버는 그걸 unknown tag 로 읽고 끊는다.
static WRITE_LOCK: Mutex<()> = Mutex::new(());

/// 이 연결이 살아 있는 동안 `HEARTBEAT_INTERVAL` 마다 빈 Ping 을 보낸다.
///
/// 소켓이 닫히면(테스트 종료로 `TcpStream` 이 drop 되면) 쓰기가 실패하고 스레드가
/// 끝난다 — 별도 종료 신호를 두지 않는 이유다.
fn spawn_heartbeat(stream: &TcpStream) {
    let Ok(mut w) = stream.try_clone() else {
        // 복제 실패는 heartbeat 없이 진행한다는 뜻이라 조용히 넘기지 않는다.
        panic!("attach heartbeat 용 소켓 복제 실패");
    };
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(HEARTBEAT_INTERVAL);
            let _guard = WRITE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let hdr = [TAG_PING, 0, 0, 0, 0];
            if w.write_all(&hdr).is_err() {
                break;
            }
        }
    });
}

/// handshake 이후 프레임을 기다리는 상한. 서버가 조용해도 테스트가 영원히 매달리지
/// 않도록 `read_exact` 에 걸어 둔다.
const FRAME_READ_TIMEOUT: Duration = Duration::from_secs(15);

/// control 프레임이 올 때까지 읽으며 **서버의 idle Ping 을 건너뛴다.**
///
/// 서버는 sink 가 5 초 조용하면 빈 Ping 을 흘린다. ack 가 그보다 늦게 오는 회차
/// (부하가 높은 러너)에서는 첫 프레임이 Ping 이라, 태그를 바로 단정하는 자리가
/// `left: 2` 로 깨진다. 실제 client 는 전부 Ping 을 무시한다
/// (`src/app/attach_client.rs` 의 `StreamTag::Ping => {}`) — 여기서도 같게 한다.
pub fn read_control_frame(stream: &mut TcpStream) -> Vec<u8> {
    loop {
        let (tag, payload) = read_frame(stream);
        if tag == TAG_PING {
            continue;
        }
        assert_eq!(tag, TAG_CONTROL, "expected control frame");
        return payload;
    }
}

/// 프레임 하나를 읽어 `(tag, payload)` 로 돌려준다. 헤더는 `tag(1) + len(4, BE)`.
pub fn read_frame(stream: &mut TcpStream) -> (u8, Vec<u8>) {
    read_frame_result(stream).expect("read frame")
}

/// panic 하지 않는 판 — **연결이 살아 있는지 자체를 단정하는 자리**가 쓴다.
/// `UnexpectedEof`(서버가 닫음)와 `WouldBlock`(상한 초과)은 서로 다른 사건이라,
/// 그 구분이 필요한 단정은 오류를 그대로 받아야 한다.
pub fn read_frame_result(stream: &mut TcpStream) -> std::io::Result<(u8, Vec<u8>)> {
    let mut hdr = [0u8; 5];
    stream.read_exact(&mut hdr)?;
    let tag = hdr[0];
    let len = u32::from_be_bytes([hdr[1], hdr[2], hdr[3], hdr[4]]) as usize;
    let mut payload = vec![0u8; len];
    if len > 0 {
        stream.read_exact(&mut payload)?;
    }
    Ok((tag, payload))
}

/// control 프레임 하나를 보낸다.
pub fn write_control_frame(stream: &mut TcpStream, payload: &Value) {
    let bytes = serde_json::to_vec(payload).unwrap();
    let mut hdr = [0u8; 5];
    hdr[0] = TAG_CONTROL;
    hdr[1..5].copy_from_slice(&(bytes.len() as u32).to_be_bytes());
    // heartbeat 스레드와 프레임이 섞이지 않게 한 프레임을 통째로 잠그고 쓴다.
    let _guard = WRITE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
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
    // **살아 있는 client 를 흉내내는 것은 여기뿐이다.** heartbeat 는 점유를 유지시키므로
    // (서버는 침묵을 죽음으로 보고 점유를 회수한다) 침묵 자체를 시험하는 헬퍼
    // (`open_surface_attach`·`open_stream_without_attach`·`try_open_*`)에는 절대 걸지
    // 않는다 — 걸면 그 테스트들이 검증하려는 TTL 회수가 영영 안 일어난다.
    spawn_heartbeat(&stream);

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

    let payload = read_control_frame(&mut stream);
    let ack: Value = serde_json::from_slice(&payload).unwrap();
    assert_eq!(ack["ok"], true, "handshake rejected: {ack:?}");

    let payload = read_control_frame(&mut stream);
    let ctrl: Value = serde_json::from_slice(&payload).unwrap();
    (stream, ctrl)
}

/// `stream.open` 을 target/target_workspace 없이 열어(단순 upgrade — client_id 는
/// 할당되지만 어떤 workspace 도 점유하지 않은 채) ack 프레임까지만 읽고 반환한다.
/// "attach 점유 없는 client" 를 재현하는 용도(하이브리드 신뢰 모델, ADR-0053 결정 3).
pub fn open_stream_without_attach(port: u16) -> TcpStream {
    let mut stream = open_stream(port, json!({"proto": 1}));

    let payload = read_control_frame(&mut stream);
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

/// `stream.open{target_workspace}` 요청만 보내고 **아무 프레임도 읽지 않은** 연결.
/// 핸드셰이크 실패(프로토콜 불일치·즉시 끊김) 재현용.
pub fn raw_open_workspace_no_read(port: u16, workspace_id: u64) -> TcpStream {
    open_stream(port, json!({"proto": 1, "target_workspace": workspace_id}))
}

/// 임의 `proto` 값으로 workspace attach 핸드셰이크만 보낸 연결(읽지 않음).
pub fn raw_open_workspace_proto(port: u16, workspace_id: u64, proto: u32) -> TcpStream {
    open_stream(
        port,
        json!({"proto": proto, "target_workspace": workspace_id}),
    )
}

/// workspace attach 를 시도하고 결과 이벤트 이름을 돌려준다(panic 하지 않는다).
/// 성공은 `"attached_workspace"`, 거절은 `"attach_error:<reason>"`.
///
/// **연결을 반환하지 않는다** — 호출 직후 소켓이 drop 되므로, 성공했다면 그 점유는
/// 곧 EOF 로 회수된다. "이 시점에 attach 가 되는가" 만 묻는 프로브용이다.
pub fn try_open_workspace_attach(port: u16, workspace_id: u64) -> String {
    try_open_workspace_attach_inner(open_stream(
        port,
        json!({"proto": 1, "target_workspace": workspace_id}),
    ))
}

/// `try_open_workspace_attach` 에 `session_token` 을 실은 판. 스트림 채널이 토큰을
/// 보지 않는다는 사실(인증 부재 = 토큰 기반 거절 경로 없음)을 고정하는 데 쓴다.
pub fn try_open_workspace_attach_with_token(port: u16, workspace_id: u64, token: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect to server");
    stream
        .set_read_timeout(Some(FRAME_READ_TIMEOUT))
        .expect("set read timeout");
    let req = json!({
        "jsonrpc": "2.0",
        "method": "stream.open",
        "params": {"proto": 1, "target_workspace": workspace_id},
        "session_token": token,
        "id": 1,
    });
    let mut msg = serde_json::to_string(&req).unwrap();
    msg.push('\n');
    stream.write_all(msg.as_bytes()).expect("send handshake");
    try_open_workspace_attach_inner(stream)
}

fn try_open_workspace_attach_inner(stream: TcpStream) -> String {
    let mut stream = stream;
    loop {
        let (tag, payload) = read_frame(&mut stream);
        if tag != TAG_CONTROL {
            continue;
        }
        let v: Value = serde_json::from_slice(&payload).unwrap();
        if v.get("ok").is_some() {
            continue;
        }
        match v.get("event").and_then(|e| e.as_str()) {
            Some("attached_workspace") => return "attached_workspace".into(),
            Some("attach_error") => {
                return format!(
                    "attach_error:{}",
                    v.get("reason").and_then(|r| r.as_str()).unwrap_or("?")
                );
            }
            _ => continue,
        }
    }
}
