//! 허용 목록 파일의 SLOC 합에 대한 예산 검사를 합성 저장소와 스텁으로 검증한다.
//! 개별 파일 검사의 예외도 총합 성장 제한을 받는다. 통과·위반·측정 실패의 종료 코드는 구별한다.
//! 실제 판정기를 여기서 빌드하면 바깥 Cargo 시험과 빌드 잠금을 기다릴 수 있어 스텁을 사용한다.

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

/// 총합 검사는 파일 크기 게이트의 임계·수집 범위·판정 플래그를 읽는다.
/// 검증하려는 항목만 바꾸고 나머지는 유효하게 남겨 다른 읽기 오류가 결과를 가리지 않게 한다.
fn write_sibling(root: &Path, threshold: Option<i64>, scan_dirs: Option<&str>) {
    write_sibling_with_judge_flags(
        root,
        threshold,
        scan_dirs,
        "--neutralize-char-literal-quotes --blank-test-only-files",
    );
}

fn write_sibling_with_judge_flags(
    root: &Path,
    threshold: Option<i64>,
    scan_dirs: Option<&str>,
    judge_flags: &str,
) {
    let mut body = sibling_head(threshold, scan_dirs);
    body.push_str(&format!("SHIPPING_JUDGE_FLAGS=({judge_flags})\n"));
    fs::write(root.join("scripts/check-file-size.sh"), body).expect("자매 게이트 스텁");
}

fn write_sibling_without_judge_flags(root: &Path, threshold: Option<i64>, scan_dirs: Option<&str>) {
    let body = sibling_head(threshold, scan_dirs);
    fs::write(root.join("scripts/check-file-size.sh"), body).expect("자매 게이트 스텁");
}

fn sibling_head(threshold: Option<i64>, scan_dirs: Option<&str>) -> String {
    let mut body = String::from("#!/usr/bin/env bash\n");
    if let Some(t) = threshold {
        body.push_str(&format!("THRESHOLD={t}\n"));
    }
    if let Some(d) = scan_dirs {
        body.push_str(&format!("SCAN_DIRS=({d})\n"));
    }
    body
}

fn root_with(budget_line: Option<i64>, entries: &[&str]) -> tempfile::TempDir {
    root_with_bias(budget_line, Some(0), entries)
}

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
    fs::create_dir_all(root.join("scripts/lib")).expect("scripts/lib");
    fs::copy(
        format!("{here}/scripts/lib/judge-bin.sh"),
        root.join("scripts/lib/judge-bin.sh"),
    )
    .expect("판정기 찾기 공용 복사");
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

/// 스텁은 모든 -- 플래그를 제거한다. 실제 도구보다 관대하므로 지원하는 플래그의 일치 여부는 검증하지 않는다.
const EAT_FLAGS: &str = "while [ \"${1#--}\" != \"$1\" ]; do shift; done\n";

fn run(root: &Path, tokei_body: &str) -> gate_env::GateRun {
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
        .map(|out| gate_env::GateRun::from_output(&out))
        .expect("게이트 실행")
}

/// 사본·doc 주석 제거본·형태 재현 입력의 보고를 모두 만든다. 기본값은 편향 0, 형태 결과 1로 합 검사만 분리한다.
fn reports(path: &str, code: i64) -> String {
    reports_with(path, code, code, 1)
}

fn reports_with(path: &str, code: i64, nodoc_code: i64, probe_code: i64) -> String {
    format!(
        r#"echo '{{"Rust":{{"reports":[{{"name":"{path}","stats":{{"code":{code}}}}},{{"name":"__nodoc/{path}","stats":{{"code":{nodoc_code}}}}},{{"name":"__probe/probe.rs","stats":{{"code":{probe_code}}}}}]}}}}'"#
    )
}

const P: &str = "src/frozen_one.rs";

#[test]
fn a_sum_at_the_budget_passes() {
    let d = root_with(Some(BUDGET), &[P]);
    assert_eq!(
        run(d.path(), &reports(P, BUDGET)),
        0,
        "예산과 같으면 통과다"
    );
}

#[test]
fn growth_of_exactly_one_file_worth_is_still_inside() {
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
        "동결 파일의 합이 예산과 허용 여유를 넘으면 위반이다"
    );
}

#[test]
fn shrinking_below_the_budget_also_fails() {
    let d = root_with(Some(BUDGET), &[P]);
    assert_eq!(
        run(d.path(), &reports(P, BUDGET - 1)),
        1,
        "합이 예산 아래로 내려가면 예산을 내리라고 실패해야 한다"
    );
}

