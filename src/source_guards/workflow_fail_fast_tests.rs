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
    // 파일 목록과 호출 수는 별도 비교하고 여기서는 빈 실행 입력을 막는다.
    assert!(
        files > 0 && invocations > 0,
        "워크플로 {files}개·cargo test 호출 {invocations}개를 읽었다. 둘 다 1개 이상이어야 한다."
    );
    assert!(
        violations.is_empty(),
        "시험 실행 명령에 --no-fail-fast가 없다. 앞 바이너리가 실패해도 나머지 타깃을 실행하도록 지정한다. 컴파일만 할 명령이면 --no-run을 사용한다:\n  {}",
        violations.join("\n  ")
    );
}

#[test]
fn the_no_run_exemption_covers_only_compile_only_invocations() {
    let compile_only =
        "      - name: Build\n        run: cargo test --workspace --no-run --locked\n";
    assert!(test_invocations_missing_no_fail_fast(compile_only).is_empty());

    let runs = "      - name: Run\n        run: cargo test --workspace --locked\n";
    assert_eq!(test_invocations_missing_no_fail_fast(runs).len(), 1);

    // 같은 스텝의 앞 --no-run이 뒤 실행 명령을 면제하지 않아야 한다.
    let both = "        run: |\n          cargo test --workspace --no-run --locked\n          cargo test --workspace --locked\n";
    assert_eq!(test_invocations_missing_no_fail_fast(both).len(), 1);
}

#[test]
fn a_flag_on_a_continuation_line_or_folded_scalar_still_counts() {
    let cont = "        run: |\n          cargo test --workspace --locked \\\n            --no-fail-fast\n";
    assert!(test_invocations_missing_no_fail_fast(cont).is_empty());
    let folded = "        run: >\n          cargo test --locked\n          --no-fail-fast\n";
    assert!(test_invocations_missing_no_fail_fast(folded).is_empty());
}

#[test]
fn a_step_name_that_quotes_the_command_is_not_an_invocation() {
    // 스텝 이름의 cargo test를 실행 명령으로 세지 않아야 한다.
    let yaml = "      - name: cargo test (unit)\n        run: cargo test --workspace --locked\n";
    assert_eq!(test_invocations_missing_no_fail_fast(yaml).len(), 1);
    let named_ok = "      - name: cargo test (unit)\n        run: cargo test --workspace --locked --no-fail-fast\n";
    assert!(test_invocations_missing_no_fail_fast(named_ok).is_empty());
}

#[test]
fn a_comment_mentioning_the_flag_does_not_exempt_a_step() {
    // 주석의 플래그 언급이 실제 명령을 통과시키지 않아야 한다.
    let yaml = "      # `--no-fail-fast` 는 필수다\n      - name: unit\n        run: cargo test --workspace --locked\n";
    assert_eq!(test_invocations_missing_no_fail_fast(yaml).len(), 1);
}

/// Git의 추적·미추적 목록을 디렉터리 순회와 별도로 수집해 비교한다. Git 실패는 검사를 중단한다.
fn git_listed_workflows() -> BTreeSet<String> {
    let root = repo_root();
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["ls-files", "-co", "--exclude-standard", "--", WORKFLOW_DIR])
        .output()
        .unwrap_or_else(|e| {
            panic!("git ls-files 실행 실패: {e}. 워크플로 수집과 대조할 Git 목록이 필요하다.")
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
        "Git 워크플로 목록이 {}개로 하한 {MIN_GIT_LISTED_WORKFLOWS} 미만이다. 비교 범위를 확인한다.",
        listed.len()
    );
    listed
}

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

#[test]
fn the_scanned_workflow_set_matches_what_git_lists() {
    let scanned: BTreeSet<String> = scanned_workflows().into_keys().collect();
    let listed = git_listed_workflows();
    let missing: Vec<&String> = listed.difference(&scanned).collect();
    let extra: Vec<&String> = scanned.difference(&listed).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "워크플로 수집과 Git 목록이 다르다.\n  수집 누락: {missing:?}\n  수집에만 있음: {extra:?}\n누락한 파일의 시험 명령은 검사되지 않으므로 경로와 순회를 확인한다."
    );
}

/// 한 파일의 삭제와 다른 파일의 추가가 상쇄되지 않도록 파일별 호출 수를 대조한다.
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
        "워크플로별 cargo test 호출 수가 달라졌다. 실제 명령 변경을 확인하고 EXPECTED_TEST_INVOCATIONS를 갱신한다:\n{}",
        drift.join("\n")
    );
}
