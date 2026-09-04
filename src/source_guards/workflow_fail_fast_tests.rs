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
    assert!(
        files >= MIN_WORKFLOW_FILES,
        "스캔 하한 미달: 워크플로 파일 {files} 개(하한 {MIN_WORKFLOW_FILES}) — 경로가 틀렸다"
    );
    assert!(
        invocations >= MIN_TEST_INVOCATIONS,
        "스캔 하한 미달: `cargo test` 호출 {invocations} 개(하한 {MIN_TEST_INVOCATIONS})"
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
