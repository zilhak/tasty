use std::collections::{BTreeMap, BTreeSet};

use super::*;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn every_workflow_step_that_runs_tests_uses_no_fail_fast() {
    let dir = repo_root().join(WORKFLOW_DIR);
    let entries = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("워크플로 디렉토리를 읽지 못했다: {}: {e}", dir.display()));
    let (mut files, mut invocations, mut violations) = (0usize, 0usize, Vec::new());
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "yml" && e != "yaml") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        files += 1;
        invocations += cargo_test_invocations(&flatten_workflow(&text)).len();
        for inv in test_invocations_missing_no_fail_fast(&text) {
            violations.push(format!("{}: {inv}", path.display()));
        }
    }
    // 모수는 아래 `the_scanned_workflow_set_matches_what_git_lists` 가 집합 동등으로
    // 고정하고, 호출 수는 `the_test_invocation_counts_are_pinned_per_file` 이 파일별로
    // 고정한다. 여기서는 이 판정이 무엇도 안 읽은 채 통과하지 않는다는 것만 본다.
    assert!(
        files > 0 && invocations > 0,
        "워크플로 {files} 개 / `cargo test` 호출 {invocations} 개를 읽었다 — 둘 중 하나가 \
         0 이면 아래 위반 목록이 비는 이유가 '위반이 없어서' 가 아니다"
    );
    assert!(
        violations.is_empty(),
        "테스트를 실행하는 워크플로 스텝에 `--no-fail-fast` 가 없다. 없으면 처음 실패한 \
         테스트 바이너리에서 멈춰 뒤따르는 타깃이 통째로 실행되지 않고, 로그는 그것을 \
         '실패 N 건' 으로만 보고한다. 컴파일만 하는 호출이면 `--no-run` 을 함께 써라.\n  {}",
        violations.join("\n  ")
    );
}

#[test]
fn the_no_run_exemption_covers_only_compile_only_invocations() {
    // 면제를 겨냥한 변이 — 면제 창 안쪽(같은 스텝, 같은 명령 형태)에 진짜 실행 호출을
    // 심으면 잡혀야 한다.
    let compile_only =
        "      - name: Build\n        run: cargo test --workspace --no-run --locked\n";
    assert!(test_invocations_missing_no_fail_fast(compile_only).is_empty());

    let runs = "      - name: Run\n        run: cargo test --workspace --locked\n";
    assert_eq!(test_invocations_missing_no_fail_fast(runs).len(), 1);

    // 같은 스텝에 둘이 붙어 있어도 앞의 `--no-run` 이 뒤를 면제하지 않는다.
    let both = "        run: |\n          cargo test --workspace --no-run --locked\n          cargo test --workspace --locked\n";
    assert_eq!(test_invocations_missing_no_fail_fast(both).len(), 1);
}

#[test]
fn a_flag_on_a_continuation_line_or_folded_scalar_still_counts() {
    // 줄 끝 `\` 이음 — 줄 단위로 보면 여기서 끊겨 있는 플래그를 놓친다.
    let cont = "        run: |\n          cargo test --workspace --locked \\\n            --no-fail-fast\n";
    assert!(test_invocations_missing_no_fail_fast(cont).is_empty());
    // `>` 접힌 스칼라.
    let folded = "        run: >\n          cargo test --locked\n          --no-fail-fast\n";
    assert!(test_invocations_missing_no_fail_fast(folded).is_empty());
}

#[test]
fn a_step_name_that_quotes_the_command_is_not_an_invocation() {
    // 이 레포의 스텝 이름은 `cargo test (unit)` 처럼 명령을 그대로 쓴다. 이름을
    // 호출로 세면 오탐이고, 이름 슬라이스가 다음 `cargo ` 앞까지라 **뒤따르는 진짜
    // 명령의 플래그를 대신 물어 그 명령을 검사에서 빼 버린다** — 오탐보다 이쪽이 나쁘다.
    let yaml = "      - name: cargo test (unit)\n        run: cargo test --workspace --locked\n";
    assert_eq!(test_invocations_missing_no_fail_fast(yaml).len(), 1);
    let named_ok = "      - name: cargo test (unit)\n        run: cargo test --workspace --locked --no-fail-fast\n";
    assert!(test_invocations_missing_no_fail_fast(named_ok).is_empty());
}

#[test]
fn a_comment_mentioning_the_flag_does_not_exempt_a_step() {
    // 주석이 면제해 주면 가드가 스스로 무력해진다 — 이 레포의 워크플로 주석에는
    // 실제로 이 플래그 이름이 나온다.
    let yaml = "      # `--no-fail-fast` 는 필수다\n      - name: unit\n        run: cargo test --workspace --locked\n";
    assert_eq!(test_invocations_missing_no_fail_fast(yaml).len(), 1);
}

