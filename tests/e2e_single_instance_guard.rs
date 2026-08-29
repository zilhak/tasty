//! e2e 단일 인스턴스 원칙 가드 — 테스트가 tasty GUI 프로세스를 필요 이상으로 띄우면 fail 한다.
//!
//! 원칙(ADR-0090, `docs/dev-guide/e2e-tests.md` §1): **격리 단위는 프로세스가 아니라
//! workspace 다.** tasty 인스턴스는 test binary 당 1 개를 `common::shared()` 로 공유하고,
//! 테스트별 격리는 `create_workspace()` 로 확보한다. GUI 창은 뜰 때마다 OS 포커스를
//! 훔치고 기동 비용도 크므로, 인스턴스 수는 그 자체로 관리 대상이다.
//!
//! 이 가드는 두 축을 본다 — 어느 한 축만으로는 회귀를 다 못 잡는다.
//!
//! | 축 | 잡는 회귀 | 상수 |
//! |---|---|---|
//! | 파일당 전용 spawn 호출 수 | `#[test]` 마다 인스턴스를 띄우는 형태 | [`ALLOWLIST_FILES`] (미등록 = 0 회) |
//! | 인스턴스를 띄우는 test 파일 개수 | 새 binary 가 늘어 총량이 다시 증가하는 형태 | [`EXPECTED_INSTANCE_TESTS`] |
//!
//! **동적 가드(실행 중 tasty PID 개수 세기)는 일부러 채택하지 않았다** — 가드가 공유
//! 인스턴스 위에서 돌면 자기 프로세스를 포함해 세고, cargo 는 test 타겟을 순차 실행하므로
//! "binary 당 1 개" 는 설계상 허용값이라 PID 수만으로 위반과 정상을 가릴 수 없으며,
//! 개발자 머신의 실사용 tasty 가 그대로 오탐이 된다. 동적 관찰은 수동 검증 절차로 남긴다.
//!
//! 선례: `tests/no_emoji_in_source.rs`(`ALLOWLIST_FILES` 정적 스캔),
//! `tests/cli_naming_count_drift.rs`(카운트 스냅샷 고정).

use std::collections::BTreeSet;
use std::path::Path;

/// 문서 경로 — 위반 메시지에 실어 다음 작업자가 원칙을 찾아갈 수 있게 한다.
const DOC: &str = "docs/dev-guide/e2e-tests.md (원칙 근거: docs/adr/0090-test-isolation-by-workspace-not-process.md)";

/// 이 가드 자신 — 아래 마커 문자열을 상수로 담고 있어 스캔하면 자기 자신을 잡는다.
const SELF_FILE: &str = "e2e_single_instance_guard.rs";

/// **전용** 인스턴스를 띄우는 호출의 마커. 공유 진입점(`common::shared()`)은 여기 없다 —
/// 그것이 기본 경로이고 몇 번 부르든 프로세스는 하나이기 때문이다.
const DEDICATED_SPAWN_MARKERS: &[&str] = &[
    // tests/common 하네스의 전용 인스턴스 생성자 두 개.
    "TastyInstance::spawn(",
    "TastyInstance::spawn_with_inherit_cwd(",
    // tests/webhook_common 하네스. 같은 `CARGO_BIN_EXE_tasty` 를 띄우므로 창 비용이 같다.
    // 이쪽은 공유 진입점이 없어 builder 호출 하나가 곧 인스턴스 하나다.
    "WebhookInstance::builder(",
];

/// 인스턴스를 띄우는 파일인지 판정하는 마커 — 전용/공유를 가리지 않는다.
/// **호출부가 아니라 하네스 모듈 선언을 본다**: 진입점은 `use gui_common::shared;` 처럼
/// import 해 쓸 수 있어 `gui_common::shared(` 같은 호출부 문자열이 아예 안 나타날 수 있는
/// 반면, 모듈 선언은 tasty 프로세스를 띄우는 하네스를 끌어다 쓴다는 사실 그 자체다.
/// (`attach_common` 은 프로토콜 frame 헬퍼일 뿐 프로세스를 띄우지 않아 여기 없다.)
const INSTANCE_HARNESS_MARKERS: &[&str] =
    &["mod common;", "mod gui_common;", "mod webhook_common;"];

/// 전용 인스턴스가 정당한 파일과 그 상한. **미등록 파일은 0 회** — 즉 새 test binary 는
/// `common::shared()` 를 쓰고 workspace 로 격리해야 한다. 등록은 "프로세스 경계 자체가
/// 검증 대상" 일 때만 정당하다.
///
/// - `tests/e2e_tests.rs` (1): 33 개 시나리오를 `#[test]` 하나에 몰아넣어 인스턴스를 1 개만
///   쓴다 — 공유 하네스가 생기기 전부터 이 원칙을 지켜 온 원본이다.
/// - `tests/soak_memory.rs` (1): 프로세스 트리 RSS 를 **외부에서** 측정한다(`pid()`).
///   다른 테스트의 활동이 섞이면 측정 자체가 무의미해진다. 전수 `#[ignore]`.
/// - `tests/attach_convert_cwd_loopback.rs` (1): 검증 대상 동작이 `inherit_cwd` 설정에
///   게이트되는데, 그 값은 격리 HOME 의 `config.toml` 에 미리 쓰는 *기동 시점* 설정이라
///   이미 떠 있는 인스턴스에 런타임으로 바꿔 끼울 수 없다. 같은 파일의 나머지 테스트는
///   공유 인스턴스를 쓴다.
/// - `tests/hook_env_integration.rs` (1): 웹훅 하네스는 `TASTY_HOME`/`webhooks.toml` 을
///   인스턴스별로 시딩해야 해 공유 진입점이 없다.
/// - `tests/webhook_integration.rs` (2): 위와 같은 이유 + **재시작 시나리오**라 같은 HOME 을
///   물려받는 두 번째 인스턴스가 검증 대상 그 자체다(영속성 확인).
const ALLOWLIST_FILES: &[(&str, usize)] = &[
    ("tests/e2e_tests.rs", 1),
    ("tests/soak_memory.rs", 1),
    ("tests/attach_convert_cwd_loopback.rs", 1),
    ("tests/hook_env_integration.rs", 1),
    ("tests/webhook_integration.rs", 2),
];

