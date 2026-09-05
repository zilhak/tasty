//! 동결 총합 래칫(`scripts/check-frozen-sum-ratchet.sh`)이 **양방향으로** 서는지 고정한다.
//!
//! 이 게이트가 보는 것은 `.complexity-file-allowlist` 에 오른 파일들의 출하 SLOC **합**
//! 하나다. 자매 게이트 `check-file-size.sh` 는 파일이 임계를 넘는 *순간* 만 보므로, 일단
//! 목록에 오른 파일이 자라는 것은 아무도 안 본다 — 실측으로 도입 시 동결 18 중 15 가
//! 자라 +2406 줄이었다. 근거는 `docs/adr/0168-the-file-sloc-threshold-is-not-derived-and-the-freeze-ratchets-one-way.md`.
//!
//! **판정 방식**: 진짜 레포를 보지 않는다. 임시 루트에 스크립트 둘과 합성 allowlist 를
//! 깔고, PATH 앞의 스텁 `tokei` 로 합을 주입한다. 그래서 레포 내용이 바뀌어도 여기 값이
//! 안 흔들리고, 예산 줄이 없는 경우처럼 **추적 파일을 훼손해야만 만들 수 있는 경우**도
//! 잴 수 있다. 진짜 판정기를 안 쓰는 이유는 자매 테스트와 같다 — cargo 산출물이라
//! 여기서 빌드하면 바깥 `cargo test` 와 빌드 디렉토리 잠금을 두고 서로를 기다린다.
//!
//! 세 종류를 **서로 다른 종료코드**로 요구한다: 위반(1) / 측정 실패(2) / 통과(0).
//! 셋 중 둘이 같은 값으로 붕괴하면 여기서 죽는다.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

const SLACK: i64 = 1000;
const BUDGET: i64 = 5000;

fn write_exec(path: &Path, body: &str) {
    fs::write(path, body).expect("스크립트 작성");
    let mut perm = fs::metadata(path).expect("metadata").permissions();
    perm.set_mode(0o755);
    fs::set_permissions(path, perm).expect("실행권한");
}

/// 임시 루트를 짓는다. `budget_line` 이 없으면 예산 줄을 빼고 쓴다.
fn root_with(budget_line: Option<i64>, entries: &[&str]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("임시 디렉토리");
    let root = dir.path();
    fs::create_dir_all(root.join("scripts")).expect("scripts");
    let here = env!("CARGO_MANIFEST_DIR");
    fs::copy(
        format!("{here}/scripts/check-frozen-sum-ratchet.sh"),
        root.join("scripts/check-frozen-sum-ratchet.sh"),
    )
    .expect("게이트 복사");
    // THRESHOLD 만 있으면 된다 — 게이트는 이 파일에서 여유를 읽어 온다.
    fs::write(
        root.join("scripts/check-file-size.sh"),
        format!("#!/usr/bin/env bash\nTHRESHOLD={SLACK}\n"),
    )
    .expect("자매 게이트 스텁");

    let mut al = String::from("# 합성 목록\n");
    if let Some(b) = budget_line {
        al.push_str(&format!("# frozen-sum-budget: {b}\n"));
    }
    for e in entries {
        al.push_str(e);
        al.push('\n');
    }
    fs::write(root.join(".complexity-file-allowlist"), al).expect("allowlist");
    dir
}

/// 스텁 tokei · 스텁 판정기를 물려 게이트를 돌리고 종료코드를 얻는다.
fn run(root: &Path, tokei_body: &str) -> i32 {
    let bin = tempfile::tempdir().expect("스텁 디렉토리");
    write_exec(
        &bin.path().join("tokei"),
        &format!("#!/bin/sh\n{tokei_body}\n"),
    );
    let strip = bin.path().join("strip-cfg-test");
    write_exec(&strip, "#!/bin/sh\nmkdir -p \"$1\"\nexit 0\n");
    let path = format!(
        "{}:{}",
        bin.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );
    Command::new("bash")
        .arg(root.join("scripts/check-frozen-sum-ratchet.sh"))
        .env("PATH", path)
        .env("TASTY_STRIP_CFG_TEST_BIN", &strip)
        .current_dir(root)
        .output()
        .expect("게이트 실행")
        .status
        .code()
        .unwrap_or(-1)
}

