//! attach 시험들이 공유하는 프레임·handshake 헬퍼다.
//! 클라이언트 GUI 대신 loopback TcpStream으로 프로토콜을 교환한다.
//! 공유 서버에서는 각 시험이 만든 workspace를 점유해야 서로의 점유 상태에 간섭하지 않는다.

// 이유: 시험 하네스의 정리 실패는 제품 코드의 오류 처리 명부와 구분한다.
#![allow(clippy::let_underscore_must_use)]
// 이유: 바이너리마다 사용하는 헬퍼가 달라 공유 API 일부가 사용되지 않을 수 있다.
#![allow(dead_code)]

use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream};
use std::ops::{Deref, DerefMut};
use std::sync::Mutex;
use std::time::Duration;

use serde_json::{Value, json};

pub const TAG_DATA: u8 = 0;
pub const TAG_CONTROL: u8 = 1;
pub const TAG_PING: u8 = 2;

/// 서버의 Ping만 읽어서는 서버 쪽 읽기 타임아웃이 갱신되지 않는다. 클라이언트도 주기적으로 Ping을 보내야 연결을 유지한다.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);

/// 헤더와 payload를 따로 쓰므로 heartbeat가 사이에 끼지 않도록 한 프레임 전체를 잠근다.
static WRITE_LOCK: Mutex<()> = Mutex::new(());

/// heartbeat 스레드가 소켓 복제본을 보유하므로 원본 drop만으로 닫히지 않는다. wrapper의 Drop에서 소켓 전체를 shutdown한다.
pub struct AttachStream {
    inner: TcpStream,
}

impl Deref for AttachStream {
    type Target = TcpStream;
    fn deref(&self) -> &TcpStream {
        &self.inner
    }
}

impl DerefMut for AttachStream {
    fn deref_mut(&mut self) -> &mut TcpStream {
        &mut self.inner
    }
}

impl Drop for AttachStream {
    fn drop(&mut self) {
        // 이미 닫혔을 수 있어 종료 요청 실패는 무시한다.
        let _ = self.inner.shutdown(Shutdown::Both);
    }
}

/// 주기적으로 Ping을 보내고 쓰기가 실패하면 끝낸다. 소켓 종료는 AttachStream의 Drop이 담당한다.
fn spawn_heartbeat(stream: &TcpStream) {
    let Ok(mut w) = stream.try_clone() else {
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

/// 조용한 서버에 무한히 기다리지 않도록 프레임 읽기에 적용하는 제한 시간.
const FRAME_READ_TIMEOUT: Duration = Duration::from_secs(15);

/// control 프레임을 기다리는 동안 서버의 idle Ping은 건너뛴다.
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

/// 헤더 tag 1바이트·길이 4바이트(BE)와 payload를 읽는다.
pub fn read_frame(stream: &mut TcpStream) -> (u8, Vec<u8>) {
    read_frame_result(stream).unwrap_or_else(|e| panic!("{}", frame_io_failure("프레임 읽기", &e)))
}

/// 연결 종료 계열 오류와 읽기 타임아웃을 구분한다. 타임아웃만으로 연결 생존을 단정하지 않는다.
fn frame_io_failure(op: &str, e: &std::io::Error) -> String {
    use std::io::ErrorKind::*;
    match e.kind() {
        UnexpectedEof | BrokenPipe | ConnectionReset | ConnectionAborted => format!(
            "{op} 중 연결 종료 오류가 발생했다: {e:?}\n클라이언트가 {}초 동안 아무것도 보내지 않으면 서버가 연결을 닫을 수 있다. 침묵을 검증하는 시험이 아니라면 heartbeat 적용을 확인한다.",
            tasty_ipc::stream::HEARTBEAT_TIMEOUT.as_secs()
        ),
        WouldBlock | TimedOut => format!(
            "{op} 중 제한 시간 {:?} 안에 프레임을 받지 못했다: {e:?}",
            FRAME_READ_TIMEOUT
        ),
        _ => format!("{op} 실패: {e:?}"),
    }
}

/// 호출자가 종료·타임아웃을 구별할 수 있도록 I/O 오류를 그대로 반환한다.
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

/// 점유한 surface의 셸 입력은 서버 로컬 IPC 대신 surface ID를 붙인 Data 프레임으로 보낸다.
pub fn write_workspace_input(stream: &mut TcpStream, surface_id: u32, bytes: &[u8]) {
    let payload = tasty_ipc::stream::encode_mux(surface_id, bytes);
    let mut hdr = [0u8; 5];
    hdr[0] = TAG_DATA;
    hdr[1..5].copy_from_slice(&(payload.len() as u32).to_be_bytes());
    let _guard = WRITE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    stream
        .write_all(&hdr)
        .unwrap_or_else(|e| panic!("{}", frame_io_failure("프레임 헤더 쓰기", &e)));
    stream
        .write_all(&payload)
        .unwrap_or_else(|e| panic!("{}", frame_io_failure("프레임 payload 쓰기", &e)));
}

pub fn write_control_frame(stream: &mut TcpStream, payload: &Value) {
    let bytes = serde_json::to_vec(payload).unwrap();
    let mut hdr = [0u8; 5];
    hdr[0] = TAG_CONTROL;
    hdr[1..5].copy_from_slice(&(bytes.len() as u32).to_be_bytes());
    let _guard = WRITE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    stream
        .write_all(&hdr)
        .unwrap_or_else(|e| panic!("{}", frame_io_failure("프레임 헤더 쓰기", &e)));
    stream
        .write_all(&bytes)
        .unwrap_or_else(|e| panic!("{}", frame_io_failure("프레임 payload 쓰기", &e)));
}

/// stream.open 요청까지만 보내고 ack·attach 결과는 호출자가 읽는다.
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

/// workspace attach의 성공 이벤트를 기다려 연결을 반환한다. ack·초기 스냅샷은 건너뛴다.
pub fn open_workspace_attach(port: u16, workspace_id: u64) -> AttachStream {
    open_workspace_attach_with_descriptor(port, workspace_id).0
}

/// 성공 이벤트의 트리·surface 역할도 반환한다.
pub fn open_workspace_attach_with_descriptor(
    port: u16,
    workspace_id: u64,
) -> (AttachStream, Value) {
    let mut stream = open_stream(port, json!({"proto": 1, "target_workspace": workspace_id}));
    // 침묵에 따른 TTL 회수를 검증하는 surface/raw 헬퍼에는 heartbeat를 추가하지 않는다. 프레임 교환이 목적인 연결에만 사용한다.
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
            Some("attached_workspace") => return (AttachStream { inner: stream }, v),
            Some("attach_error") => panic!("workspace attach rejected: {v:?}"),
            _ => continue, // 터미널 스냅샷 등 무관한 control 프레임 — 계속 대기.
        }
    }
}

