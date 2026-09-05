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
//! **이 테스트가 실제로 실행하는 서버 경로는 빌드 조합이 정한다.** `common::shared()`
//! 가 띄우는 것은 `spawn_diag::instance_bin`, 곧 `CARGO_BIN_EXE_tasty` — **이 테스트를
//! 빌드한 그 feature 로 빌드된 자기 바이너리**다. 그래서 기본 조합에서는 GUI 이벤트
//! 루프(`src/app/event_handler.rs::apply_stream_outcome`)가, `--no-default-features`
//! 에서는 headless 부팅 진입점(`src/boot.rs::run_headless` → `boot/headless_stream.rs`)이
//! 실제로 뜬다. 두 경로 모두 실행으로 검증된다.
//!
//! 한때 여기에 "이 프로젝트엔 `cargo test` 로 headless 런타임을 기동하는 경로가 없고
//! `--no-default-features` 는 `cargo check` 로만 쓰인다" 고 적혀 있었다. **둘 다 사실이
//! 아니다** — `crossplatform-check.yml` 의 `check-headless` 잡은 `cargo check (headless)`
//! 와 별도로 `cargo test --workspace --no-default-features --locked --no-fail-fast` 스텝을
//! 돌리고, 그 스텝이 이 파일도 헤드리스로 세워 돌린다. 실측(2026-09-05): 이 타깃을
//! `--no-default-features` 로 빌드해 30 회 실행(그중 5 회는 인위 부하 아래) — 28 회
//! `13 passed`, 2 회 실패. 실패한 2 회는 **이 결함과 무관한 다른 테스트**
//! (`self_attach_is_rejected_before_it_can_take_occupancy` — IPC 왕복 시간에 벽시계
//! 상한을 건 단언이라 부하에 흔들린다. 실측 왕복 10.6s vs 상한 2s)였고, 그중 한 회는
//! 실패 목록을 저장하지 못해 그 회의 전체 목록은 확인하지 못했다. 낡은 서술을 믿으면
//! 헤드리스 쪽 회귀를 "이 테스트가 못 보는 것" 으로 오분류하게 된다.
//!
//! **끊김 처리의 순서 계약**은 두 경로가 각자 갖는다(gui `apply_stream_outcome`,
//! headless `boot/headless_stream.rs::apply`). 그 순서가 어긋나면 같은 배치의 재attach 가
//! 이미 죽은 holder 에게 `already_attached` 로 막히는데, 그 결함은 이 파일의 어떤
//! 테스트로도 안 보인다(실측: 결함을 되살린 뮤테이션에서 이 타깃 3 회 전부 `13 passed`).
//! 판정은 `src/core/attach.rs` 의 합성 회귀와 배선 가드
//! (`both_pumps_mark_disconnects_before_applying_attach_requests`)가 갖는다 —
//! 근거는 `docs/adr/0157-a-disconnected-holder-does-not-block-a-reattach.md`.

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
/// 점유가 풀릴 때까지 폴링한 결과 — **폴 횟수를 함께 돌려준다.**
struct FreeWait {
    elapsed: Duration,
    polls: usize,
}

