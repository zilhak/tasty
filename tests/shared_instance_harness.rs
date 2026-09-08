//! `tests/common/mod.rs` 의 공유 인스턴스 하네스 자체 검증.
//!
//! 여기 있는 `#[test]` 들은 전부 [`common::shared()`] 를 쓴다 — 이 바이너리 전체가
//! tasty 프로세스 **하나** 위에서 돌아야 하고, 각 테스트는 자기 workspace 안에서만
//! 논다는 것을 확인한다. 전역 목록(`pty.list`)이 남의 항목을 섞어 돌려줘도
//! "내 것이 있는가"(`any`) 형태의 assert 는 병렬/직렬 어느 쪽에서도 통과해야 한다.

mod common;

use common::spawn_diag;

use std::sync::Mutex;
use std::time::Duration;

use serde_json::json;

/// 자식 모드 표지 — 이 값이 있으면 아래 재실행 시험은 아무것도 안 하고 돌아간다.
/// 없으면 자식이 자기를 또 띄워 무한히 내려간다.
const BLOCKED_BOOT_CHILD: &str = "TASTY_TEST_BLOCKED_BOOT_CHILD";

/// 래치의 **효과**를 잰다 — 순서가 아니라 결과다.
///
/// `tests/common/mod.rs` 의 `get_or_init` 클로저는 첫 줄에서 래치를 걸고 그 다음에
/// 프로세스를 띄운다. 그 배치가 *있는지*는 정적으로 판정된다
/// (`crates/tasty-doc-guards/tests/spawn_latch_precedes_the_spawn.rs`). 안 잡히던 것은
/// **그 배치가 실제로 증폭을 막는가** 였다. 막는 장면은 부팅이 실패했을 때만 나오는데
/// 성공하는 환경에서는 그 조건이 안 생기고, 그래서 이 축은 오래 사람이 슬롯에서
/// 벽시계를 재는 것뿐이었다.
///
/// **조건을 만든다.** 하네스가 띄울 바이너리를 실재하지 않는 경로로 덮으면
/// [`spawn_diag::instance_bin`] 이 그 자리에서 죽는다 — 프로세스는 하나도 안 뜨고
/// 상한을 기다리지도 않는다. 그러면 첫 호출은 래치를 걸고 spawn 경로에서 죽고,
/// 나머지 호출은 래치에 막혀 spawn 경로에 **닿지도 못한다.**
///
/// 조건을 **자식 프로세스**에서 만드는 이유는 공유 인스턴스가 프로세스 전역이기
/// 때문이다. 이 바이너리 안에서 조건을 만들면 같은 바이너리의 다른 시험이 전부 그
/// 실패에 휘말린다 — 조건과 나머지 스위트는 한 프로세스에 공존할 수 없다.
///
/// ★ 이 시험이 **못 덮는 것**: `tests/gui_common/mod.rs` 쪽 하네스. 같은 기전을 쓰지만
/// 그 하네스는 `tests/gui_tests.rs` 에 살고 그 바이너리는 어떤 자동 채널도 안 돈다 —
/// 거기에 무엇을 넣어도 아무도 안 돌린다. 밖에서 그 바이너리를 재실행하려면 경로를
/// `target/<프로필>/deps/` 에서 주워야 하는데, 그 조합에서 그 바이너리는 **빌드되지도
/// 않아** 시험이 건너뛰기로 끝난다. 건너뛰는 잡은 0 건 발견과 구별되지 않으므로
/// 그것은 채널이 아니다.
#[test]
fn a_blocked_boot_costs_one_spawn_attempt_no_matter_how_many_tests_ask() {
    if std::env::var_os(BLOCKED_BOOT_CHILD).is_some() {
        return;
    }
    // 프로세스 **안**에서 유일해야 하고, 그 유일성을 시계에 지우지 않는다. 시계는
    // 프로세스 간 축만 갈라 주고(그 축은 `process::id()` 가 이미 진다) 같은 프로세스가
    // 연달아 부르는 경우는 **해상도**에 맡기는데, 그 해상도는 플랫폼이 정한다 —
    // 한 OS 에서 초록인 것이 다른 OS 에서 깨지고 그때 실패는 "경로가 이미 있다" 로
    // 나와 원인을 안 가리킨다. 단조 카운터는 해상도가 없다.
    //
    // 이 자리는 시계가 의도된 선택도 아니다. 이 경로는 **없어야 한다**는 것이 전부라
    // 회차마다 달라야 할 이유가 없다.
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let missing = std::env::temp_dir().join(format!(
        "tasty-no-such-binary-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    assert!(
        !missing.exists(),
        "조건을 만들 경로가 실재한다 — 그러면 이 시험은 부팅이 막힌 상황을 안 만든다: {}",
        missing.display()
    );

    let exe = std::env::current_exe().expect("이 시험 바이너리의 경로를 못 얻었다");
    let out = std::process::Command::new(&exe)
        .args(["--test-threads=1", "--nocapture"])
        .env(BLOCKED_BOOT_CHILD, "1")
        .env(spawn_diag::INSTANCE_BIN_ENV, &missing)
        .output()
        .expect("자기 바이너리를 자식으로 못 띄웠다");
    let log = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    // 첫 팔 — 조건이 실제로 만들어졌나. 안 만들어졌으면 아래 두 수는 우연히 맞을 수 있다.
    let attempts = log.matches("가리키는 경로에 실행 파일이 없다").count();
    assert_eq!(
        attempts, 1,
        "부팅이 막힌 조건에서 spawn 경로에 닿은 횟수가 {attempts} 다(기대 1).\n\
         0 이면 조건이 안 만들어진 것이라 이 시험은 아무것도 안 쟀고, 2 이상이면 \
         래치가 두 번째를 못 막은 것이다 — 그 둘은 같은 수로 안 나온다.\n\
         자식 출력:\n{log}"
    );

    // 둘째 팔 — 나머지는 래치에 막혔나. "spawn 시도 1" 만으로는 나머지가 **왜** 안 떴는지
    // 모른다(전부 건너뛰었어도 1 이다). 막힌 것들이 래치의 문장을 달고 나와야 한다.
    let latched = log.matches("공유 인스턴스 spawn 이 이미 실패했다").count();
    assert!(
        latched >= 2,
        "래치에 막힌 호출이 {latched} 건이다(하한 2).\n\
         이 바이너리에는 `shared()` 를 부르는 시험이 여럿 있으므로, 첫 실패 뒤의 \
         호출들은 전부 래치의 문장을 달고 나와야 한다. 0 이면 래치가 안 걸렸거나 \
         나머지 시험이 `shared()` 를 안 부른 것이다.\n\
         자식 출력:\n{log}"
    );
    assert!(
        !out.status.success(),
        "부팅이 막혔는데 자식이 초록으로 끝났다 — 조건이 안 만들어졌다는 뜻이다.\n\
         자식 출력:\n{log}"
    );
}

/// 서로 다른 `#[test]` 가 관측한 `(ipc port, workspace id)`. 실행 순서와 무관하게
/// 교차 검증하려고 관측값을 누적한다.
static OBSERVED: Mutex<Vec<(u16, u64)>> = Mutex::new(Vec::new());

/// 공유 인스턴스에서 workspace 를 하나 잡고, 앞서 실행된 테스트들의 관측값과 대조한다.
fn claim_workspace(name: &str) -> u64 {
    let tasty = common::shared();
    assert_eq!(
        common::shared_spawn_count(),
        1,
        "shared() 는 test binary 당 정확히 한 번만 프로세스를 띄워야 한다"
    );

    let ws = tasty.create_workspace(name);
    assert_eq!(
        tasty.first_surface_id_in_workspace(ws.id),
        ws.surface_id,
        "workspace.create 가 돌려준 surface_id 와 surface.list 의 workspace 소속이 어긋남"
    );

    // workspace.list 에 방금 만든 id 가 보인다 (전역 목록이지만 `any` 로 조회).
    let listed = tasty.call("workspace.list", json!({}));
    assert!(
        listed
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w["id"].as_u64() == Some(ws.id)),
        "workspace.list 가 방금 만든 workspace 를 빠뜨림: {listed:?}"
    );

    // 공유 인스턴스에서 만든 workspace 는 결코 `workspace.list[0]` 이 아니다 —
    // 부팅 시 만들어진 첫 workspace 뒤에 붙기 때문이다. "첫 workspace 를 집는"
    // 습관이 남아 있으면 남의 격리 단위를 밟게 되므로 그 전제를 못 박아 둔다.
    assert_ne!(
        listed.as_array().unwrap()[0]["id"].as_u64(),
        Some(ws.id),
        "격리 헬퍼가 만든 workspace 가 목록의 [0] 이면 안 된다: {listed:?}"
    );

    // ★ **가드를 쥔 채로 단정하지 않는다.** 이 `static` 은 이 바이너리의 모든 `#[test]` 가
    // 공유하는데, 잠금을 든 채 아래 단정이 터지면 되감기 중에 가드가 떨어지며 뮤텍스가
    // **오염된다.** 그러면 이후 테스트는 자기 실패가 아니라 `PoisonError` 로 죽고,
    // 진짜 실패 하나가 테스트 수만큼의 실패로 불어난다(같은 형태를 `tests/gui_common`
    // 에서 실측했다 — 실패 1 건이 31 건으로 나왔다).
    //
    // 오염을 견디는 것(`unwrap_or_else(into_inner)`)이 아니라 **만들지 않는 쪽**을 쓴다:
    // 값을 꺼내고 기록한 뒤 가드를 이 블록 끝에서 떨어뜨리고, 단정은 그 밖에서 한다.
    // 잠금이 사는 구간에는 패닉할 수 있는 코드가 없다.
    let previously_observed = {
        let mut observed = OBSERVED.lock().unwrap();
        let snapshot = observed.clone();
        observed.push((tasty.port(), ws.id));
        snapshot
    };
    for (port, prev_ws) in previously_observed {
        assert_eq!(
            port,
            tasty.port(),
            "shared() 호출마다 다른 port 가 보인다 — 인스턴스가 재사용되지 않음"
        );
        assert_ne!(
            prev_ws, ws.id,
            "격리 헬퍼가 다른 테스트와 같은 workspace 를 돌려줬다"
        );
    }

    ws.id
}