/// attach_error이면 None을 반환한다. handshake·I/O 실패는 다른 헬퍼처럼 panic한다.
pub fn try_open_workspace_attach_stream(
    port: u16,
    workspace_id: u64,
) -> Option<(AttachStream, Value)> {
    let mut stream = open_stream(port, json!({"proto": 1, "target_workspace": workspace_id}));
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
            Some("attached_workspace") => return Some((AttachStream { inner: stream }, v)),
            Some("attach_error") => return None,
            _ => continue,
        }
    }
}

/// surface attach의 ack 뒤 결과를 그대로 반환한다. TTL 회수를 관측할 수 있도록 heartbeat 없이 연결을 유지한다.
pub fn open_surface_attach(port: u16, surface_id: u64) -> (TcpStream, Value) {
    let mut stream = open_stream(port, json!({"proto": 1, "target": surface_id}));

    let payload = read_control_frame(&mut stream);
    let ack: Value = serde_json::from_slice(&payload).unwrap();
    assert_eq!(ack["ok"], true, "handshake rejected: {ack:?}");

    let payload = read_control_frame(&mut stream);
    let ctrl: Value = serde_json::from_slice(&payload).unwrap();
    (stream, ctrl)
}

/// 대상 없이 stream만 열고 ack를 읽는다. 점유가 없어도 읽기 타임아웃은 적용되므로 프레임 교환 동안 heartbeat를 보낸다.
pub fn open_stream_without_attach(port: u16) -> AttachStream {
    let mut stream = heartbeating(open_stream(port, json!({"proto": 1})));

    let payload = read_control_frame(&mut stream);
    let ack: Value = serde_json::from_slice(&payload).unwrap();
    assert_eq!(ack["ok"], true, "handshake rejected: {ack:?}");
    stream
}

/// 지정한 control event를 기다리며 무관한 프레임은 건너뛴다.
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

/// workspace handshake 요청만 보내고 응답을 읽지 않는다. 즉시 종료·프로토콜 실패를 재현할 때 쓴다.
pub fn raw_open_workspace_no_read(port: u16, workspace_id: u64) -> TcpStream {
    open_stream(port, json!({"proto": 1, "target_workspace": workspace_id}))
}

/// 지정한 proto로 workspace handshake만 보내고 응답은 읽지 않는다.
pub fn raw_open_workspace_proto(port: u16, workspace_id: u64, proto: u32) -> TcpStream {
    open_stream(
        port,
        json!({"proto": proto, "target_workspace": workspace_id}),
    )
}

/// attach 결과 이름을 반환하고 연결은 닫는다. attach_error는 문자열로 반환하지만 I/O 실패는 panic한다.
pub fn try_open_workspace_attach(port: u16, workspace_id: u64) -> String {
    try_open_workspace_attach_inner(heartbeating(open_stream(
        port,
        json!({"proto": 1, "target_workspace": workspace_id}),
    )))
}

/// session_token을 실어 스트림의 토큰 기반 거절 여부를 확인한다.
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
    // 응답 대기 중에는 연결을 유지해야 하므로 heartbeat를 시작한다.
    try_open_workspace_attach_inner(heartbeating(stream))
}

/// heartbeat와 Drop 시 소켓 종료를 함께 적용한다.
fn heartbeating(stream: TcpStream) -> AttachStream {
    spawn_heartbeat(&stream);
    AttachStream { inner: stream }
}

fn try_open_workspace_attach_inner(stream: AttachStream) -> String {
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