#[test]
fn the_slack_is_read_from_the_file_size_gate() {
    let d = root_with(Some(BUDGET), &[P]);
    // 수집 범위는 남기고 임계만 바꿔 읽기 실패와 구별한다.
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

/// 임계 읽기 실패만 검사하도록 SCAN_DIRS는 남긴다. 둘 다 없으면 뒤의 오류가 앞의 오류를 가릴 수 있다.
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
    assert_eq!(run(d.path(), "exit 3"), 2, "tokei 실행 실패는 측정 실패다");
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
    // 실재하는 파일의 보고 누락은 삭제와 구별해야 한다. 누락을 0으로 세면 예산을 잘못 낮추게 된다.
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
    let d = root_with(Some(BUDGET), &[P, "src/frozen_gone.rs"]);
    assert_eq!(
        run(d.path(), &reports(P, BUDGET)),
        0,
        "목록에 있으나 디스크에 없는 경로는 0 으로 세고 통과여야 한다"
    );
}

#[test]
fn files_outside_the_allowlist_do_not_count() {
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

/// 범위에 extra가 들어오면 보고를 추가한다. 파일을 실제로 만들면 보고 누락 검사가 먼저 실패하므로 여기서는 만들지 않는다.
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

/// 추가된 파일의 합에 맞춰 예산을 둔다. 범위가 전달되지 않으면 합이 예산 미만이 돼 실패한다.
#[test]
fn the_scan_dirs_are_read_from_the_file_size_gate() {
    let d = root_with(Some(BUDGET + 100), &[P, "extra/e.rs"]);
    write_sibling(d.path(), Some(SLACK), Some("src crates extra"));
    assert_eq!(
        run(d.path(), &reports_sensitive_to_scan_dirs(BUDGET, 100)),
        0,
        "파일 크기 게이트의 수집 범위를 늘렸지만 총합 검사에 반영되지 않았다"
    );
}

#[test]
fn a_sibling_without_scan_dirs_is_undecidable() {
    let d = root_with(Some(BUDGET), &[P]);
    write_sibling(d.path(), Some(SLACK), None);
    assert_eq!(
        run(d.path(), &reports(P, BUDGET)),
        2,
        "수집 범위 선언이 없는데도 총합을 계산했다"
    );
}

/// 고정 JSON 스텁으로는 사본 생성 범위를 확인할 수 없어 실제 복사·디스크 읽기를 하는 스텁을 사용한다.
#[test]
fn the_scan_dirs_reach_the_stripper() {
    let d = root_with(Some(BUDGET + 100), &[P, "extra/e.rs"]);
    write_sibling(d.path(), Some(SLACK), Some("src crates extra"));
    // 수집 범위의 디렉터리를 모두 만든다. 그래야 범위 전달 대신 디렉터리 부재 오류를 검사하지 않는다.
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
        "수집 범위가 사본 생성 도구에 전달되지 않았다. 디렉터리 인자를 확인한다:\n{text}"
    );
}

/// 남은 여유는 종료 코드에 반영되지 않는다. SLACK과 다른 값으로 설정해 상수를 잘못 출력하는 경우도 찾는다.
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
        "상한 이내 입력의 검사가 실패했다:\n{text}"
    );
    assert!(
        text.contains(&format!("남은 여유 {}", SLACK / 2)),
        "남은 여유가 상한에서 합을 뺀 값과 다르다. SLACK {SLACK}을 그대로 출력하지 않아야 한다:\n{text}"
    );
}

/// keep으로 보이는 도구를 제한한다. 여러 오류가 rc 2를 쓰므로 원인을 구별할 출력도 반환한다.
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

/// 신선도 확인에는 성공해야 실제 판정 실행 실패를 검사할 수 있다.
const STRIP_FAILS: &str = "#!/bin/sh\nif [ \"$1\" = \"--check-fresh\" ]; then exit 0; fi\nexit 7\n";
const STRIP_OK: &str = "#!/bin/sh\nif [ \"$1\" = \"--check-fresh\" ]; then exit 0; fi\n\
     while [ \"${1#--}\" != \"$1\" ]; do shift; done\nmkdir -p \"$1\"\nexit 0\n";

