//! `tests/common/mod.rs` 의 공유 인스턴스 하네스 자체 검증.
//!
//! 여기 있는 `#[test]` 들은 전부 [`common::shared()`] 를 쓴다 — 이 바이너리 전체가
//! tasty 프로세스 **하나** 위에서 돌아야 하고, 각 테스트는 자기 workspace 안에서만
//! 논다는 것을 확인한다. 전역 목록(`pty.list`)이 남의 항목을 섞어 돌려줘도
//! "내 것이 있는가"(`any`) 형태의 assert 는 병렬/직렬 어느 쪽에서도 통과해야 한다.

mod common;

use std::sync::Mutex;
use std::time::Duration;

use serde_json::json;

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
