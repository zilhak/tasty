//! common 공유 인스턴스를 사용해 워크스페이스 격리와 인스턴스 재사용을 확인한다.
//! 전역 목록은 다른 시험의 항목이 섞여도 자기 항목을 찾을 수 있어야 한다.

mod common;

use common::spawn_diag;

use std::sync::Mutex;
use std::time::Duration;

use serde_json::json;

/// 재실행한 자식이 다시 자기를 실행하지 않도록 구별한다.
const BLOCKED_BOOT_CHILD: &str = "TASTY_TEST_BLOCKED_BOOT_CHILD";

/// 별도 자식 프로세스에 없는 바이너리 경로를 주어 최초 초기화 실패를 만든다.
/// 이후 호출이 재생성 대신 래치에서 거절되는지 확인한다. common 하네스만 검사하며 GUI 하네스는 범위 밖이다.
#[test]
fn a_blocked_boot_costs_one_spawn_attempt_no_matter_how_many_tests_ask() {
    if std::env::var_os(BLOCKED_BOOT_CHILD).is_some() {
        return;
    }
    // 시계 해상도 대신 PID와 카운터로 없는 경로 이름을 구별한다.
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

    let attempts = log.matches("가리키는 경로에 실행 파일이 없다").count();
    assert_eq!(
        attempts, 1,
        "부팅 차단 상태에서 생성 경로의 실패 진단이 {attempts} 번 나왔다(기대 1). 경로 설정과 래치의 재시도 차단을 확인한다.\n자식 출력:\n{log}"
    );

    // 생성 시도 횟수만으로는 건너뛴 호출과 구별되지 않아 래치 거절 진단도 센다.
    let latched = log.matches("공유 인스턴스 spawn 이 이미 실패했다").count();
    assert!(
        latched >= 2,
        "첫 생성 실패 뒤 래치 거절 진단이 {latched} 건이다(하한 2). 후속 shared 호출과 래치 동작을 확인한다.\n자식 출력:\n{log}"
    );
    assert!(
        !out.status.success(),
        "부팅 차단 시나리오의 자식 실행이 성공했다. 실패 조건이 적용됐는지 확인한다.\n자식 출력:\n{log}"
    );
}

static OBSERVED: Mutex<Vec<(u16, u64)>> = Mutex::new(Vec::new());

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

    let listed = tasty.call("workspace.list", json!({}));
    assert!(
        listed
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w["id"].as_u64() == Some(ws.id)),
        "workspace.list 가 방금 만든 workspace 를 빠뜨림: {listed:?}"
    );

    // 부팅 시 만든 워크스페이스를 잘못 재사용하지 않는지 확인한다.
    assert_ne!(
        listed.as_array().unwrap()[0]["id"].as_u64(),
        Some(ws.id),
        "격리 헬퍼가 만든 workspace 가 목록의 [0] 이면 안 된다: {listed:?}"
    );

    // 단정 실패가 뮤텍스를 poison 상태로 만들지 않도록 관측값을 기록하고 잠금을 해제한 뒤 비교한다.
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

/// 전역 PTY 목록은 각 시험이 만든 ID로 확인해 다른 항목이 섞여도 동작하게 한다.
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

#[path = "common/startup_tests.rs"]
mod startup_tests;