/// 빈 괄호는 앞의 읽기 오류에 걸린다. 공백만 넣어 선언은 읽혔지만 범위가 빈 경우를 만든다.
#[test]
fn a_sibling_whose_scan_dirs_are_blank_is_undecidable() {
    let d = root_with(Some(BUDGET), &[P]);
    write_sibling(d.path(), Some(SLACK), Some("   "));
    let (code, text) = run_bare(d.path(), None, Some(&reports(P, BUDGET)), STRIP_OK);
    assert_eq!(code, 2, "훑을 트리가 없는데 값을 냈다:\n{text}");
    assert!(
        text.contains("SCAN_DIRS 가 비었다"),
        "예상한 오류의 진단이 없다:\n{text}"
    );
}

#[test]
fn a_sibling_without_judge_flags_is_undecidable() {
    let d = root_with(Some(BUDGET), &[P]);
    write_sibling_without_judge_flags(d.path(), Some(SLACK), Some("src crates"));
    let (code, text) = run_bare(d.path(), None, Some(&reports(P, BUDGET)), STRIP_OK);
    assert_eq!(code, 2, "부를 방법이 없는데 값을 냈다:\n{text}");
    assert!(
        text.contains("SHIPPING_JUDGE_FLAGS 를 못 읽었다"),
        "예상한 오류의 진단이 없다:\n{text}"
    );
}

/// 빈 괄호는 앞의 읽기 오류에 걸리므로 공백을 넣어 읽힌 플래그 목록이 빈 경우를 만든다.
#[test]
fn a_sibling_whose_judge_flags_are_empty_is_undecidable() {
    let d = root_with(Some(BUDGET), &[P]);
    write_sibling_with_judge_flags(d.path(), Some(SLACK), Some("src crates"), "   ");
    let (code, text) = run_bare(d.path(), None, Some(&reports(P, BUDGET)), STRIP_OK);
    assert_eq!(code, 2, "판정 방식이 없는데 값을 냈다:\n{text}");
    assert!(
        text.contains("SHIPPING_JUDGE_FLAGS 가 비었다"),
        "예상한 오류의 진단이 없다:\n{text}"
    );
}

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
        "예상한 오류의 진단이 없다:\n{text}"
    );
}

/// Python 확인은 tokei 뒤에 있으므로 tokei는 남긴다.
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
        "예상한 오류의 진단이 없다:\n{text}"
    );
}

#[test]
fn a_stripper_that_fails_is_undecidable() {
    let d = root_with(Some(BUDGET), &[P]);
    let (code, text) = run_bare(d.path(), None, Some(&reports(P, BUDGET)), STRIP_FAILS);
    assert_eq!(
        code, 2,
        "판정 도구가 실패했는데 측정 실패로 보고하지 않았다:\n{text}"
    );
    assert!(
        text.contains("출하 줄 판정 실패"),
        "예상한 오류의 진단이 없다:\n{text}"
    );
}

/// tokei 실패와 빈 보고의 거절은 같은 종료 코드를 낼 수 있다. 이 시험만으로 tokei 실패 처리 분기를 독립적으로 입증하지 못한다.
#[test]
fn a_tokei_that_fails_is_shadowed_by_the_parser() {
    let d = root_with(Some(BUDGET), &[P]);
    let (code, text) = run_bare(d.path(), None, Some("exit 3"), STRIP_OK);
    assert_eq!(
        code, 2,
        "tokei가 실패했는데 측정 실패로 보고하지 않았다:\n{text}"
    );
    let (code_empty, text_empty) = run_bare(
        d.path(),
        None,
        Some(r#"echo '{"Rust":{"reports":[]}}'"#),
        STRIP_OK,
    );
    assert_eq!(
        code_empty, 2,
        "빈 보고가 측정 실패로 처리되지 않았다. 파서의 거절과 tokei 실패 처리를 각각 확인한다:\n{text_empty}"
    );
}

/// 예산 미달만으로 실제 코드 감소를 단정하지 않도록 원인별 안내와 파일 내역을 검사한다.
/// 허용 목록 변경, 수집 오류, 측정 방식 보정도 구별해야 한다. 보정 근거는 complexity-gate 가이드의 계측용 사본과 측정값 보정 절을 따른다.
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
        "미달 안내에 원인을 단정하지 말라는 설명이 없다. 측정 오류 때문에 예산을 낮추지 않도록 확인한다:\n{text}"
    );
    for branch in ["ㄱ ", "ㄴ ", "ㄷ ", "ㄹ "] {
        // 들여쓴 줄의 첫 항목 글자를 확인해 본문의 단순 언급과 구별한다.
        // 장식 문자가 앞에 붙으면 놓치고, 다른 줄이 같은 글자로 시작하면 오인할 수 있다.
        assert!(
            text.lines()
                .any(|l| l.starts_with(' ') && l.trim_start().starts_with(branch)),
            "원인 {branch}를 설명하는 열거 항목을 찾지 못했다. 본문에 글자만 언급한 것은 인정하지 않는다:\n{text}"
        );
    }
    assert!(
        text.contains("check-file-size.sh"),
        "허용 목록 변경 시 파일 크기 게이트 결과도 확인하라는 안내가 없다:\n{text}"
    );
    assert!(
        text.contains("최소 재현"),
        "측정 방식 보정에 필요한 최소 재현 안내가 없다:\n{text}"
    );
    assert!(
        text.lines().any(|l| l.contains(P)),
        "미달 안내에 파일별 내역이 없다. 감소가 특정 오독 형태에만 생겼는지 비교할 수 없다:\n{text}"
    );
}

