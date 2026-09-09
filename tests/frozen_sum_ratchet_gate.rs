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

mod gate_env;

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

/// 자매 게이트(`check-file-size.sh`) 스텁을 쓴다.
///
/// 이 게이트는 자기 값을 두 개 **외우지 않고 저기서 읽는다** — 여유는 `THRESHOLD`,
/// 좌변은 `SCAN_DIRS`. 그래서 스텁이 무엇을 담는지가 곧 시험이 재는 조건이다.
/// `scan_dirs` 가 `None` 이면 그 줄을 아예 빼서 **읽기 실패 갈래**를 만든다.
fn write_sibling(root: &Path, threshold: Option<i64>, scan_dirs: Option<&str>) {
    let mut body = String::from("#!/usr/bin/env bash\n");
    if let Some(t) = threshold {
        body.push_str(&format!("THRESHOLD={t}\n"));
    }
    if let Some(d) = scan_dirs {
        body.push_str(&format!("SCAN_DIRS=({d})\n"));
    }
    fs::write(root.join("scripts/check-file-size.sh"), body).expect("자매 게이트 스텁");
}

/// 임시 루트를 짓는다. `budget_line` 이 없으면 예산 줄을 빼고 쓴다.
fn root_with(budget_line: Option<i64>, entries: &[&str]) -> tempfile::TempDir {
    root_with_bias(budget_line, Some(0), entries)
}

/// 편향 핀까지 고르는 판. 이 게이트의 좌변은 **둘**이라 둘 다 고를 수 있어야 한다.
fn root_with_bias(
    budget_line: Option<i64>,
    bias_line: Option<i64>,
    entries: &[&str],
) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("임시 디렉토리");
    let root = dir.path();
    fs::create_dir_all(root.join("scripts")).expect("scripts");
    let here = env!("CARGO_MANIFEST_DIR");
    fs::copy(
        format!("{here}/scripts/check-frozen-sum-ratchet.sh"),
        root.join("scripts/check-frozen-sum-ratchet.sh"),
    )
    .expect("게이트 복사");
    // 게이트가 판정기 찾기를 공용 `resolve_judge` 에 위임하므로 그 파일도 합성 root 에
    // 있어야 한다. 없으면 `set -e` 아래에서 source 가 죽어 **판정 불가(2)가 아니라
    // 위반(1)** 이 나온다 — 게이트가 무엇을 재는지와 무관한 값이라 원인이 안 보인다.
    fs::create_dir_all(root.join("scripts/lib")).expect("scripts/lib");
    fs::copy(
        format!("{here}/scripts/lib/judge-bin.sh"),
        root.join("scripts/lib/judge-bin.sh"),
    )
    .expect("판정기 찾기 공용 복사");
    // 게이트는 이 파일에서 **둘 다** 읽어 온다: 여유(THRESHOLD)와 좌변(SCAN_DIRS).
    // 둘 중 하나라도 없으면 판정 불가(2)라, 여기 빠뜨리면 아래 시험들이 재려던 것이
    // 아니라 **읽기 실패**를 재게 된다(실측으로 밟았다 — 좌변 읽기를 넣은 회차에 이
    // 스텁이 안 따라가 7 건이 한꺼번에 판정 불가로 떨어졌다).
    write_sibling(root, Some(SLACK), Some("src crates"));

    let mut al = String::from("# 합성 목록\n");
    if let Some(b) = budget_line {
        al.push_str(&format!("# frozen-sum-budget: {b}\n"));
    }
    if let Some(b) = bias_line {
        al.push_str(&format!("# doc-comment-bias: {b}\n"));
    }
    for e in entries {
        al.push_str(e);
        al.push('\n');
    }
    fs::write(root.join(".complexity-file-allowlist"), al).expect("allowlist");
    dir
}

/// 게이트가 판정기에 주는 플래그를 스텁도 걷어낸다.
///
/// 실물은 위치와 무관하게 걷어내지만(`args.retain`), 스텁이 이걸 빼먹으면 `$1` 이
/// out-dir 이 아니게 되고 그 어긋남은 위반이 아니라 **판정 불가**로 나온다 — 그러면
/// 시험이 자기가 재려던 갈래가 아니라 대조 실패를 재게 된다. 실측(2026-09-09): SLOC
/// 게이트가 `--neutralize-char-literal-quotes` 를 주기 시작하자 이 파일에서 1 건,
/// 자매 파일에서 3 건이 한꺼번에 그렇게 떨어졌다.
///
/// **스텁은 실물보다 관대하다 — 일부러 그렇다.** 실물은 아는 플래그 둘만 `retain` 으로
/// 빼고 모르는 `--*` 는 인자로 남겨 usage 실패(rc=2)로 떨어지는데, 스텁은 모든 `--*` 를
/// 먹는다. 그래서 **게이트가 판정기에 주는 플래그와 판정기가 아는 플래그의 정합은 여기서
/// 안 재진다** — 이 시험이 재려는 것은 게이트의 갈래 논리이지 그 정합이 아니고, 스텁을
/// 좁히면 플래그가 하나 늘 때마다 이 파일이 함께 죽는다. 그 정합의 채널은 **게이트 자신의
/// 실행**이다: 실측(2026-09-09) 플래그 철자를 틀린 트리에서 게이트가 rc=2(판정 불가)로
/// 떨어졌다. 판정 불가는 통과가 아니므로 그 회귀는 조용하지 않다.
const EAT_FLAGS: &str = "while [ \"${1#--}\" != \"$1\" ]; do shift; done\n";