/// 목록의 파일 하나에 `code` 를 실어 보고하는 tokei 스텁.
fn reports(path: &str, code: i64) -> String {
    format!(r#"echo '{{"Rust":{{"reports":[{{"name":"{path}","stats":{{"code":{code}}}}}]}}}}'"#)
}

const P: &str = "src/frozen_one.rs";

#[test]
fn a_sum_at_the_budget_passes() {
    // 대조군. 이것이 없으면 아래 것들은 "항상 실패하는 게이트" 로도 통과한다.
    let d = root_with(Some(BUDGET), &[P]);
    assert_eq!(
        run(d.path(), &reports(P, BUDGET)),
        0,
        "예산과 같으면 통과다"
    );
}

#[test]
fn growth_of_exactly_one_file_worth_is_still_inside() {
    // 경계. 여유가 "파일 하나 분량" 이라는 것이 이 게이트의 눈금이므로, 그 지점이
    // 어느 쪽인지가 곧 눈금의 정의다.
    let d = root_with(Some(BUDGET), &[P]);
    assert_eq!(
        run(d.path(), &reports(P, BUDGET + SLACK)),
        0,
        "예산 + 여유 까지는 통과여야 한다"
    );
}

#[test]
fn growing_past_one_file_worth_fails() {
    let d = root_with(Some(BUDGET), &[P]);
    assert_eq!(
        run(d.path(), &reports(P, BUDGET + SLACK + 1)),
        1,
        "동결분이 파일 하나 분량보다 더 자라면 위반이다 — 이게 이 게이트의 본래 일이다"
    );
}

#[test]
fn shrinking_below_the_budget_also_fails() {
    // 한 방향만 서는 래칫은 래칫이 아니다. 줄었는데 예산을 안 내리면 그만큼이
    // 아무도 안 보는 구간으로 남는다.
    let d = root_with(Some(BUDGET), &[P]);
    assert_eq!(
        run(d.path(), &reports(P, BUDGET - 1)),
        1,
        "합이 예산 아래로 내려가면 예산을 내리라고 실패해야 한다"
    );
}

#[test]
fn the_slack_is_read_from_the_file_size_gate() {
    // 여유는 임의의 수가 아니라 파일 임계 자신이다. 그 연결이 끊기면 눈금이 임의값이 된다.
    let d = root_with(Some(BUDGET), &[P]);
    fs::write(
        d.path().join("scripts/check-file-size.sh"),
        "#!/usr/bin/env bash\nTHRESHOLD=200\n",
    )
    .expect("임계 교체");
    assert_eq!(
        run(d.path(), &reports(P, BUDGET + 500)),
        1,
        "임계가 200 이면 +500 은 여유 밖이다 — 여유를 그 게이트에서 읽지 않으면 이게 통과한다"
    );
}

#[test]
fn a_missing_budget_line_is_a_measurement_failure() {
    let d = root_with(None, &[P]);
    assert_eq!(
        run(d.path(), &reports(P, BUDGET)),
        2,
        "예산 줄이 없으면 통과(0)도 위반(1)도 아니고 측정 실패(2)다"
    );
}

#[test]
fn a_missing_threshold_is_a_measurement_failure() {
    let d = root_with(Some(BUDGET), &[P]);
    fs::write(
        d.path().join("scripts/check-file-size.sh"),
        "#!/usr/bin/env bash\n# THRESHOLD 가 없다\n",
    )
    .expect("임계 제거");
    assert_eq!(
        run(d.path(), &reports(P, BUDGET)),
        2,
        "여유를 못 읽으면 측정 실패다"
    );
}

#[test]
fn a_dead_tokei_is_a_measurement_failure() {
    let d = root_with(Some(BUDGET), &[P]);
    assert_eq!(run(d.path(), "exit 3"), 2, "tokei 가 죽으면 측정 실패다");
}

#[test]
fn an_empty_report_is_a_measurement_failure() {
    let d = root_with(Some(BUDGET), &[P]);
    assert_eq!(
        run(d.path(), r#"echo '{"Rust":{"reports":[]}}'"#),
        2,
        "보고가 비면 '위반 0 건' 이 아니라 측정 실패다"
    );
}

#[test]
fn a_listed_file_that_exists_but_is_unreported_is_a_measurement_failure() {
    // 목록에 있고 디스크에도 있는데 보고에 없으면 합이 조용히 줄어 "래칫을 조여라" 가
    // 나온다 — 틀린 지시다. 없어진 파일(디스크에 없음)은 0 으로 세는 것이 맞고, 이 둘을
    // 가르지 않으면 계측기 고장이 리팩터 성과로 읽힌다.
    let d = root_with(Some(BUDGET), &[P, "src/frozen_two.rs"]);
    fs::create_dir_all(d.path().join("src")).expect("src");
    fs::write(d.path().join("src/frozen_two.rs"), "fn a() {}\n").expect("파일");
    assert_eq!(
        run(d.path(), &reports(P, BUDGET)),
        2,
        "보고 누락은 통과도 위반도 아니다"
    );
}

#[test]
fn a_deleted_listed_file_just_counts_as_zero() {
    // 위 테스트의 짝. 삭제는 정당하게 합을 줄인다 — 그것까지 측정 실패로 읽으면
    // 파일을 지울 때마다 게이트가 선다.
    let d = root_with(Some(BUDGET), &[P, "src/frozen_gone.rs"]);
    assert_eq!(
        run(d.path(), &reports(P, BUDGET)),
        0,
        "목록에 있으나 디스크에 없는 경로는 0 으로 세고 통과여야 한다"
    );
}

#[test]
fn files_outside_the_allowlist_do_not_count() {
    // 예산은 **동결분** 의 합이다. 보고 전체를 세면 레포가 자라는 것만으로 발화해
    // 이 게이트가 묻는 물음("동결 안에서 자랐나")이 아니라 다른 물음이 된다.
    let d = root_with(Some(BUDGET), &[P]);
    let body = format!(
        r#"echo '{{"Rust":{{"reports":[{{"name":"{P}","stats":{{"code":{BUDGET}}}}},{{"name":"src/not_frozen.rs","stats":{{"code":900000}}}}]}}}}'"#
    );
    assert_eq!(
        run(d.path(), &body),
        0,
        "목록 밖 파일은 합에 안 들어가야 한다 — 들어가면 예산이 레포 크기를 따라간다"
    );
}