/// 폴 주기. 예산을 이 값으로 나눈 것이 "굶지 않았다면 이만큼 봤어야 한다" 는 기대치다.
const FREE_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// **대기 예산은 단정하는 문턱보다 커야 한다.** 작으면 문턱을 넘는 값이 이 함수에서
/// 나올 수 없어 그 단정이 죽는다 — 종전 형태가 그랬다: 예산 10 s 로 재고 20 s 문턱을
/// 단정했으니, TTL 을 기다린 경우는 `None` 으로 빠져 문턱 비교에 도달조차 못 한다.
/// 그런데 train68 에서 **21.65 s** 가 실측됐다. 그 값은 TTL 을 기다려서 나온 것이
/// 아니라(예산이 10 s 다) **한 번의 `is_attached` 왕복이 11 s 넘게 굶은 것**인데,
/// 실패 문구는 TTL 을 지목했다 — 그 값이 배제하는 원인을 단언한 것이다.
///
/// 그래서 예산을 문턱 위로 올리고, 가르는 값(폴 횟수)을 함께 낸다.
fn wait_until_free(server: &TastyInstance, surface_id: u64, within: Duration) -> Option<FreeWait> {
    let t0 = Instant::now();
    let mut polls = 0usize;
    while t0.elapsed() < within {
        polls += 1;
        if !is_attached(server, surface_id) {
            return Some(FreeWait {
                elapsed: t0.elapsed(),
                polls,
            });
        }
        std::thread::sleep(FREE_POLL_INTERVAL);
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

    // 예산은 문턱보다 크다 — 그래야 "TTL 을 기다렸다" 가 이 단정에 **도달할 수 있는**
    // 결과가 된다. 작게 두면 그 경우는 `None` 으로 빠져 문턱 비교가 죽는다.
    let budget = HEARTBEAT_TIMEOUT_HINT + Duration::from_secs(5);
    let freed = wait_until_free(server, ws.surface_id, budget);
    let freed = freed.expect("EOF 이후 점유가 회수되지 않았다");
    let expected_polls = budget.as_millis() / FREE_POLL_INTERVAL.as_millis();
    assert!(
        freed.elapsed < HEARTBEAT_TIMEOUT_HINT,
        "EOF 는 즉시 감지돼야 한다 — TTL({HEARTBEAT_TIMEOUT_HINT:?}) 을 기다리면 회귀다 \
         (경과 {:?} · 폴 {}회, 예산이면 최대 {expected_polls}회). \
         ★ 폴 수가 경과에 비해 훨씬 적으면 TTL 을 기다린 것이 아니라 **폴 루프가 굶은 것**이고, \
         그때 이 문구가 지목하는 회귀는 원인이 아니다.",
        freed.elapsed,
        freed.polls
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

/// **양방향의 반대편.** 위 테스트들은 *침묵하면* 점유가 회수되는 것을 본다. 이것은
/// **heartbeat 를 보내는 client 는 TTL 을 넘겨도 살아 있는 것**을 본다.
///
/// 이 절이 없으면 `attach_common::open_workspace_attach` 에서 heartbeat 가 사라져도
/// 전부 초록이다 — 그 침묵은 **단독 실행에서 안 보인다.** 교환이 20 초 안에 끝나기
/// 때문이고, 부하가 붙어 20 초를 넘기는 순간에만 `UnexpectedEof` 로 터진다. 실측
/// (2026-09-06, 같은 바이너리·8-way 동시 실행): heartbeat 없으면 8/8 실패·EOF 56 건
/// (스위트 21.0~21.6 s), 있으면 8/8 통과(23.4~43.4 s — 벽을 두 배 넘겨도 산다).
///
/// 상한은 제품 상수를 그대로 읽는다 — 값이 바뀌면 이 테스트가 따라간다.
#[test]
fn a_heartbeating_client_outlives_the_silence_ttl() {
    let server = common::shared();
    let ws = server.create_workspace("heartbeat-outlives-ttl");
    let mut stream = attach_common::open_workspace_attach(server.port(), ws.id);

    // TTL 을 확실히 넘긴다. 서버는 이 동안 client 가 조용하면 죽은 peer 로 보고 끊는다.
    std::thread::sleep(tasty_ipc::stream::HEARTBEAT_TIMEOUT + Duration::from_secs(3));

    // 살아 있으면 서버의 idle Ping 이 계속 와서 읽기가 성공한다. 끊겼으면
    // `UnexpectedEof` 이고, 그건 상한 초과(`WouldBlock`)와 다른 사건이다.
    let read = attach_common::read_frame_result(&mut stream);
    assert!(
        read.is_ok(),
        "heartbeat 를 보내는 client 가 TTL 을 못 넘겼다 — 서버가 끊었다: {:?}",
        read.err()
    );

    // 점유도 그대로 유지된다 — 살아 있다는 것의 제품 측 관측이다.
    let outcome = attach_common::try_open_workspace_attach(server.port(), ws.id);
    assert_eq!(
        outcome, "attach_error:already_attached",
        "연결이 살아 있으면 그 workspace 는 여전히 점유 중이어야 한다"
    );
}