/// 스텁 tokei · 스텁 판정기를 물려 게이트를 돌리고 종료코드를 얻는다.
fn run(root: &Path, tokei_body: &str) -> i32 {
    let bin = tempfile::tempdir().expect("스텁 디렉토리");
    write_exec(
        &bin.path().join("tokei"),
        &format!("#!/bin/sh\n{tokei_body}\n"),
    );
    let strip = bin.path().join("strip-cfg-test");
    write_exec(
        &strip,
        &format!("#!/bin/sh\n{EAT_FLAGS}mkdir -p \"$1\"\nexit 0\n"),
    );
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
///
/// 게이트가 한 번의 tokei 호출로 **셋**을 재므로 스텁도 셋을 담는다: 사본(합) ·
/// doc 줄을 뺀 사본(편향) · 형태 프로브. 편향은 기본으로 0(둘이 같은 code)이고
/// 프로브는 실물 tokei 가 내는 값(1)이라, 이 기본판을 쓰는 시험들은 **합 축만** 잰다.
fn reports(path: &str, code: i64) -> String {
    reports_with(path, code, code, 1)
}

/// 셋을 따로 고르는 판 — 편향 축·프로브 축을 재는 시험이 쓴다.
fn reports_with(path: &str, code: i64, nodoc_code: i64, probe_code: i64) -> String {
    format!(
        r#"echo '{{"Rust":{{"reports":[{{"name":"{path}","stats":{{"code":{code}}}}},{{"name":"__nodoc/{path}","stats":{{"code":{nodoc_code}}}}},{{"name":"__probe/probe.rs","stats":{{"code":{probe_code}}}}}]}}}}'"#
    )
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
    // 좌변은 그대로 두고 임계만 바꾼다 — 둘을 같이 지우면 이 시험이 여유가 아니라
    // **읽기 실패**를 재게 된다.
    write_sibling(d.path(), Some(200), Some("src crates"));
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

/// 자매에 `THRESHOLD` 가 없으면 **판정 불가(2)** 다.
///
/// 좌변(`SCAN_DIRS`)은 **남긴다.** 한때 둘을 같이 지웠고, 그러면 이 시험이 재는 것은
/// 임계 읽기가 아니라 그 **다음** 호출 지점(좌변 읽기)이 된다 — 실측 2026-09-08:
/// 임계 읽기의 거절을 임계 기본값 폴백으로 바꾸는 완화 변이에서 이 시험이 그대로
/// 통과했다. 형제 시험 `the_slack_is_read_from_the_file_size_gate` 의 주석이 같은
/// 함정을 반대 방향으로 경고하고 있었는데 이 시험이 그 함정에 있었다.
#[test]
fn a_missing_threshold_is_a_measurement_failure() {
    let d = root_with(Some(BUDGET), &[P]);
    write_sibling(d.path(), None, Some("src crates"));
    assert_eq!(
        run(d.path(), &reports(P, BUDGET)),
        2,
        "여유를 못 읽으면 측정 실패다 — 기본값으로 물러나면 눈금이 임의값이 된다"
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
        r#"echo '{{"Rust":{{"reports":[{{"name":"{P}","stats":{{"code":{BUDGET}}}}},{{"name":"__nodoc/{P}","stats":{{"code":{BUDGET}}}}},{{"name":"__probe/probe.rs","stats":{{"code":1}}}},{{"name":"src/not_frozen.rs","stats":{{"code":900000}}}}]}}}}'"#
    );
    assert_eq!(
        run(d.path(), &body),
        0,
        "목록 밖 파일은 합에 안 들어가야 한다 — 들어가면 예산이 레포 크기를 따라간다"
    );
}

// ── 좌변도 자매에게서 읽어 온다 ──────────────────────────────────────────
//
// 여유(`THRESHOLD`)만이 아니라 **무엇을 훑는가**(`SCAN_DIRS`)도 `check-file-size.sh`
// 에서 읽는다. 그 전에는 같은 pathspec 이 이 파일에 둘, 저 파일에 둘 — 두 파일에 걸쳐
// **네 벌**이었다. 넷이 갈리면 두 게이트가 서로 다른 모수를 재는데, 이 게이트의 합은
// 정의상 저 게이트가 판정하는 집합의 부분집합(allowlist 항목)이라 맞물림이 깨진다.
// 그리고 그 어긋남은 조용하다 — 여유가 파일 하나 몫이라 웬만한 차이를 삼킨다.

/// 좌변에 `extra` 가 들어왔을 때만 항목을 하나 더 내는 스텁 tokei 본문.
///
/// 디스크에 그 파일을 만들지 **않는다** — 만들면 "목록에 있는데 보고에 없다" 검사가
/// 먼저 걸려 이 시험이 좌변이 아니라 그 검사를 재게 된다.
fn reports_sensitive_to_scan_dirs(base: i64, extra: i64) -> String {
    format!(
        "case \" $* \" in\n\
         *\" extra \"*) printf '{{\"Rust\":{{\"reports\":[{{\"name\":\"{P}\",\"stats\":{{\"code\":{base}}}}},\
         {{\"name\":\"__nodoc/{P}\",\"stats\":{{\"code\":{base}}}}},\
         {{\"name\":\"__probe/probe.rs\",\"stats\":{{\"code\":1}}}},\
         {{\"name\":\"extra/e.rs\",\"stats\":{{\"code\":{extra}}}}},\
         {{\"name\":\"__nodoc/extra/e.rs\",\"stats\":{{\"code\":{extra}}}}}]}}}}' ;;\n\
         *) printf '{{\"Rust\":{{\"reports\":[{{\"name\":\"{P}\",\"stats\":{{\"code\":{base}}}}},\
         {{\"name\":\"__nodoc/{P}\",\"stats\":{{\"code\":{base}}}}},\
         {{\"name\":\"__probe/probe.rs\",\"stats\":{{\"code\":1}}}}]}}}}' ;;\n\
         esac"
    )
}

/// 자매의 `SCAN_DIRS` 를 늘리면 이 게이트가 그 영토까지 잰다.
///
/// 예산을 **늘어난 합**에 맞춰 둔다. 좌변이 따라오면 합이 예산과 같아 통과(0)이고,
/// 안 따라오면 합이 예산 아래로 내려가 "래칫을 조여라"(1) 로 빨개진다. 그래서 이
/// 시험은 rc 하나로 좌변이 따라왔는지를 가른다.
#[test]
fn the_scan_dirs_are_read_from_the_file_size_gate() {
    let d = root_with(Some(BUDGET + 100), &[P, "extra/e.rs"]);
    write_sibling(d.path(), Some(SLACK), Some("src crates extra"));
    assert_eq!(
        run(d.path(), &reports_sensitive_to_scan_dirs(BUDGET, 100)),
        0,
        "자매의 좌변을 늘렸는데 이 게이트가 안 따라왔다 — 두 게이트가 서로 다른 모수를 \
         재고, 그 어긋남은 여유가 삼켜 조용하다"
    );
}

/// 자매에 `SCAN_DIRS` 줄이 없으면 **판정 불가(2)** 다. 기본값으로 물러나지 않는다.
///
/// 물러나면 자매가 넓어진 날 이쪽만 좁은 채로 조용히 돈다 — 그 상태의 합은 "동결분이
/// 그만큼이다" 가 아니라 "그만큼만 봤다" 이고, 둘이 같은 줄로 나간다.
#[test]
fn a_sibling_without_scan_dirs_is_undecidable() {
    let d = root_with(Some(BUDGET), &[P]);
    write_sibling(d.path(), Some(SLACK), None);
    assert_eq!(
        run(d.path(), &reports(P, BUDGET)),
        2,
        "자매에 좌변이 없는데 값을 냈다 — 무엇을 훑을지 모르는 채로 잰 합이다"
    );
}

/// ★ R1056 축 — 좌변이 **판정기**에 닿는지는 위 시험이 못 잰다.
///
/// 실측: 판정기 좌변만 옛 문자열로 되돌리는 변이에서 위 열넷이 **전부 통과했다.**
/// 이 파일의 다른 시험들이 쓰는 스텁 조합 때문이다 — 판정기 스텁은 사본을 안 만들고
/// (`mkdir -p` 뿐), tokei 스텁은 고정 JSON 을 찍는다. 그러면 판정기가 어느 디렉토리를
/// 받았는지가 값 어디에도 안 나타난다.
///
/// R1056 의 갈래는 둘이다: 그 자리가 판정에 안 쓰이거나(지운다), **다른 채널로
/// 쓰이거나**(축을 세운다). 여기는 뒤쪽이다 — 진짜 게이트에서 사본에 없는 디렉토리를
/// tokei 가 열면 죽는다. 그래서 그 채널을 재현하는 스텁 조합을 이 시험에만 쓴다:
/// 판정기는 인자대로 **실제로 복사**하고, tokei 는 사본 디스크를 읽으며 없는
/// 디렉토리에 진짜처럼 비영으로 죽는다.
#[test]
fn the_scan_dirs_reach_the_stripper() {
    let d = root_with(Some(BUDGET + 100), &[P, "extra/e.rs"]);
    write_sibling(d.path(), Some(SLACK), Some("src crates extra"));
    // 판정기가 옮길 실물. 위 시험과 달리 디스크에 만든다 — 판정기가 무엇을 옮겼는지가
    // 이 시험의 관측 대상이기 때문이다.
    // `crates/` 도 실재시킨다 — 좌변에 있는데 디스크에 없으면 판정기가 건너뛰고
    // tokei 가 그 없는 사본을 열다 죽어, 이 시험이 좌변이 아니라 **빈 뿌리**를 잰다.
    for (rel, n) in [(P, BUDGET), ("extra/e.rs", 100), ("crates/zz/src/z.rs", 1)] {
        let f = d.path().join(rel);
        fs::create_dir_all(f.parent().expect("부모")).expect("디렉토리");
        let body: String = (0..n).map(|i| format!("fn f{i}() {{}}\n")).collect();
        fs::write(&f, body).expect("실물 소스");
    }

    let bin = tempfile::tempdir().expect("스텁 디렉토리");
    write_exec(
        &bin.path().join("tokei"),
        "#!/bin/sh\n\
         shift 2\n\
         for d in \"$@\"; do [ -d \"$d\" ] || exit 1; done\n\
         files=$(for d in \"$@\"; do find \"$d\" -name '*.rs' -type f; done)\n\
         printf '{\"Rust\":{\"reports\":['\n\
         sep=\"\"\n\
         for f in $files; do\n\
         n=$(wc -l < \"$f\" | tr -d ' ')\n\
         case \"$f\" in __probe/*) n=1 ;; esac\n\
         printf '%s{\"name\":\"%s\",\"stats\":{\"code\":%s}}' \"$sep\" \"$f\" \"$n\"\n\
         sep=\",\"\n\
         done\n\
         printf ']}}'\n",
    );
    let strip = bin.path().join("strip-cfg-test");
    write_exec(
        &strip,
        "#!/bin/sh\n\
         if [ \"$1\" = \"--check-fresh\" ]; then exit 0; fi\n\
         while [ \"${1#--}\" != \"$1\" ]; do shift; done\n\
         out=$1; shift; src=$1; shift\n\
         mkdir -p \"$out\" || exit 1\n\
         for d in \"$@\"; do\n\
         [ -d \"$src/$d\" ] || continue\n\
         cp -r \"$src/$d\" \"$out/$d\" || exit 1\n\
         done\n\
         exit 0\n",
    );
    let path = format!(
        "{}:{}",
        bin.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let out = Command::new("bash")
        .arg(d.path().join("scripts/check-frozen-sum-ratchet.sh"))
        .env("PATH", path)
        .env("TASTY_STRIP_CFG_TEST_BIN", &strip)
        .current_dir(d.path())
        .output()
        .expect("게이트 실행");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        out.status.code().unwrap_or(-1),
        0,
        "좌변이 판정기에 안 닿았다 — 사본에 없는 디렉토리를 tokei 가 열다 죽는다:\n{text}"
    );
}

/// ★ R1056 축 — **통과줄의 "남은 여유" 도 rc 에 안 들어간다.**
///
/// 이 게이트의 rc 는 `SUM` 과 천장·예산의 비교로만 갈린다. 통과줄이 찍는 남은 여유는
/// 판정에 안 들어가고, 실측(2026-09-08) 그것을 상수 `0` 으로 바꾸는 변이에서 위
/// 열다섯이 전부 통과했다.
///
/// 이 자리는 **이미 한 번 틀렸던 자리**다. 게이트 본문이 그 사고를 적어 둔다 — 통과줄이
/// 식을 떼고 띠(`SLACK`)만 남기자 그 상수가 남은 여유로 읽혀, lane 이 924 를 보고하는데
/// 이 줄은 1000 을 찍는 어긋남이 났다. 그래서 이 시험은 **띠와 다른 값**이 나오게
/// 픽스처를 짠다: 합을 예산보다 띠의 절반만큼 올려 여유가 500 이 되게 한다. 여유와
/// 띠가 같은 값이면 두 이름이 한 수로 붕괴한 상태도 통과한다.
#[test]
fn the_reported_headroom_is_the_ceiling_minus_the_sum() {
    let sum = BUDGET + SLACK / 2; // 천장까지 SLACK/2 = 500 남는다
    let d = root_with(Some(BUDGET), &[P]);
    write_sibling(d.path(), Some(SLACK), Some("src crates"));
    let bin = tempfile::tempdir().expect("스텁 디렉토리");
    write_exec(
        &bin.path().join("tokei"),
        &format!("#!/bin/sh\n{}\n", reports(P, sum)),
    );
    let strip = bin.path().join("strip-cfg-test");
    write_exec(
        &strip,
        &format!("#!/bin/sh\n{EAT_FLAGS}mkdir -p \"$1\"\nexit 0\n"),
    );
    let path = format!(
        "{}:{}",
        bin.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let out = Command::new("bash")
        .arg(d.path().join("scripts/check-frozen-sum-ratchet.sh"))
        .env("PATH", path)
        .env("TASTY_STRIP_CFG_TEST_BIN", &strip)
        .current_dir(d.path())
        .output()
        .expect("게이트 실행");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        out.status.code().unwrap_or(-1),
        0,
        "천장 안인데 초록이 아니다:\n{text}"
    );
    assert!(
        text.contains(&format!("남은 여유 {}", SLACK / 2)),
        "남은 여유가 천장 − 합이 아니다 — 띠({SLACK})가 그 자리로 읽히면 lane 이 재는 \
         값과 이 줄이 갈린다:\n{text}"
    );
}

// ── 거절은 **호출 지점마다** 선다 ────────────────────────────────────────
//
// 이 게이트의 거절은 공용 `die()` 하나를 열 곳에서 부른다. 그래서 `die` 의 **정의**를
// `exit 0` 으로 바꾸는 변이는 시원하게 죽지만(실측 2026-09-08: 이 파일의 시험 여섯이
// 한꺼번에 빨개졌다), **호출 지점 하나**를 폴백으로 바꾸는 완화는 그 여섯 중 어느 것도
// 못 본다 — 뒤에 오는 다른 호출 지점이 같은 rc 를 대신 내주기 때문이다. 정의와 호출
// 지점이 같은 저울에 올라가 있었다는 뜻이고, 초록은 정의 쪽 하나만 보고 있었다.
//
// 실측(2026-09-08) 호출 지점 열에 완화 변이를 하나씩 넣은 결과: 넷이 죽고 **여섯이
// 살아남았다.** 아래 넷과 위에서 고친 하나가 그중 다섯을 각자의 저울에 올린다.
// 남은 하나(tokei 실행 실패)는 아래 `a_tokei_that_fails_is_shadowed_by_the_parser` 가
// 왜 따로 못 서는지를 값으로 적는다.

/// 도구 부재·판정기 실패 갈래 전용 실행기 — rc 와 **출력**을 함께 준다.
///
/// `keep` 가 `Some` 이면 PATH 를 그 이름들로만 채운다. 도구의 부재는 환경변수가 아니라
/// **자리**의 문제라 그렇게만 만들 수 있다(`gate_env` 참조). 출력까지 보는 이유는
/// rc 2 가 열 곳에서 나오기 때문이다 — rc 만 보면 다른 호출 지점이 낸 2 를 자기 것으로
/// 읽고, 그것이 바로 이 절이 고치는 병이다.
fn run_bare(
    root: &Path,
    keep: Option<&[&str]>,
    tokei: Option<&str>,
    strip_body: &str,
) -> (i32, String) {
    let bin = tempfile::tempdir().expect("스텁 디렉토리");
    if let Some(body) = tokei {
        write_exec(&bin.path().join("tokei"), &format!("#!/bin/sh\n{body}\n"));
    }
    let strip = bin.path().join("strip-cfg-test");
    write_exec(&strip, strip_body);
    let held;
    let tail = match keep {
        Some(names) => {
            held = gate_env::only(names);
            held.path().display().to_string()
        }
        None => std::env::var("PATH").unwrap_or_default(),
    };
    let out = Command::new("bash")
        .arg(root.join("scripts/check-frozen-sum-ratchet.sh"))
        .env("PATH", format!("{}:{}", bin.path().display(), tail))
        .env("TASTY_STRIP_CFG_TEST_BIN", &strip)
        .current_dir(root)
        .output()
        .expect("게이트 실행");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.code().unwrap_or(-1), text)
}

/// 신선도 물음에는 답하고 본 일에서 죽는 판정기 스텁.
///
/// `--check-fresh` 를 안 가르면 `resolve_judge` 가 **없는 판정기**로 다뤄, 그 다음
/// 호출 지점("판정기를 못 쓴다")이 먼저 답한다 — 재려던 자리가 아니다.
const STRIP_FAILS: &str = "#!/bin/sh\nif [ \"$1\" = \"--check-fresh\" ]; then exit 0; fi\nexit 7\n";
const STRIP_OK: &str = "#!/bin/sh\nif [ \"$1\" = \"--check-fresh\" ]; then exit 0; fi\n\
     while [ \"${1#--}\" != \"$1\" ]; do shift; done\nmkdir -p \"$1\"\nexit 0\n";

/// 자매의 좌변이 **공백뿐**이면 판정 불가(2)다.
///
/// `SCAN_DIRS=()` 로는 이 자리에 못 닿는다 — 그때는 읽은 문자열이 비어 앞 호출 지점이
/// 답한다. 공백만 있는 괄호가 "읽히긴 했는데 훑을 트리가 없다" 를 만드는 유일한 형태다.
#[test]
fn a_sibling_whose_scan_dirs_are_blank_is_undecidable() {
    let d = root_with(Some(BUDGET), &[P]);
    write_sibling(d.path(), Some(SLACK), Some("   "));
    let (code, text) = run_bare(d.path(), None, Some(&reports(P, BUDGET)), STRIP_OK);
    assert_eq!(code, 2, "훑을 트리가 없는데 값을 냈다:\n{text}");
    assert!(
        text.contains("SCAN_DIRS 가 비었다"),
        "다른 호출 지점이 낸 2 다 — 이 시험은 자기 자리를 안 재고 있다:\n{text}"
    );
}

/// tokei 가 없으면 판정 불가(2)다. **통과가 아니다.**
#[test]
fn a_missing_tokei_is_undecidable() {
    let d = root_with(Some(BUDGET), &[P]);
    let (code, text) = run_bare(
        d.path(),
        Some(&["bash", "dirname", "sed", "mktemp", "rm", "python3"]),
        None,
        STRIP_OK,
    );
    assert_eq!(code, 2, "tokei 가 없는데 판정 불가가 아니다:\n{text}");
    assert!(
        text.contains("tokei 미설치"),
        "다른 호출 지점이 낸 2 다:\n{text}"
    );
}

/// python 이 없으면 판정 불가(2)다. 이 자리는 tokei 검사 **뒤**라, 앞의 것을 살려 둬야
/// 여기까지 온다.
#[test]
fn a_missing_python_is_undecidable() {
    let d = root_with(Some(BUDGET), &[P]);
    let (code, text) = run_bare(
        d.path(),
        Some(&["bash", "dirname", "sed", "mktemp", "rm", "tokei"]),
        None,
        STRIP_OK,
    );
    assert_eq!(code, 2, "python 이 없는데 판정 불가가 아니다:\n{text}");
    assert!(
        text.contains("python 미설치"),
        "다른 호출 지점이 낸 2 다:\n{text}"
    );
}

/// 판정기가 **있는데 죽으면** 판정 불가(2)다.
///
/// 부재(`every_judge_consumer_pins_its_absence_branch` 가 보는 자리)와 다른 사건이다 —
/// 여기서 `|| true` 로 물러나면 사본이 안 만들어진 채 tokei 가 빈 트리를 읽고, 그 값이
/// "동결분이 줄었다" 로 나가 래칫을 **조이라는 틀린 지시**가 된다.
#[test]
fn a_stripper_that_fails_is_undecidable() {
    let d = root_with(Some(BUDGET), &[P]);
    let (code, text) = run_bare(d.path(), None, Some(&reports(P, BUDGET)), STRIP_FAILS);
    assert_eq!(code, 2, "판정기가 죽었는데 값을 냈다:\n{text}");
    assert!(
        text.contains("출하 줄 판정 실패"),
        "다른 호출 지점이 낸 2 다:\n{text}"
    );
}

/// ★ 이 자리는 **따로 못 선다** — 그 사실을 값으로 적는다.
///
/// tokei 실행 실패의 거절을 지우면 `set -e` 가 tokei 의 rc 를 그대로 내보내고,
/// 폴백으로 바꾸면 그 폴백이 무엇이든 파서가 "보고가 비었다" 로 받아 **다음** 호출
/// 지점이 2 를 낸다. 즉 이 자리의 완화는 관측 가능한 결과를 안 바꾼다(실측 2026-09-08:
/// 빈 보고를 넣는 완화 변이에서 이 파일의 어느 시험도 안 죽었고, 게이트 rc 는 2 그대로였다).
///
/// 그래서 여기 두는 것은 그 자리의 시험이 아니라 **가려짐 자체의 고정**이다: 폴백이
/// 있든 없든 rc 는 2 여야 한다. 이 단정이 깨지는 날은 파서의 빈-보고 거절이 사라진
/// 날이고, 그때는 이 자리에 진짜 시험이 필요해진다.
#[test]
fn a_tokei_that_fails_is_shadowed_by_the_parser() {
    let d = root_with(Some(BUDGET), &[P]);
    let (code, text) = run_bare(d.path(), None, Some("exit 3"), STRIP_OK);
    assert_eq!(code, 2, "tokei 가 죽었는데 값을 냈다:\n{text}");
    let (code_empty, text_empty) = run_bare(
        d.path(),
        None,
        Some(r#"echo '{"Rust":{"reports":[]}}'"#),
        STRIP_OK,
    );
    assert_eq!(
        code_empty, 2,
        "빈 보고가 판정 불가가 아니다 — 이 자리를 가려 주던 파서의 거절이 사라졌으니 \
         tokei 실패 갈래에 자기 시험이 필요하다:\n{text_empty}"
    );
}

// ── 실패문의 갈래(822 의 절) ──────────────────────────────────────────
//
// 위 절이 저울의 **단위**를 묻는다면 이 절은 실패문의 **갈래**를 묻는다. 같은 파일에서
// 다른 물음을 푸는 두 절이라 하나로 접지 않는다.

/// 미달 분기가 **원인을 단정하지 않는다.**
///
/// 예산은 되돌아 올라가지 않으므로, 틀린 갈래에서 내린 한 번이 영구히 그만큼을 안 보게
/// 만든다. 형제 둘(`check-allow-reason.sh` · `check-shared-walk-ratchet.sh`)은 같은
/// 자리에서 갈래를 열거하는데 이 게이트만 안 열고 있었다 — 실측(2026-09-08) 갈래 열거
/// 줄 수가 1 · 1 · **0** 이었고, 합이 줄면 곧바로 "예산 줄을 이 값으로 내린다" 로 갔다.
///
/// **rc 로는 이 자리가 안 보인다.** 갈래를 열든 안 열든 미달은 1 이다. 그래서 이 시험은
/// 종료 코드와 **문구**를 함께 건다. 문구 전체가 아니라 갈래 넷이 존재한다는 조각만
/// 건다 — 문장은 다듬어도 되고, 다듬을 때마다 이 시험이 죽으면 아무도 안 다듬는다.
///
/// **갈래 수는 이 배열에만 적는다.** 한때 스크립트가 ㄹ 을 열었는데 이 배열은 셋이라,
/// ㄹ 을 통째로 지워도 여기가 초록이었다(실측 2026-09-09: 그 네 줄을 지우고 22 passed).
/// 갈래를 열다 만 목록은 안 연 것과 같다는 이 시험의 실패문이, 정확히 그 상태를 자기가
/// 못 잡고 있었다.
///
/// 갈래 ㄴ(목록에서 항목이 빠졌다)은 이 게이트에만 있다. 형제 둘의 좌변에는 "목록" 이
/// 없다. 그 갈래에서 형제의 색을 먼저 보라고 말하는지도 함께 건다 — 그 안내가 없으면
/// 두 게이트가 함께 빨간 상태에서 이쪽 처방만 따르게 된다.
///
/// 갈래 ㄹ(술어를 고쳤다)에는 **요건**을 함께 건다. 글자만 걸면 "고쳤다" 는 자기 신고가
/// 통행증이 되는데, ㄷ(술어가 깨져 덜 센다)과 ㄹ 은 관측이 같고 예산은 되돌아 올라가지
/// 않는다 — 틀린 한 번이 영구적이다. 요건 둘 중 둘째(내려감이 한 형태에 갇혔는가)는
/// 파일별 내역이 있어야 볼 수 있으므로 그 표가 찍히는지도 함께 건다.
/// 근거·기각한 대안: `docs/adr/0258-the-measured-copy-is-neutralized-for-the-counter.md`
#[test]
fn the_under_budget_branch_does_not_name_a_cause() {
    let d = root_with(Some(BUDGET), &[P]);
    write_sibling(d.path(), Some(SLACK), Some("src crates"));
    let bin = tempfile::tempdir().expect("스텁 디렉토리");
    write_exec(
        &bin.path().join("tokei"),
        &format!("#!/bin/sh\n{}\n", reports(P, BUDGET - 1)),
    );
    let strip = bin.path().join("strip-cfg-test");
    write_exec(
        &strip,
        &format!("#!/bin/sh\n{EAT_FLAGS}mkdir -p \"$1\"\nexit 0\n"),
    );
    let path = format!(
        "{}:{}",
        bin.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let out = Command::new("bash")
        .arg(d.path().join("scripts/check-frozen-sum-ratchet.sh"))
        .env("PATH", path)
        .env("TASTY_STRIP_CFG_TEST_BIN", &strip)
        .current_dir(d.path())
        .output()
        .expect("게이트 실행");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        out.status.code().unwrap_or(-1),
        1,
        "예산 아래인데 위반이 아니다:\n{text}"
    );
    assert!(
        text.contains("원인을 말하지 않는다"),
        "미달 분기가 원인을 단정한다 — 예산은 되돌아 올라가지 않으므로 틀린 갈래에서 \
         내린 한 번이 영구히 그만큼을 안 보게 만든다:\n{text}"
    );
    for branch in ["ㄱ ", "ㄴ ", "ㄷ ", "ㄹ "] {
        // 좌변은 **열거 항목 줄**이다 — `contains` 가 아니다. 글자를 파일 어디서나 찾으면
        // 다른 갈래의 본문이 그 글자를 언급하는 것만으로 채널이 채워진다. 실측
        // (2026-09-09): `contains` 판에서 ㄱ·ㄷ 의 echo 를 지워도 22 passed 였다 — 마무리
        // 줄 "ㄱ 이나 ㄹ 임을 확인한 뒤에만" 이 ㄱ 을, ㄹ 의 본문 "★ ㄷ 과 가르는 것은" 이
        // ㄷ 을 대신 채우고 있었다. 열거 항목은 들여쓰인 줄의 **첫 글자**라는 것이 그 둘과
        // 갈리는 성질이고, 들여쓰기 폭에는 안 걸리게 `trim_start` 로 본다.
        //
        // 이 좌변에 남은 성질상의 구멍 둘 — 현행 본문에서는 안 나고 인위 변이에서만 난다.
        // (1) 항목 글자 앞에 다른 글자가 붙으면(예: `★ ㄱ `) 갈래가 열려 있는데도 **거짓
        //     실패**가 난다. 좌변이 `trim_start` 뒤 **첫 글자**라 그렇다.
        // (2) 다른 들여쓴 줄이 우연히 같은 글자로 시작하면 여전히 채널이 채워진다 —
        //     `contains` 보다 좁을 뿐 "열거 항목" 자체를 판정하는 것은 아니다.
        // 둘 다 본문 문구를 고칠 때 깨질 수 있으므로, 여기를 손보는 사람이 알고 고치라고
        // 적어 둔다(지금 더 좁히지 않는 이유는 그 판정에 마크다운 파서가 필요해서다).
        assert!(
            text.lines()
                .any(|l| l.starts_with(' ') && l.trim_start().starts_with(branch)),
            "갈래 '{branch}' 가 열거 항목으로 없다 — 갈래를 열다 만 목록은 안 연 것과 \
             같다(다른 갈래 본문의 언급은 이 갈래를 연 것이 아니다):\n{text}"
        );
    }
    assert!(
        text.contains("check-file-size.sh"),
        "목록에서 항목이 빠진 갈래에서 형제의 색을 먼저 보라고 말하지 않는다 — 그 안내가 \
         없으면 두 게이트가 함께 빨간 상태에서 이쪽 처방만 따라 예산이 내려간다:\n{text}"
    );
    assert!(
        text.contains("최소 재현"),
        "ㄹ 이 ㄷ 과 갈리는 요건을 안 건다 — 갈래 글자만 있고 요건이 없으면 '술어를 \
         고쳤다' 는 자기 신고가 곧 통행증이 되고, 예산은 되돌아 올라가지 않는다:\n{text}"
    );
    assert!(
        text.lines().any(|l| l.contains(P)),
        "미달 갈래가 파일별 내역을 안 찍는다 — 내려감이 한 형태에 갇혔는지는 파일별로만 \
         보이고, 그 표가 없으면 ㄹ 의 둘째 요건을 아무도 확인할 수 없다:\n{text}"
    );
}

// ── 둘째 좌변 — 계측기의 doc 주석 편향 ─────────────────────────────────
//
// 이 게이트의 합은 실제 출하 줄이 아니라 **tokei 가 센 값**이고, 둘은 상수만큼 다르다.
// 그 상수는 예산에도 같이 박혀 있어 절대값은 상쇄되지만 **움직이면** 합이 성장 없이
// 움직인다. 방향에 따라 나타나는 곳이 다르다: 편향이 늘면 합이 내려가 미달 갈래로
// 위장하고, 줄면 합이 올라가 **띠 안에 통째로 들어간다** — 뒤엣것은 아무 데서도 안
// 울린다. 그래서 여기가 저울 하나를 더 둔다. 근거·기각한 대안(상시 독립 계측 대조 ·
// 차이의 상한 래칫): `docs/adr/0258-the-measured-copy-is-neutralized-for-the-counter.md`

/// 대조군 — 편향이 고정값과 같으면 통과다.
///
/// 이것이 없으면 아래 둘은 "편향을 항상 거절하는 게이트" 로도 통과한다.
#[test]
fn a_bias_that_matches_the_pin_passes() {
    let d = root_with_bias(Some(BUDGET), Some(3), &[P]);
    assert_eq!(
        run(d.path(), &reports_with(P, BUDGET, BUDGET + 3, 1)),
        0,
        "편향이 고정값과 같은데 통과가 아니다"
    );
}

/// 편향이 **늘면** 실패다 — 합이 그만큼 내려가 미달로 위장하는 방향.
#[test]
fn a_bias_that_grew_fails() {
    let d = root_with_bias(Some(BUDGET), Some(3), &[P]);
    assert_eq!(
        run(d.path(), &reports_with(P, BUDGET, BUDGET + 4, 1)),
        1,
        "편향이 늘었는데 조용하다 — 그 폭만큼 합이 내려가고, 그것을 ㄱ 이나 ㄹ 로 읽으면 \
         예산이 성장 없이 영구히 내려간다"
    );
}

/// 편향이 **줄어도** 실패다. 한 방향만 서면 래칫이 아니다.
///
/// 이쪽이 조용한 방향이다 — 합이 올라가는데 띠가 파일 하나 몫이라 통째로 삼켜진다.
/// "남는 여유는 곧 안 보는 구간" 이라는 이 게이트 자신의 명제가 그만큼 깨진다.
#[test]
fn a_bias_that_shrank_also_fails() {
    let d = root_with_bias(Some(BUDGET), Some(3), &[P]);
    assert_eq!(
        run(d.path(), &reports_with(P, BUDGET, BUDGET + 2, 1)),
        1,
        "편향이 줄었는데 조용하다 — 이 방향은 합을 올리므로 띠 안에서 아무 데서도 안 운다"
    );
}

/// 편향 핀이 없으면 **판정 불가(2)** 다. 0 으로 물러나지 않는다.
///
/// 물러나면 편향이 있는 트리에서 매번 거짓 위반이 나고, 그 처방을 따르면 예산이
/// 편향만큼 잘못 움직인다.
#[test]
fn a_missing_bias_line_is_a_measurement_failure() {
    let d = root_with_bias(Some(BUDGET), None, &[P]);
    // rc 만 보면 안 된다 — 이 게이트의 판정 불가는 열 곳 넘는 자리에서 같은 2 로 나오고,
    // 그러면 다른 자리가 낸 2 를 이 자리의 것으로 읽는다. 그래서 **자기 말**을 함께 건다
    // (`tests/refusals_are_told_apart_by_their_own_words.rs` 가 세는 좌변이 그것이다).
    let (code, text) = run_bare(d.path(), None, Some(&reports(P, BUDGET)), STRIP_OK);
    assert_eq!(
        code, 2,
        "편향 줄이 없는데 값을 냈다 — 좌변 하나가 꺼진 채로 도는 것은 통과가 아니다:\n{text}"
    );
    assert!(
        text.contains("'# doc-comment-bias: <수>' 줄이 없다"),
        "편향 줄이 없다는 사실을 자기 말로 말하지 않는다 — 다른 판정 불가 자리와 rc 로 \
         구별되지 않으므로 이 자리의 완화가 조용해진다:\n{text}"
    );
}

/// 편향 실패문이 **폭만큼** 옮기라고 말하는가 — 합에 맞추라고 하면 안 된다.
///
/// 예산을 합으로 맞추면 같은 커밋에 섞인 진짜 성장까지 함께 사면된다. 옮길 폭은
/// 편향의 차뿐이고, 성장은 그 뒤 합 판정이 봐야 한다. 그리고 그 실패문은 **어느
/// 파일이 오독 형태를 보유하는지**를 함께 찍는다 — ADR 의 요건 (2)("내려감이 그
/// 형태를 담은 파일에 갇혔는가")의 앞쪽 절반이 그 표 없이는 손 측정으로 남는다.
#[test]
fn the_bias_failure_moves_both_pins_by_the_delta() {
    let d = root_with_bias(Some(BUDGET), Some(3), &[P]);
    let (code, text) = run_bare(
        d.path(),
        None,
        Some(&reports_with(P, BUDGET, BUDGET + 5, 1)),
        STRIP_OK,
    );
    assert_eq!(code, 1, "편향이 갈렸는데 위반이 아니다:\n{text}");
    assert!(
        text.contains(&format!("# frozen-sum-budget: {}", BUDGET - 2)),
        "예산을 **차(2)만큼** 옮기라고 말하지 않는다 — 합으로 맞추라고 하면 같은 \
         커밋의 진짜 성장이 함께 사면된다:\n{text}"
    );
    assert!(
        text.contains("# doc-comment-bias: 5"),
        "새 편향 값을 안 알려준다 — 좌변이 둘인데 하나만 알려주면 다음 회차가 남은 \
         하나를 손으로 찾는다:\n{text}"
    );
    assert!(
        text.lines().any(|l| l.contains(P)),
        "어느 파일이 오독 형태를 보유하는지를 안 찍는다 — 그 표가 없으면 ㄹ 의 둘째 \
         요건 앞쪽 절반이 손 측정으로 남는다:\n{text}"
    );
}

/// 오독의 **형태**가 달라지면 합을 판정하기 전에 먼저 선다.
///
/// 편향의 수만 고정하면 "얼마나" 는 지키지만 "무엇 때문에" 는 안 지킨다. 계측기가 이
/// 오독을 고치거나 다른 형태로 바꾸면 편향의 수는 여전히 어떤 값을 갖는데 그 뜻이
/// 달라진다. 그래서 게이트가 세 줄짜리 최소 재현을 매번 함께 재고, 그 값이 갈리면
/// 합·편향을 판정하기 전에 멈춘다. ADR 이 갈래 ㄹ 의 요건 (1)로 요구하는 "형태의
/// 최소 재현" 이 이 자리에 산다.
#[test]
fn the_form_probe_stands_before_the_sum() {
    let d = root_with_bias(Some(BUDGET), Some(0), &[P]);
    // 합도 편향도 멀쩡한데 프로브만 갈린 트리. 그래도 멈춰야 한다.
    let (code, text) = run_bare(
        d.path(),
        None,
        Some(&reports_with(P, BUDGET, BUDGET, 2)),
        STRIP_OK,
    );
    assert_eq!(
        code, 1,
        "계측기의 오독 형태가 달라졌는데 값을 냈다 — 그 값의 뜻이 어제와 다르다:\n{text}"
    );
    assert!(
        text.contains("형태"),
        "무엇이 달라졌는지를 안 말한다 — 편향의 수만 갱신하면 형태가 바뀐 사실이 \
         값 뒤에 묻힌다:\n{text}"
    );
}
