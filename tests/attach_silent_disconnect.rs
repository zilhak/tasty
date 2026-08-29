//! Silent disconnect(EOF 없는 조용한 단절) 시 서버의 `OccupancyRegistry` 점유
//! lock 이 heartbeat TTL 만료로 자동 해제되는지 검증(ADR-0052). 프로토콜 상세는
//! `crates/tasty-ipc/src/stream.rs`, 해제 경로는 `docs/dev-guide/attach-behavior.md`
//! "release 경로" 절 참고.
//!
//! attach client 는 `tasty` CLI/GUI 가 아니라 raw `TcpStream` 으로 직접 핸드셰이크한다
//! — 서버는 transport 를 모르고 항상 loopback 으로 받으므로(`docs/dev-guide/
//! attach-behavior.md`), 이 test 도 실제 client 구현과 동일한 프로토콜 바이트만
//! 흉내내면 충분하다(frame/handshake 헬퍼는 `tests/attach_common/mod.rs` 공유).
//! silent disconnect 는 소켓을 닫지 않고(FIN 미전송) 그냥 아무 프레임도 더 보내지
//! 않는 것으로 재현한다 — heartbeat Ping 을 멈추는 것이 곧 "조용히 죽음" 이다.
//!
//! 점유는 **surface 단위**(`stream.open{target}`)라 자기 workspace 의 surface 만
//! 쓰면 공유 인스턴스 위에서 다른 테스트와 격리된다. TTL 만료를 기다리느라 오래
//! 걸리는 테스트이므로 인스턴스를 새로 띄우지 않는 편이 특히 이득이다.
//!
//! **이 테스트가 실제로 실행하는 서버 경로**: `common::shared()`
//! 는 `CARGO_BIN_EXE_tasty` 를 `--no-default-features`/`--headless` 없이 그대로
//! 실행한다 — 즉 `cargo test` 의 기본 feature(`default = ["gui"]`) 로 빌드된
//! **GUI 이벤트 루프 경로**(`src/app/event_handler.rs` 의 `StreamInbound::Disconnected`
//! → `release_attach_for_disconnected` → `release_all_for_client`)만 실제로
//! 구동·검증한다. `src/boot.rs::run_headless`(`--no-default-features` 전용,
//! disconnect → `release_all_for_client` 호출부는 `src/boot.rs:435-436`)는 이
//! 테스트로 실행되지 않는다 — 이 프로젝트에 `cargo test` 로 headless 런타임을
//! 실제 기동하는 경로 자체가 없기 때문(`--no-default-features` 는
//! `crossplatform-check.yml` 에서 `cargo check` 로만 쓰인다).
//!
//! headless 경로가 GUI 경로와 동일하게 동작한다는 근거는 **코드 리딩에 의한
//! 정적 확인**이다 — 실제 lock 해제를 수행하는 `OccupancyRegistry::
//! release_all_for_client`(`src/core/attach.rs`)와 disconnect 를 감지하는
//! `tcp_ipc_server.rs`/`stream_hub.rs` 는 `#[cfg(feature = "gui")]` 분기 없이
//! GUI/headless 양쪽 빌드에 동일하게 컴파일되는 공유 코드이고, `boot.rs` 의
//! headless 분기는 그 공유 함수를 그대로 호출할 뿐 별도 로직을 갖지 않는다.
//! 이 확인은 이 test 가 실행 시점에 보장하는 것이 아니라 리뷰 시점의 코드
//! 검토 결과다 — 회귀가 나면 이 test 는 GUI 경로만 잡아낸다.

mod attach_common;
mod common;

use std::time::{Duration, Instant};

use attach_common::open_surface_attach;
use common::TastyInstance;
use serde_json::json;

// crates/tasty-ipc/src/stream.rs 의 HEARTBEAT_TIMEOUT 과 동일 — 상수를 여기서
// 재선언하는 대신 값을 하드코딩하면 프로토콜 변경 시 조용히 어긋날 수 있으니,
// 여유를 넉넉히 둔 폴링 상한(아래 RELEASE_POLL_TIMEOUT)으로 실제 상수 변경에도
// 견고하게 만든다.
const HEARTBEAT_TIMEOUT_HINT: Duration = Duration::from_secs(20);
const RELEASE_POLL_TIMEOUT: Duration = Duration::from_secs(45);

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
    let server = common::shared();
    let sid = server.create_workspace("silent-disconnect").surface_id;

    assert!(!is_attached(server, sid), "surface must start unattached");

    // client A: attach 성공 → 점유 lock 획득.
    let (stale_conn, ctrl) = open_surface_attach(server.port(), sid);
    assert_eq!(
        ctrl["event"].as_str(),
        Some("attached"),
        "attach should succeed: {ctrl:?}"
    );
    assert!(
        is_attached(server, sid),
        "surface must show attached after a successful attach"
    );

    // TTL 만료 전: 새 client 는 AlreadyAttached 로 거부돼야 한다.
    let (_rejected_conn, reject_ctrl) = open_surface_attach(server.port(), sid);
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
    // `release_all_for_client` 체인만으로 동작 — ADR-0052 핵심 가정 검증).
    // 이 test 는 GUI 이벤트 루프 경로만 구동한다 — 상단 모듈 doc comment 참조.
    let deadline = Instant::now() + RELEASE_POLL_TIMEOUT;
    let mut released = false;
    while Instant::now() < deadline {
        if !is_attached(server, sid) {
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
    let (_reattached_conn, reattach_ctrl) = open_surface_attach(server.port(), sid);
    assert_eq!(
        reattach_ctrl["event"].as_str(),
        Some("attached"),
        "re-attach after TTL release should succeed: {reattach_ctrl:?}"
    );
    assert!(is_attached(server, sid));

    drop(stale_conn); // 정리 — 이미 서버측에서 release 됐으므로 이제 닫아도 무해.
}
