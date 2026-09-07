//! 파일 SLOC 게이트가 **측정 실패를 통과로 읽지 않는지** 고정한다.
//!
//! `scripts/check-file-size.sh` 는 tokei 로 재고 파이썬으로 판정한다. 둘 중 하나가 죽어도
//! 게이트가 `exit 0` 을 내던 자리가 있었다 — `mapfile < <(...)` 의 프로세스 치환이 종료코드를
//! 버려서 `set -o pipefail` 이 닿지 않았고, "결과 0 줄" 과 "위반 0 건" 이 구분되지 않았다.
//! 그 상태에서는 러너에서 tokei 가 어긋나는 순간 게이트가 **영원히 초록**이 된다.
//!
//! `docs/adr/0131-file-sloc-gate-needs-a-firing-trigger.md` 가 이 게이트에 발화 트리거를 달았기
//! 때문에 그 결함이 그때부터 활성이다. 채널을 켠 것과 같은 계보에서 닫는다.
//!
//! **판정 방식**: 실제 tokei 도 실제 판정기도 쓰지 않는다. PATH 앞에 스텁 `tokei` 를 놓고
//! `TASTY_STRIP_CFG_TEST_BIN` 에 스텁 판정기를 물려 경우를 주입하고 종료코드만 본다 —
//! 러너에 tokei 가 없어도 돌고, 레포 내용이 바뀌어도 값이 안 흔들린다. 스텁을 쓰는 두 번째
//! 이유가 있다: 진짜 판정기는 cargo 산출물이라, 여기서 그것을 빌드하면 **바깥 `cargo test`
//! 와 빌드 디렉토리 잠금을 두고 서로를 기다린다.**
//! 위반(1) / 측정 실패(2) / 통과(0) 를 **서로 다른 코드**로 요구하므로, 셋 중 둘이 같은 값으로
//! 붕괴하면 여기서 죽는다.
//!
//! **자동 채널**: 통합 테스트라 헤드리스 잡(전체 스위트)이 본다. Windows 잡은 `--lib --bins` 라
//! 보지 않는데, 이 테스트는 `#[cfg(unix)]` 라 애초에 그 조합의 대상이 아니다.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

/// PATH 앞에 놓을 스텁 `tokei` 를 만든다. `body` 는 셸 스크립트 본문(shebang 제외).
fn stub_dir(body: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("임시 디렉토리");
    let path = dir.path().join("tokei");
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("스텁 작성");
    let mut perm = fs::metadata(&path).expect("스텁 metadata").permissions();
    perm.set_mode(0o755);
    fs::set_permissions(&path, perm).expect("실행권한");
    dir
}

/// 스텁 tokei 를 PATH 앞에 두고 게이트를 돌려 종료코드를 얻는다. 판정기는 성공 스텁.
fn run_gate_with(body: &str) -> i32 {
    run_gate(body, "mkdir -p \"$1\"\nexit 0")
}

/// tokei 스텁과 판정기(strip-cfg-test) 스텁을 함께 주입하고 게이트를 돌린다.
fn run_gate(tokei_body: &str, strip_body: &str) -> i32 {
    let dir = stub_dir(tokei_body);
    let strip = dir.path().join("strip-cfg-test");
    fs::write(&strip, format!("#!/bin/sh\n{strip_body}\n")).expect("판정기 스텁 작성");
    let mut perm = fs::metadata(&strip).expect("스텁 metadata").permissions();
    perm.set_mode(0o755);
    fs::set_permissions(&strip, perm).expect("실행권한");

    let root = env!("CARGO_MANIFEST_DIR");
    let old = std::env::var("PATH").unwrap_or_default();
    let path = format!("{}:{}", dir.path().display(), old);
    let out = Command::new("bash")
        .arg(format!("{root}/scripts/check-file-size.sh"))
        .env("PATH", path)
        .env("TASTY_STRIP_CFG_TEST_BIN", &strip)
        .current_dir(root)
        .output()
        .expect("게이트 실행");
    out.status.code().unwrap_or(-1)
}

/// tokei 가 정상 동작해 임계 이하만 보고하는 JSON.
const UNDER_THRESHOLD: &str =
    r#"echo '{"Rust":{"reports":[{"name":"src/zz_stub_small.rs","stats":{"code":10}}]}}'"#;

/// 임계를 넘는 파일 하나 — allowlist 에도 skip 패턴에도 걸리지 않는 경로여야 한다.
const OVER_THRESHOLD: &str =
    r#"echo '{"Rust":{"reports":[{"name":"src/zz_stub_violation.rs","stats":{"code":9999}}]}}'"#;