/// tasty 프로세스를 띄우는 test 파일의 **전체 목록**. 파일당 spawn 수를 아무리 조여도
/// binary 가 늘면 총량은 다시 증가하므로(각 binary 가 1 개씩만 띄워도 마찬가지) 목록 자체를
/// 고정한다. 새 e2e binary 가 정말 필요하면 여기 추가하면서 그 필요를 한 번 되짚게 된다.
const EXPECTED_INSTANCE_TESTS: &[&str] = &[
    "tests/attach_convert_cwd_loopback.rs",
    "tests/attach_git_query_loopback.rs",
    "tests/attach_list_dir_loopback.rs",
    "tests/attach_local_creation_tap.rs",
    "tests/attach_silent_disconnect.rs",
    "tests/e2e_tests.rs",
    "tests/gui_tests.rs",
    "tests/hook_env_integration.rs",
    "tests/hooks_detection_e2e.rs",
    "tests/shared_instance_harness.rs",
    "tests/soak_memory.rs",
    "tests/webhook_integration.rs",
];

/// `tests/*.rs` (깊이 1) 만 모은다 — test binary 의 진입 파일이다. 하네스 모듈은
/// `tests/<name>/mod.rs` 에 있어 자연히 제외되며, 거기 있는 `pub fn spawn` 은 호출이
/// 아니라 정의라 세면 안 된다.
fn test_entry_files(tests_dir: &Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let entries = std::fs::read_dir(tests_dir).expect("tests/ 디렉토리 read 실패");
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_string();
        if name == SELF_FILE {
            continue;
        }
        let contents = std::fs::read_to_string(&path).expect("test 파일 read 실패");
        out.push((format!("tests/{name}"), contents));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn count_markers(contents: &str, markers: &[&str]) -> usize {
    markers.iter().map(|m| contents.matches(m).count()).sum()
}

#[test]
fn dedicated_instance_spawns_stay_within_allowlist() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut violations = Vec::new();

    for (rel, contents) in test_entry_files(&root.join("tests")) {
        let found = count_markers(&contents, DEDICATED_SPAWN_MARKERS);
        let allowed = ALLOWLIST_FILES
            .iter()
            .find(|(f, _)| *f == rel)
            .map(|(_, n)| *n)
            .unwrap_or(0);
        if found > allowed {
            violations.push(format!(
                "  {rel} — 전용 인스턴스 spawn {found} 회 (허용 {allowed} 회)"
            ));
        }
    }

    assert!(
        violations.is_empty(),
        "테스트가 tasty 인스턴스를 필요 이상으로 띄운다. GUI 창은 뜰 때마다 OS 포커스를 \
         훔치므로 인스턴스는 test binary 당 1 개가 기본이다 — `common::shared()` 를 쓰고 \
         격리는 `create_workspace()` 의 **workspace 단위**로 하라. 프로세스 경계 자체가 \
         검증 대상이라 전용 인스턴스가 꼭 필요하면 ALLOWLIST_FILES 에 이유와 함께 등록할 것. \
         원칙: {DOC}\n{}",
        violations.join("\n")
    );
}

#[test]
fn instance_spawning_test_files_match_snapshot() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let actual: BTreeSet<String> = test_entry_files(&root.join("tests"))
        .into_iter()
        .filter(|(_, contents)| count_markers(contents, INSTANCE_HARNESS_MARKERS) > 0)
        .map(|(rel, _)| rel)
        .collect();
    let expected: BTreeSet<String> = EXPECTED_INSTANCE_TESTS
        .iter()
        .map(|s| (*s).to_string())
        .collect();

    let added: Vec<&String> = actual.difference(&expected).collect();
    let removed: Vec<&String> = expected.difference(&actual).collect();

    assert!(
        added.is_empty() && removed.is_empty(),
        "tasty 인스턴스를 띄우는 test 파일 목록이 스냅샷과 다르다. 파일당 spawn 수를 조여도 \
         binary 가 늘면 총량은 다시 증가하므로 목록 자체를 고정한다 — 새 파일이 정말 별도 \
         binary 여야 하는지(기존 e2e 파일에 시나리오를 추가하면 되는 것은 아닌지) 먼저 \
         확인하고, 맞다면 EXPECTED_INSTANCE_TESTS 에 추가하라. 원칙: {DOC}\n\
         \x20 추가됨: {added:?}\n\x20 사라짐: {removed:?}"
    );
}