// doc 주석으로 생기는 tokei 측정 차이도 고정해 실제 코드 변화 없이 예산이 바뀌지 않게 한다.

#[test]
fn a_bias_that_matches_the_pin_passes() {
    let d = root_with_bias(Some(BUDGET), Some(3), &[P]);
    assert_eq!(
        run(d.path(), &reports_with(P, BUDGET, BUDGET + 3, 1)),
        0,
        "편향이 고정값과 같은데 통과가 아니다"
    );
}

#[test]
fn a_bias_that_grew_fails() {
    let d = root_with_bias(Some(BUDGET), Some(3), &[P]);
    assert_eq!(
        run(d.path(), &reports_with(P, BUDGET, BUDGET + 4, 1)),
        1,
        "측정 편향이 늘었는데 검사가 실패하지 않았다. 이를 코드 감소로 오인하면 예산을 잘못 낮출 수 있다."
    );
}

#[test]
fn a_bias_that_shrank_also_fails() {
    let d = root_with_bias(Some(BUDGET), Some(3), &[P]);
    assert_eq!(
        run(d.path(), &reports_with(P, BUDGET, BUDGET + 2, 1)),
        1,
        "측정 편향이 줄었는데 검사가 실패하지 않았다. 합이 허용 여유 안에 있어도 편향 변경은 확인해야 한다."
    );
}

#[test]
fn a_missing_bias_line_is_a_measurement_failure() {
    let d = root_with_bias(Some(BUDGET), None, &[P]);
    // 같은 종료 코드를 쓰는 다른 오류와 구별하도록 해당 진단도 확인한다.
    let (code, text) = run_bare(d.path(), None, Some(&reports(P, BUDGET)), STRIP_OK);
    assert_eq!(
        code, 2,
        "편향 선언이 없는데 측정 실패로 처리하지 않았다:\n{text}"
    );
    assert!(
        text.contains("'# doc-comment-bias: <수>' 줄이 없다"),
        "편향 선언 누락을 구별할 진단이 없다. 같은 종료 코드를 쓰는 다른 오류와 구별해야 한다:\n{text}"
    );
}

/// 예산은 측정 편향의 차이만큼만 보정해야 같은 변경의 실제 코드 증가를 숨기지 않는다.
/// 파일별 내역도 출력해 감소가 해당 오독 형태에 한정되는지 확인한다.
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
        "예산을 편향 차이 2만큼 보정하라는 안내가 없다. 전체 합에 맞추면 같은 변경의 실제 증가를 숨길 수 있다:\n{text}"
    );
    assert!(
        text.contains("# doc-comment-bias: 5"),
        "새 편향 값을 안내하지 않았다:\n{text}"
    );
    assert!(
        text.lines().any(|l| l.contains(P)),
        "오독 형태가 있는 파일의 내역이 없다. 감소한 파일과 비교할 수 없다:\n{text}"
    );
}

/// 최소 재현의 결과가 바뀌면 도구의 계산 방식부터 확인해야 한다. 편향 값만 갱신하고 진행하지 않도록 검사한다.
#[test]
fn the_form_probe_stands_before_the_sum() {
    let d = root_with_bias(Some(BUDGET), Some(0), &[P]);
    let (code, text) = run_bare(
        d.path(),
        None,
        Some(&reports_with(P, BUDGET, BUDGET, 2)),
        STRIP_OK,
    );
    assert_eq!(
        code, 1,
        "최소 재현의 측정 결과가 달라졌는데 검사가 실패하지 않았다:\n{text}"
    );
    assert!(
        text.contains("형태"),
        "측정 형태의 변화가 진단에 드러나지 않는다:\n{text}"
    );
}