#[test]
fn real_violations_still_exit_one() {
    assert_eq!(
        run_gate_with(OVER_THRESHOLD),
        1,
        "임계 초과 파일이 있으면 exit 1 이어야 한다 — 이게 게이트의 본래 일이다"
    );
}

#[test]
fn a_clean_tree_exits_zero() {
    // 대조군. 이것이 없으면 아래 두 테스트는 "항상 실패하는 게이트" 로도 통과한다.
    assert_eq!(
        run_gate_with(UNDER_THRESHOLD),
        0,
        "임계 초과가 없으면 exit 0 이어야 한다"
    );
}

#[test]
fn a_failing_tokei_is_not_a_pass() {
    assert_eq!(
        run_gate_with("echo boom >&2\nexit 7"),
        2,
        "tokei 가 죽으면 측정 실패(exit 2)여야 한다 — 통과(0)도 위반(1)도 아니다"
    );
}

#[test]
fn an_empty_report_is_not_a_pass() {
    // 가장 위험한 형태: rc 0 + 유효한 JSON. 고치기 전에는 "게이트 통과" 라고 출력했다.
    assert_eq!(
        run_gate_with(r#"echo '{}'"#),
        2,
        "Rust report 가 비면 측정 실패(exit 2)여야 한다 — 위반 0 건과 구분되지 않으면 안 된다"
    );
    assert_eq!(
        run_gate_with(r#"echo '{"Rust":{"reports":[]}}'"#),
        2,
        "reports 가 빈 배열인 경우도 같다"
    );
}

#[test]
fn malformed_json_is_not_a_pass() {
    assert_eq!(
        run_gate_with("echo 'not json at all'"),
        2,
        "파서가 죽으면 측정 실패(exit 2)여야 한다"
    );
}

/// 판정기가 죽으면 **측정 실패**다. 사본이 안 만들어졌는데 tokei 가 (빈 트리를 보고)
/// 무언가를 돌려주면 게이트는 "위반 0 건" 으로 읽는다 — 그 붕괴를 여기서 막는다.
#[test]
fn a_failing_stripper_is_not_a_pass() {
    assert_eq!(
        run_gate(UNDER_THRESHOLD, "echo boom >&2\nexit 3"),
        2,
        "출하 줄 판정기가 죽으면 측정 실패(exit 2)여야 한다 — 통과(0)도 위반(1)도 아니다"
    );
}

/// 판정기 경로가 아예 없으면 통과가 아니다. 러너에 바이너리를 안 만들어 둔 상태가
/// 조용히 초록이 되면 게이트가 무엇을 쟀는지 아무도 모른다.
/// skip 대상만 보고하는 tokei 스텁 — 걸러내고 나면 **판정 대상이 0 건**이 된다.
const ALL_SKIPPED: &str =
    r#"echo '{"Rust":{"reports":[{"name":"src/tests/zz_stub_skipped.rs","stats":{"code":10}}]}}'"#;

/// 판정 대상이 0 건인 것은 통과가 아니다.
///
/// skip 과 allowlist 가 전부를 삼키면 위반 목록도 비고, 그러면 "아무것도 안 쟀다" 가
/// "다 통과했다" 와 **같은 줄**을 만든다. 이 게이트의 다른 모든 측정 실패는 exit 2 인데
/// 이 갈래만 초록으로 나가면 그 규율이 한 자리에서만 지켜지는 것이다.
#[test]
fn zero_judged_files_is_not_a_pass() {
    assert_eq!(
        run_gate_with(ALL_SKIPPED),
        2,
        "판정 대상이 0 건이면 측정 실패다 — 통과로 읽지 않는다"
    );
}

#[test]
fn a_missing_stripper_is_not_a_pass() {
    let dir = stub_dir(UNDER_THRESHOLD);
    let root = env!("CARGO_MANIFEST_DIR");
    let old = std::env::var("PATH").unwrap_or_default();
    let out = Command::new("bash")
        .arg(format!("{root}/scripts/check-file-size.sh"))
        .env("PATH", format!("{}:{}", dir.path().display(), old))
        .env(
            "TASTY_STRIP_CFG_TEST_BIN",
            dir.path().join("__no_such_binary__"),
        )
        .current_dir(root)
        .output()
        .expect("게이트 실행");
    assert_eq!(out.status.code(), Some(2));
}
