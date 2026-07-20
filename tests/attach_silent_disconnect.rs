//! Silent disconnect(EOF 없는 조용한 단절) 시 서버의 `OccupancyRegistry` 점유
//! lock 이 heartbeat TTL 만료로 자동 해제되는지 검증(TODO 05). 프로토콜 상세는
//! `crates/tasty-ipc/src/stream.rs`, 해제 경로는 `docs/dev-guide/attach-behavior.md`
//! "release 경로" 절 참고.
//!
//! attach client 는 `tasty` CLI/GUI 가 아니라 raw `TcpStream` 으로 직접 핸드셰이크한다
//! — 서버는 transport 를 모르고 항상 loopback 으로 받으므로(`docs/dev-guide/
//! attach-behavior.md`), 이 test 도 실제 client 구현과 동일한 프로토콜 바이트만
//! 흉내내면 충분하다. silent disconnect 는 소켓을 닫지 않고(FIN 미전송) 그냥
//! 아무 프레임도 더 보내지 않는 것으로 재현한다 — heartbeat Ping 을 멈추는 것이
//! 곧 "조용히 죽음" 이다.

mod common;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

use common::TastyInstance;
use serde_json::{Value, json};

const TAG_CONTROL: u8 = 1;

// crates/tasty-ipc/src/stream.rs 의 HEARTBEAT_TIMEOUT 과 동일 — 상수를 여기서
// 재선언하는 대신 값을 하드코딩하면 프로토콜 변경 시 조용히 어긋날 수 있으니,
// 여유를 넉넉히 둔 폴링 상한(아래 RELEASE_POLL_TIMEOUT)으로 실제 상수 변경에도
// 견고하게 만든다.
const HEARTBEAT_TIMEOUT_HINT: Duration = Duration::from_secs(20);
const RELEASE_POLL_TIMEOUT: Duration = Duration::from_secs(45);

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

/// `stream.open{target}` 핸드셰이크를 열고 (ack, attach 결과) 를 돌려준다.
/// 연결은 살려서 반환한다 — drop 하면 FIN 이 나가 EOF 케이스가 돼버린다.
fn open_attach(port: u16, surface_id: u64) -> (TcpStream, Value) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect to server");
    stream
        .set_read_timeout(Some(Duration::from_secs(15)))
        .expect("set read timeout");

    let req = json!({
        "jsonrpc": "2.0",
        "method": "stream.open",
        "params": {"proto": 1, "target": surface_id},
        "id": 1,
    });
    let mut msg = serde_json::to_string(&req).unwrap();
    msg.push('\n');
    stream.write_all(msg.as_bytes()).expect("send handshake");

    let (tag, payload) = read_frame(&mut stream);
    assert_eq!(tag, TAG_CONTROL, "expected control ack frame");
    let ack: Value = serde_json::from_slice(&payload).unwrap();
    assert_eq!(ack["ok"], true, "handshake rejected: {ack:?}");

    let (tag, payload) = read_frame(&mut stream);
    assert_eq!(tag, TAG_CONTROL, "expected attach control frame");
    let ctrl: Value = serde_json::from_slice(&payload).unwrap();
    (stream, ctrl)
}

fn is_attached(instance: &TastyInstance, surface_id: u64) -> bool {
    let surfaces = instance.call("surface.list", json!({}));
    surfaces
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"].as_u64() == Some(surface_id))
        .and_then(|s| s["attached"].as_bool())
        .unwrap_or(false)
}

#[test]
fn silent_disconnect_releases_occupancy_via_heartbeat_ttl() {
    let server = TastyInstance::spawn();
    let sid = server.first_surface_id();

    assert!(!is_attached(&server, sid), "surface must start unattached");

    // client A: attach 성공 → 점유 lock 획득.
    let (stale_conn, ctrl) = open_attach(server.port(), sid);
    assert_eq!(
        ctrl["event"].as_str(),
        Some("attached"),
        "attach should succeed: {ctrl:?}"
    );
    assert!(
        is_attached(&server, sid),
        "surface must show attached after a successful attach"
    );

    // TTL 만료 전: 새 client 는 AlreadyAttached 로 거부돼야 한다.
    let (_rejected_conn, reject_ctrl) = open_attach(server.port(), sid);
    assert_eq!(
        reject_ctrl["event"].as_str(),
        Some("attach_error"),
        "second attach must be rejected while the first holds the lock: {reject_ctrl:?}"
    );
    drop(_rejected_conn); // EOF 로 정리 — 거부된 연결이라 점유와 무관.

    // silent disconnect 재현: `stale_conn` 을 닫지 않고(FIN 미전송) 그냥 방치.
    // heartbeat Ping 을 더 이상 보내지 않으므로 서버의 read timeout
    // (HEARTBEAT_TIMEOUT) 이 유일한 감지 수단이 된다.
    let held_since = Instant::now();

    // TTL 만료 후: OccupancyRegistry 가 자동으로 free 로 돌아온다(코드 변경 없이
    // 04번의 read timeout → `Err(_) => break` → `Disconnected` →
    // `release_all_for_client` 체인만으로 동작 — TODO 05 핵심 가정 검증).
    let deadline = Instant::now() + RELEASE_POLL_TIMEOUT;
    let mut released = false;
    while Instant::now() < deadline {
        if !is_attached(&server, sid) {
            released = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    assert!(
        released,
        "occupancy lock was not released within {:?} of a silent disconnect \
         (heartbeat TTL hint: {:?})",
        RELEASE_POLL_TIMEOUT, HEARTBEAT_TIMEOUT_HINT
    );
    assert!(
        held_since.elapsed() >= Duration::from_secs(5),
        "sanity: release should not be instantaneous (would indicate EOF, not TTL)"
    );

    // 재attach: 같은 surface 에 새 client(C)가 성공해야 한다(과거엔
    // AlreadyAttached 로 거부됐을 상황).
    let (_reattached_conn, reattach_ctrl) = open_attach(server.port(), sid);
    assert_eq!(
        reattach_ctrl["event"].as_str(),
        Some("attached"),
        "re-attach after TTL release should succeed: {reattach_ctrl:?}"
    );
    assert!(is_attached(&server, sid));

    drop(stale_conn); // 정리 — 이미 서버측에서 release 됐으므로 이제 닫아도 무해.
}
