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
//! 는 하네스가 고른 바이너리(`spawn_diag::instance_bin`)를 그대로
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

// ───── 실패한 attach 는 점유를 남기지 않는다 (ADR-0116) ─────

/// 점유가 풀릴 때까지 폴링한다. 시한 내에 풀리면 걸린 시간, 아니면 `None`.
fn wait_until_free(server: &TastyInstance, surface_id: u64, within: Duration) -> Option<Duration> {
    let t0 = Instant::now();
    while t0.elapsed() < within {
        if !is_attached(server, surface_id) {
            return Some(t0.elapsed());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    None
}

/// **(a) 프로토콜 불일치.** 서버가 핸드셰이크 params 의 `proto` 를 검증하지 않던 시절엔
/// 버전이 안 맞는 client 도 점유부터 잡았다. 그 client 는 그 점유를 쓸 수 없는데,
/// 소켓을 닫지 않는 구버전/hung peer 면 heartbeat TTL(약 20 초)이 만료될 때까지 서버가
/// 그 workspace 를 붙잡아 **정상 attach 가 `already_attached` 로 거절**됐다.
///
/// 이 테스트는 그 peer 를 **연결한 채로 살려 둔다** — 소켓을 닫으면 EOF 해제 경로가
/// 대신 동작해서 검증하려는 성질(애초에 점유를 잡지 않는다)이 가려진다.
#[test]
fn proto_mismatch_never_takes_occupancy_and_leaves_the_workspace_attachable() {
    let server = common::shared();
    let ws = server.create_workspace("proto-mismatch-no-occupancy");
    assert!(!is_attached(server, ws.surface_id), "시작은 비점유");

    // 서버가 말하지 못하는 proto 로 attach 를 시도하고, 연결은 닫지 않는다.
    let _hung_peer = attach_common::raw_open_workspace_proto(server.port(), ws.id, 999);

    // 점유가 잡히지 않아야 한다. (수정 전에는 ~200ms 안에 잡혔다.)
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        assert!(
            !is_attached(server, ws.surface_id),
            "proto 가 맞지 않는 client 는 점유를 잡으면 안 된다 — 그 점유를 쓸 수 없다"
        );
        std::thread::sleep(Duration::from_millis(100));
    }

    // 그 peer 가 아직 붙어 있는 동안에도 정상 attach 가 **즉시** 성공해야 한다.
    // (수정 전에는 여기서 `attach_error:already_attached` 가 나왔다.)
    let outcome = attach_common::try_open_workspace_attach(server.port(), ws.id);
    assert_eq!(
        outcome, "attached_workspace",
        "실패한 attach 가 정상 attach 를 막으면 안 된다"
    );
    // 점유 자체는 여기서 단정하지 않는다 — `try_open_workspace_attach` 는 연결을
    // 돌려주지 않아 소켓이 곧바로 drop 되고, EOF 해제가 경합한다. 이 테스트가 묻는
    // 것은 "그 시점에 attach 가 성립하는가" 이고 위 결과가 그 답이다.
}

/// **(c) 연결 즉시 끊김.** 핸드셰이크만 보내고 디스크립터를 읽지 않은 채 소켓을 닫는다
/// — 점유는 잡히지만 EOF 해제 경로(`Disconnected` → `release_all_for_client`)가 곧바로
/// 회수해야 하고, 같은 workspace 에 정상 attach 가 이어서 성공해야 한다.
/// TTL(약 20 초)을 기다리지 않는다는 것이 이 테스트의 요지다.
#[test]
fn closing_before_the_descriptor_releases_occupancy_promptly() {
    let server = common::shared();
    let ws = server.create_workspace("close-before-descriptor");

    drop(attach_common::raw_open_workspace_no_read(
        server.port(),
        ws.id,
    ));

    let freed = wait_until_free(server, ws.surface_id, Duration::from_secs(10));
    let freed = freed.expect("EOF 이후 점유가 회수되지 않았다");
    assert!(
        freed < HEARTBEAT_TIMEOUT_HINT,
        "EOF 는 즉시 감지돼야 한다 — TTL({HEARTBEAT_TIMEOUT_HINT:?}) 을 기다리면 회귀다 (실측 {freed:?})"
    );

    let outcome = attach_common::try_open_workspace_attach(server.port(), ws.id);
    assert_eq!(
        outcome, "attached_workspace",
        "재attach 가 곧바로 성공해야 한다"
    );
}

/// **(b) 인증/토큰.** attach 스트림 채널에는 인증이 없다 — 신뢰 경계가 SSH + loopback
/// 이고 `session_token` 은 핸드셰이크에서 무시된다(`tcp_ipc_server.rs`
/// `handle_stream_connection` doc). 따라서 "토큰 실패로 attach 가 거절되는" 경로 자체가
/// 없고, 거기서 점유가 새는 일도 없다. 이 테스트는 그 부재를 값으로 고정한다 —
/// 엉뚱한 토큰을 실어도 attach 는 정상 성립하고(=토큰이 판정에 쓰이지 않는다),
/// 끊으면 점유가 회수된다.
#[test]
fn the_stream_channel_ignores_session_token_so_auth_cannot_strand_occupancy() {
    let server = common::shared();
    let ws = server.create_workspace("stream-ignores-token");

    let outcome =
        attach_common::try_open_workspace_attach_with_token(server.port(), ws.id, "bogus-token");
    assert_eq!(
        outcome, "attached_workspace",
        "스트림 채널은 session_token 을 보지 않는다 — 토큰 기반 거절 경로가 없다"
    );
}

/// self-attach(자기 IPC 포트를 대상으로 한 GUI mirror attach)는 **점유를 잡지 않고**
/// 디스패치 단계에서 거절된다. 이 attach 는 GUI 메인 스레드가 자기 자신의 핸드셰이크
/// 응답을 기다리며 교착돼 성립할 수 없는데, 실패하는 동안 대상 workspace 점유만
/// 잡았다(관찰 사례: `expected attach Control frame, got Ping` + `attach: workspace N
/// -> client M`). 상세: `docs/adr/0116-attach-handshake-validated-before-occupancy.md`.
#[test]
fn self_attach_is_rejected_before_it_can_take_occupancy() {
    let server = common::shared();
    let ws = server.create_workspace("self-attach-rejected");

    let queued = server.call(
        "attach.into_gui",
        json!({ "port": server.port(), "workspace": ws.id }),
    );
    assert_eq!(queued["queued"], true, "IPC 는 큐잉까지만 한다: {queued:?}");

    // 주 관측량: **메인 루프가 막히지 않는다.** 게이트가 없으면 디스패치가
    // `attach_handshake` 를 메인 스레드에서 동기 실행하고, 그 응답을 만들 주체도 같은
    // 메인 스레드라 heartbeat Ping/read timeout 이 걸릴 때까지 루프가 멈춘다 — 그 동안
    // IPC 왕복도 함께 멈춘다. 점유 유무만 폴링하면 그 구간이 짧을 때 놓치므로, 교착
    // 자체를 신호로 쓴다. (점유가 잡히는지도 함께 본다.)
    let deadline = Instant::now() + Duration::from_secs(6);
    while Instant::now() < deadline {
        let t = Instant::now();
        let alive = server.call("ui.state", json!({}));
        let rtt = t.elapsed();
        assert!(
            alive.get("active_workspace").is_some(),
            "ui.state 응답 형태: {alive:?}"
        );
        assert!(
            rtt < Duration::from_secs(2),
            "self-attach 가 거절되지 않으면 메인 루프가 자기 응답을 기다리며 막힌다 \
             (IPC 왕복 {rtt:?})"
        );
        assert!(
            !is_attached(server, ws.surface_id),
            "self-attach 는 점유를 잡기 전에 거절돼야 한다"
        );
        std::thread::sleep(Duration::from_millis(100));
    }

    // 그 workspace 는 여전히 정상 attach 가능해야 한다.
    let outcome = attach_common::try_open_workspace_attach(server.port(), ws.id);
    assert_eq!(outcome, "attached_workspace");
}