#[test]
fn shared_instance_is_reused_a() {
    claim_workspace("harness-a");
}

#[test]
fn shared_instance_is_reused_b() {
    claim_workspace("harness-b");
}

/// workspace 로 격리되지 않는 전역 상태(headless PTY) 검증 — 아래 두 테스트가 같은
/// 인스턴스에 각자 PTY 를 띄운다. 목록 assert 가 `any` 이므로 남의 PTY 가 섞여도
/// 병렬/`--test-threads=1` 양쪽에서 통과해야 한다.
fn spawn_and_find_own_pty() {
    let tasty = common::shared();
    let pty_id = tasty.call("pty.spawn", json!({}))["pty_id"]
        .as_u64()
        .expect("pty.spawn returns pty_id");

    let listed = tasty.call("pty.list", json!({}));
    assert!(
        listed["ptys"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["id"].as_u64() == Some(pty_id)),
        "pty.list 가 방금 spawn 한 pty 를 빠뜨림: {listed:?}"
    );

    // 전역 목록 오염을 최소화하기 위해 자기 PTY 는 자기가 회수한다.
    tasty.call("pty.kill", json!({ "id": pty_id }));
}

#[test]
fn global_pty_list_tolerates_other_tests_a() {
    spawn_and_find_own_pty();
}

#[test]
fn global_pty_list_tolerates_other_tests_b() {
    spawn_and_find_own_pty();
}

/// 격리 workspace 의 surface 로 실제 shell 왕복이 되는지 — 공유 인스턴스에서도
/// per-test surface 가 정상적인 PTY 를 갖는다는 확인.
#[test]
fn per_test_workspace_surface_runs_a_shell() {
    let tasty = common::shared();
    let ws = tasty.create_workspace("harness-shell");
    tasty.wait_for_shell(ws.surface_id);

    tasty.set_mark(ws.surface_id);
    tasty.send_text(ws.surface_id, "echo HARNESS_SHARED_MARK\n");
    tasty.wait_for_output(
        ws.surface_id,
        "HARNESS_SHARED_MARK",
        Duration::from_secs(10),
    );
}