/// git 이 아는 워크플로 파일 이름 — 스캔 모수의 **독립 대조군**.
///
/// 스냅샷 상수가 아니라 git 을 쓰는 이유는 `scan_population.rs` 가 적어 둔 것과 같다:
/// git 의 목록은 이 파일의 `read_dir` 순회와 완전히 다른 시스템이 만든 것이라 같은
/// 버그를 공유하지 않고, 워크플로가 늘거나 줄면 저절로 따라온다. 갱신 절차가 필요 없는
/// 것이 이 선택이 사는 이유다.
///
/// 추적본과 무시되지 않은 미추적본을 함께 본다 — 새 워크플로를 `git add` 하기 전에도
/// 양쪽 집합에 동시에 들어와야 작업 중에 헛되이 빨개지지 않는다.
///
/// git 이 없으면 **크게 죽는다.** "git 이 없어서 판정 안 함" 은 이 가드가 막으려는
/// 0 회 실행 그 자체이고, 0 회 실행은 0 건 발견과 구별되지 않는다.
fn git_listed_workflows() -> BTreeSet<String> {
    let root = repo_root();
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["ls-files", "-co", "--exclude-standard", "--", WORKFLOW_DIR])
        .output()
        .unwrap_or_else(|e| {
            panic!(
                "`git ls-files` 를 실행할 수 없다 — {e}. 이 가드는 git 의 목록을 워크플로 \
                 모수의 대조군으로 쓴다. 여기서 조용히 넘어가면 '0 회 실행' 이 \
                 '0 건 발견' 으로 보이므로 죽는다"
            )
        });
    assert!(
        output.status.success(),
        "`git ls-files` 가 실패했다(rc {:?}): {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr).trim()
    );
    let listed: BTreeSet<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| line.ends_with(".yml") || line.ends_with(".yaml"))
        .filter_map(|line| line.rsplit('/').next().map(str::to_owned))
        .filter(|name| root.join(WORKFLOW_DIR).join(name).is_file())
        .collect();
    assert!(
        listed.len() >= MIN_GIT_LISTED_WORKFLOWS,
        "git 이 낸 워크플로가 {} 개뿐이다(하한 {MIN_GIT_LISTED_WORKFLOWS}). 대조군이 비면 \
         집합 동등은 언제나 초록이므로, 대조군 자신에도 연기 검사를 둔다",
        listed.len()
    );
    listed
}

/// `read_dir` 로 걷은 워크플로 이름과 그 파일별 `cargo test` 호출 수.
fn scanned_workflows() -> BTreeMap<String, usize> {
    let dir = repo_root().join(WORKFLOW_DIR);
    let entries = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("워크플로 디렉토리를 읽지 못했다: {}: {e}", dir.display()));
    let mut out = BTreeMap::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "yml" && e != "yaml") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        out.insert(name, cargo_test_invocations(&flatten_workflow(&text)).len());
    }
    out
}

/// 걷은 워크플로 집합이 git 의 목록과 **정확히** 같다.
///
/// 이 자리가 하한이었다(4). 실측 9 였으므로 절반 넘게 사라져도 초록이었고, 더 나쁘게는
/// 어느 파일이 빠졌는지 말하지 못했다. 하한을 8 로 올리는 것은 같은 결함을 옮기는 것뿐이라
/// 대조군을 세웠다 — 여기는 독립 오라클이 있는 자리다.
#[test]
fn the_scanned_workflow_set_matches_what_git_lists() {
    let scanned: BTreeSet<String> = scanned_workflows().into_keys().collect();
    let listed = git_listed_workflows();
    let missing: Vec<&String> = listed.difference(&scanned).collect();
    let extra: Vec<&String> = scanned.difference(&listed).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "워크플로 스캔 모수가 git 의 목록과 다르다.\n\
         \x20 git 은 아는데 스캔이 못 봄: {missing:?}\n\
         \x20 스캔에는 있는데 git 이 모름: {extra:?}\n\
         전자는 그 워크플로가 `--no-fail-fast` 판정을 아예 안 받는다는 뜻이다."
    );
}

/// 파일별 `cargo test` 호출 수가 고정값과 같다.
///
/// 총계가 아니라 파일별인 이유는 [`EXPECTED_TEST_INVOCATIONS`] 에 적혀 있다 — 총계는
/// 호출이 파일 사이를 옮겨 다니는 것을 못 본다.
#[test]
fn the_test_invocation_counts_are_pinned_per_file() {
    let scanned = scanned_workflows();
    let expected: BTreeMap<&str, usize> = EXPECTED_TEST_INVOCATIONS.iter().copied().collect();

    let mut drift = Vec::new();
    for (name, actual) in &scanned {
        let want = expected.get(name.as_str()).copied().unwrap_or(0);
        if *actual != want {
            drift.push(format!("  {name}: 기대 {want} / 실제 {actual}"));
        }
    }
    for name in expected.keys() {
        if !scanned.contains_key(*name) {
            drift.push(format!("  {name}: 목록에 있으나 그런 워크플로가 없다"));
        }
    }
    assert!(
        drift.is_empty(),
        "워크플로의 `cargo test` 호출 수가 고정값과 다르다. 워크플로를 고쳤으면 \
         `EXPECTED_TEST_INVOCATIONS` 를 함께 갱신하라 — 그 갱신이 이 가드가 요구하는 \
         '어느 자리에 호출이 생겼는가' 의 검토다.\n{}",
        drift.join("\n")
    );
}
