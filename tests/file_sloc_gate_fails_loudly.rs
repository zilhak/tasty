//! 파일 SLOC 게이트의 통과(0), 위반(1), 측정 실패(2)를 스텁 도구로 구별한다.
//! 실제 판정기를 빌드하면 바깥 Cargo 시험과 빌드 잠금을 기다릴 수 있어 스텁을 쓴다.
//! Unix 전용 시험이며 Windows에서는 실행되지 않는다.

#![cfg(unix)]

mod gate_env;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

/// 판정기 플래그를 제거해야 첫 인자가 출력 디렉터리가 된다.
const EAT_FLAGS: &str = "while [ \"${1#--}\" != \"$1\" ]; do shift; done\n";

fn stub_dir(body: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("임시 디렉토리");
    let path = dir.path().join("tokei");
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("스텁 작성");
    let mut perm = fs::metadata(&path).expect("스텁 metadata").permissions();
    perm.set_mode(0o755);
    fs::set_permissions(&path, perm).expect("실행권한");
    dir
}

/// 사본을 만들지 않는 성공 스텁은 사본 수 대조를 통과하도록 0을 출력한다.
fn run_gate_with(body: &str) -> gate_env::GateRun {
    run_gate(body, "mkdir -p \"$1\"\necho 0\nexit 0")
}

fn run_gate(tokei_body: &str, strip_body: &str) -> gate_env::GateRun {
    let (code, output) = run_gate_full(tokei_body, strip_body);
    gate_env::GateRun { code, output }
}

/// 경고는 종료 코드에 반영되지 않아 출력도 반환한다.
fn run_gate_full(tokei_body: &str, strip_body: &str) -> (i32, String) {
    // 합성 스텁에는 대응 소스가 없어 신선도 조회에 판정 불가(3)를 반환한다.
    run_gate_raw(
        tokei_body,
        &format!("if [ \"$1\" = \"--check-fresh\" ]; then exit 3; fi\n{EAT_FLAGS}{strip_body}"),
    )
}

/// 신선도 확인 자체를 검사할 때는 그 응답까지 호출자가 지정한다.
fn run_gate_raw(tokei_body: &str, strip_script: &str) -> (i32, String) {
    let dir = stub_dir(tokei_body);
    let strip = dir.path().join("strip-cfg-test");
    fs::write(&strip, format!("#!/bin/sh\n{strip_script}\n")).expect("판정기 스텁 작성");
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
    let run = gate_env::GateRun::from_output(&out);
    (run.code, run.output)
}

const UNDER_THRESHOLD: &str =
    r#"echo '{"Rust":{"reports":[{"name":"src/zz_stub_small.rs","stats":{"code":10}}]}}'"#;

/// 허용 목록이나 제외 패턴에 걸리지 않는 위반 경로여야 한다.
const OVER_THRESHOLD: &str =
    r#"echo '{"Rust":{"reports":[{"name":"src/zz_stub_violation.rs","stats":{"code":9999}}]}}'"#;

#[test]
fn real_violations_still_exit_one() {
    assert_eq!(
        run_gate_with(OVER_THRESHOLD),
        1,
        "임계 초과 파일이 있으면 exit 1이어야 한다"
    );
}

#[test]
fn a_clean_tree_exits_zero() {
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
        "tokei 실행 실패는 측정 실패(exit 2)여야 한다"
    );
}

/// 종료 코드 단정에 실패할 때 게이트 stderr도 남는지 확인한다.
#[test]
fn a_code_only_assertion_carries_the_gate_stderr_into_its_failure_text() {
    const MARK: &str = "zz-tokei-died-here";
    let run = run_gate_with(&format!("echo {MARK} >&2\nexit 7"));
    assert_eq!(run, 2, "tokei 실행 실패가 측정 실패로 분류되지 않았다");
    let shown = format!("{run:?}");
    assert!(
        shown.contains(MARK),
        "종료 코드 단정의 실패 진단에 게이트 stderr가 없다:\n{shown}"
    );
}

#[test]
fn an_empty_report_is_not_a_pass() {
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
        "JSON 파싱 실패는 측정 실패(exit 2)여야 한다"
    );
}

#[test]
fn a_failing_stripper_is_not_a_pass() {
    assert_eq!(
        run_gate(UNDER_THRESHOLD, "echo boom >&2\nexit 3"),
        2,
        "test 전용 코드를 제외하는 도구의 실행 실패는 측정 실패(exit 2)여야 한다"
    );
}

const ALL_SKIPPED: &str =
    r#"echo '{"Rust":{"reports":[{"name":"src/tests/zz_stub_skipped.rs","stats":{"code":10}}]}}'"#;

#[test]
fn zero_judged_files_is_not_a_pass() {
    assert_eq!(
        run_gate_with(ALL_SKIPPED),
        2,
        "판정 대상이 0 건이면 측정 실패다 — 통과로 읽지 않는다"
    );
}

/// 신선도 확인의 stdout을 실행 파일 경로로 잘못 읽지 않는지 확인한다.
#[test]
fn a_judge_that_prints_while_answering_freshness_does_not_poison_the_path() {
    let (code, out) = run_gate_raw(
        UNDER_THRESHOLD,
        &format!(
            "if [ \"$1\" = \"--check-fresh\" ]; then echo POLLUTION; exit 3; fi\n\
             {EAT_FLAGS}mkdir -p \"$1\"\necho 0\nexit 0"
        ),
    );
    assert_eq!(
        code, 0,
        "신선도 조회의 stdout 때문에 판정기 경로가 잘못 선택됐다:\n{out}"
    );
}

#[test]
fn a_partially_successful_stripper_is_not_a_pass() {
    assert_eq!(
        run_gate(
            UNDER_THRESHOLD,
            "mkdir -p \"$1\"\ntouch \"$1/zz_one.rs\"\necho 5\nexit 0"
        ),
        2,
        "판정기가 만들었다는 수와 디스크의 사본 수가 어긋나면 측정 실패(exit 2)여야 한다 — \
         통과(0)도 위반(1)도 아니다"
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
    assert_eq!(gate_env::GateRun::from_output(&out), 2);
}

const IN_WARN_BAND: &str =
    r#"echo '{"Rust":{"reports":[{"name":"src/zz_stub_warn.rs","stats":{"code":950}}]}}'"#;

const BELOW_WARN_BAND: &str =
    r#"echo '{"Rust":{"reports":[{"name":"src/zz_stub_ok.rs","stats":{"code":899}}]}}'"#;

const VIOLATION_AND_WARNING: &str = r#"echo '{"Rust":{"reports":[{"name":"src/zz_stub_violation.rs","stats":{"code":9999}},{"name":"src/zz_stub_warn.rs","stats":{"code":950}}]}}'"#;

#[test]
fn nothing_in_the_band_prints_no_warning_section() {
    let (code, out) = run_gate_full(BELOW_WARN_BAND, "mkdir -p \"$1\"\necho 0\nexit 0");
    assert_eq!(code, 0, "899 는 임계 미만이라 통과여야 한다:\n{out}");
    assert!(
        out.contains("경고(900↑) 0"),
        "경고가 없어도 경고 수 0을 출력해야 한다: {out}"
    );
    assert!(
        !out.contains("경고 —"),
        "띠에 든 파일이 없으면 경고 목록 절을 내지 않는다: {out}"
    );
}

#[test]
fn the_warning_band_names_the_file_and_keeps_the_exit_code() {
    let (code, out) = run_gate_full(IN_WARN_BAND, "mkdir -p \"$1\"\necho 0\nexit 0");
    assert_eq!(
        code, 0,
        "경고는 rc 에 안 들어간다 — 띠 안이어도 통과다:\n{out}"
    );
    assert!(out.contains("경고(900↑) 1"), "경고 수를 찍어야 한다: {out}");
    assert!(
        out.contains("src/zz_stub_warn.rs"),
        "경고 대상 파일의 경로가 없다: {out}"
    );
    // 같은 문구의 부정 단정만 있으면 출력 자체가 사라져도 통과하므로 여기서 출력을 요구한다.
    assert!(
        out.contains("경고 —"),
        "띠에 든 파일이 있으면 경고 목록 절의 머리를 내야 한다: {out}"
    );
}

#[test]
fn a_violation_and_a_warning_stay_separate() {
    let (code, out) = run_gate_full(VIOLATION_AND_WARNING, "mkdir -p \"$1\"\necho 0\nexit 0");
    assert_eq!(code, 1, "임계 초과가 있으면 여전히 exit 1 이다:\n{out}");
    assert!(
        out.contains("임계초과 1") && out.contains("경고(900↑) 1"),
        "위반과 경고가 서로 다른 칸이어야 한다: {out}"
    );
}

// 측정 디렉터리를 늘렸을 때 사본 생성과 tokei 모두 같은 범위를 쓰는지 확인한다.
// 이 경우의 tokei 스텁은 고정 JSON 대신 실제 사본을 읽어 누락을 드러낸다.

fn scan_synth_root() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("임시 디렉토리");
    let root = dir.path();
    let here = env!("CARGO_MANIFEST_DIR");
    fs::create_dir_all(root.join("scripts/lib")).expect("scripts/lib");
    fs::copy(
        format!("{here}/scripts/check-file-size.sh"),
        root.join("scripts/check-file-size.sh"),
    )
    .expect("게이트 복사");
    fs::copy(
        format!("{here}/scripts/lib/judge-bin.sh"),
        root.join("scripts/lib/judge-bin.sh"),
    )
    .expect("판정기 찾기 공용 복사");
    fs::write(root.join(".complexity-file-allowlist"), "").expect("빈 allowlist");
    write_rs(root, "src/zz_small.rs", 10);
    write_rs(root, "crates/zz/src/zz_small.rs", 10);
    dir
}

/// 스텁은 빈 줄이 아닌 줄을 code로 센다. 이 입력에서는 n이 측정값이다.
fn write_rs(root: &Path, rel: &str, n: usize) {
    let f = root.join(rel);
    fs::create_dir_all(f.parent().expect("부모")).expect("디렉토리");
    let body: String = (0..n).map(|i| format!("fn f{i}() {{}}\n")).collect();
    fs::write(&f, body).expect("파일 쓰기");
}

/// 없는 디렉터리를 받으면 실패하는 스텁으로 사본 생성 범위 누락을 확인한다.
fn stub_tokei_reading_disk() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("스텁 디렉토리");
    let bin = dir.path().join("tokei");
    fs::write(
        &bin,
        "#!/bin/sh\n\
         # `tokei --output json <dirs...>` 흉내: 인자 아래 .rs 의 줄 수를 code 로 낸다.\n\
         shift 2\n\
         for d in \"$@\"; do [ -d \"$d\" ] || exit 1; done\n\
         files=$(for d in \"$@\"; do find \"$d\" -name '*.rs' -type f; done)\n\
         printf '{\"Rust\":{\"reports\":['\n\
         sep=\"\"\n\
         for f in $files; do\n\
         n=$(grep -c '[^[:space:]]' \"$f\" || true)\n\
         printf '%s{\"name\":\"%s\",\"stats\":{\"code\":%s}}' \"$sep\" \"$f\" \"$n\"\n\
         sep=\",\"\n\
         done\n\
         printf ']}}'\n",
    )
    .expect("스텁 tokei");
    make_executable(&bin);
    dir
}

/// 실제로 복사한 .rs 수를 반환해 게이트의 사본 수 대조를 통과한다.
fn stub_strip_copying() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("스텁 디렉토리");
    let bin = dir.path().join("strip-cfg-test");
    fs::write(
        &bin,
        "#!/bin/sh\n\
         if [ \"$1\" = \"--check-fresh\" ]; then exit 0; fi\n\
         while [ \"${1#--}\" != \"$1\" ]; do shift; done\n\
         out=$1; shift; src=$1; shift\n\
         mkdir -p \"$out\" || exit 1\n\
         for d in \"$@\"; do\n\
         [ -d \"$src/$d\" ] || continue\n\
         cp -r \"$src/$d\" \"$out/$d\" || exit 1\n\
         done\n\
         find \"$out\" -name '*.rs' -type f | wc -l | tr -d ' '\n",
    )
    .expect("스텁 판정기");
    make_executable(&bin);
    dir
}

fn make_executable(p: &Path) {
    let mut perm = fs::metadata(p).expect("권한 읽기").permissions();
    perm.set_mode(0o755);
    fs::set_permissions(p, perm).expect("실행권한");
}

fn widen_scan_dirs(root: &Path) {
    let p = root.join("scripts/check-file-size.sh");
    let text = fs::read_to_string(&p).expect("게이트 사본을 읽을 수 없다");
    let widened = text.replace("SCAN_DIRS=(src crates)", "SCAN_DIRS=(src crates extra)");
    assert_ne!(
        widened, text,
        "변경할 SCAN_DIRS=(src crates) 선언을 찾지 못했다. 게이트의 선언 형식을 확인한다."
    );
    fs::write(&p, widened).expect("게이트 사본 쓰기");
}

fn run_scan_gate(root: &Path) -> (i32, String) {
    run_scan_gate_with(root, &stub_strip_copying())
}

fn run_scan_gate_with(root: &Path, strip: &tempfile::TempDir) -> (i32, String) {
    let tokei = stub_tokei_reading_disk();
    let path = std::env::var("PATH").unwrap_or_default();
    let out = Command::new("bash")
        .arg(root.join("scripts/check-file-size.sh"))
        .current_dir(root)
        .env("PATH", format!("{}:{path}", tokei.path().display()))
        .env(
            "TASTY_STRIP_CFG_TEST_BIN",
            strip.path().join("strip-cfg-test"),
        )
        .output()
        .expect("게이트 실행");
    (
        out.status.code().unwrap_or(-1),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}

/// 새 경로의 위반을 검사한다. tokei가 경로를 빠뜨리면 통과하고 사본이 경로를 빠뜨리면 측정 실패가 된다.
#[test]
fn widening_the_left_side_moves_both_consumers() {
    let d = scan_synth_root();
    write_rs(d.path(), "extra/zz_big.rs", 1500);
    widen_scan_dirs(d.path());
    let (code, text) = run_scan_gate(d.path());
    assert_eq!(
        code, 1,
        "추가한 디렉터리의 임계 초과가 검출되지 않았다. 사본 생성과 tokei의 수집 범위를 확인한다:\n{text}"
    );
    assert!(
        text.contains("extra/zz_big.rs"),
        "위반의 좌표를 안 찍는다:\n{text}"
    );
}

/// 임계 이하 파일은 누락돼도 종료 코드가 달라지지 않아 출력된 판정 수의 증가를 확인한다.
#[test]
fn widening_the_left_side_moves_the_judged_count() {
    fn judged(text: &str) -> usize {
        let tail = text
            .split("게이트 판정 ")
            .nth(1)
            .unwrap_or_else(|| panic!("통과 메시지에서 판정 수를 읽지 못했다:\n{text}"));
        tail.split(' ')
            .next()
            .and_then(|n| n.trim().parse().ok())
            .unwrap_or_else(|| panic!("판정 수가 숫자가 아니다:\n{text}"))
    }

    let d = scan_synth_root();
    write_rs(d.path(), "extra/zz_small.rs", 10);
    let (code, before) = run_scan_gate(d.path());
    assert_eq!(code, 0, "범위를 늘리기 전 실행이 실패했다:\n{before}");

    widen_scan_dirs(d.path());
    let (code, after) = run_scan_gate(d.path());
    assert_eq!(code, 0, "범위를 늘린 뒤 실행이 실패했다:\n{after}");

    assert_eq!(
        judged(&after),
        judged(&before) + 1,
        "디렉터리를 추가했지만 판정 대상 수가 늘지 않았다. 임계 이하 파일도 수집하는지 확인한다.\n추가 전:\n{before}\n추가 후:\n{after}"
    );
}

/// 최댓값과 남은 여유는 종료 코드에 반영되지 않으므로 출력값을 직접 비교한다.
#[test]
fn the_reported_maximum_names_the_largest_judged_file() {
    let d = scan_synth_root();
    // 파일 SLOC 임계 1000보다 작은 입력으로 최댓값과 남은 여유를 확인한다.
    write_rs(d.path(), "extra/zz_tall.rs", 500);
    widen_scan_dirs(d.path());
    let (code, text) = run_scan_gate(d.path());
    assert_eq!(code, 0, "임계 이하 입력의 검사가 실패했다:\n{text}");
    assert!(
        text.contains("최대 500 (extra/zz_tall.rs)"),
        "출력한 최댓값이 가장 큰 판정 대상과 다르다:\n{text}"
    );
    assert!(
        text.contains("남은 여유 500"),
        "출력한 남은 여유가 임계와 최댓값의 차이와 다르다:\n{text}"
    );
}

#[test]
fn a_missing_tokei_is_not_a_pass() {
    // 도구가 없는 PATH를 만든다. 다른 실패와 구별하도록 종료 코드와 메시지를 함께 본다.
    let path = gate_env::only(&["bash", "dirname"]);
    let out = Command::new("bash")
        .arg(format!(
            "{}/scripts/check-file-size.sh",
            env!("CARGO_MANIFEST_DIR")
        ))
        .env("PATH", path.path())
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("게이트 실행");
    let text = gate_env::GateRun::from_output(&out).output;
    assert_eq!(
        out.status.code(),
        Some(2),
        "tokei 가 없으면 판정 불가여야 한다 — 0 이면 아무것도 안 재고 통과한 것이다:\n{text}"
    );
    assert!(text.contains("tokei 미설치"), "{text}");
}

#[test]
fn a_missing_python_is_not_a_pass() {
    let path = gate_env::only(&["bash", "dirname"]);
    let tokei = path.path().join("tokei");
    fs::write(&tokei, "#!/bin/sh\nexit 0\n").expect("tokei 스텁");
    let mut perm = fs::metadata(&tokei).expect("스텁 metadata").permissions();
    perm.set_mode(0o755);
    fs::set_permissions(&tokei, perm).expect("실행권한");

    let out = Command::new("bash")
        .arg(format!(
            "{}/scripts/check-file-size.sh",
            env!("CARGO_MANIFEST_DIR")
        ))
        .env("PATH", path.path())
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("게이트 실행");
    let text = gate_env::GateRun::from_output(&out).output;
    assert_eq!(
        out.status.code(),
        Some(2),
        "python 이 없으면 판정 불가여야 한다:\n{text}"
    );
    assert!(text.contains("python 미설치"), "{text}");
}

#[test]
fn a_nonnumeric_stripper_count_is_not_a_pass() {
    let (code, out) = run_gate_full(UNDER_THRESHOLD, "mkdir -p \"$1\"\necho abc\nexit 0");
    assert_eq!(
        code, 2,
        "판정기가 수를 안 냈는데 판정 불가가 아니다 — 그 수 없이는 사본이 모자란지 모른다:\n{out}"
    );
    assert!(out.contains("사본 수를 안 냈다"), "{out}");
}

#[test]
fn an_uncountable_copy_count_is_not_a_pass() {
    // 게이트가 사본 디렉터리를 미리 만들므로 디렉터리를 생략해도 이 오류에 도달하지 않는다. find 자체를 실패시킨다.
    let dir = tempfile::tempdir().expect("임시 디렉토리");
    for (name, body) in [
        ("tokei", format!("#!/bin/sh\n{UNDER_THRESHOLD}\n")),
        (
            "strip-cfg-test",
            format!(
                "#!/bin/sh\nif [ \"$1\" = \"--check-fresh\" ]; then exit 3; fi\n\
                 {EAT_FLAGS}mkdir -p \"$1\"\necho 0\nexit 0\n"
            ),
        ),
        ("find", "#!/bin/sh\nexit 1\n".to_string()),
    ] {
        let p = dir.path().join(name);
        fs::write(&p, body).expect("스텁 작성");
        let mut perm = fs::metadata(&p).expect("스텁 metadata").permissions();
        perm.set_mode(0o755);
        fs::set_permissions(&p, perm).expect("실행권한");
    }
    let path = format!(
        "{}:{}",
        dir.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let out = Command::new("bash")
        .arg(format!(
            "{}/scripts/check-file-size.sh",
            env!("CARGO_MANIFEST_DIR")
        ))
        .env("PATH", path)
        .env(
            "TASTY_STRIP_CFG_TEST_BIN",
            dir.path().join("strip-cfg-test"),
        )
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("게이트 실행");
    let text = gate_env::GateRun::from_output(&out).output;
    assert_eq!(
        out.status.code(),
        Some(2),
        "사본 수를 못 셌는데 판정 불가가 아니다:\n{text}"
    );
    assert!(text.contains("사본 수를 셀 수 없다"), "{text}");
}

/// 파일 제외를 요청하는 플래그의 전달 여부만 확인한다. 실제 선언 파서 대신 표식으로 test 전용 파일을 정한다.
fn stub_strip_honoring_blank_flag() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("스텁 디렉토리");
    let bin = dir.path().join("strip-cfg-test");
    fs::write(
        &bin,
        "#!/bin/sh\n\
         if [ \"$1\" = \"--check-fresh\" ]; then exit 0; fi\n\
         blank=0\n\
         while [ \"${1#--}\" != \"$1\" ]; do\n\
         [ \"$1\" = \"--blank-test-only-files\" ] && blank=1\n\
         shift\n\
         done\n\
         out=$1; shift; src=$1; shift\n\
         mkdir -p \"$out\" || exit 1\n\
         for d in \"$@\"; do\n\
         [ -d \"$src/$d\" ] || continue\n\
         cp -r \"$src/$d\" \"$out/$d\" || exit 1\n\
         done\n\
         if [ \"$blank\" = 1 ]; then\n\
         for f in $(grep -rl TEST_ONLY_SENTINEL \"$out\" 2>/dev/null); do\n\
         awk '{ print \"\" }' \"$f\" > \"$f.blanked\" && mv \"$f.blanked\" \"$f\"\n\
         done\n\
         fi\n\
         find \"$out\" -name '*.rs' -type f | wc -l | tr -d ' '\n",
    )
    .expect("스텁 판정기");
    make_executable(&bin);
    dir
}

fn write_test_only_rs(root: &Path, rel: &str, n: usize) {
    let f = root.join(rel);
    fs::create_dir_all(f.parent().expect("부모")).expect("디렉토리");
    let mut body = String::from("// TEST_ONLY_SENTINEL\n");
    for i in 1..n {
        body.push_str(&format!("fn f{i}() {{}}\n"));
    }
    fs::write(&f, body).expect("파일 쓰기");
}

fn drop_whole_file_erasure(root: &Path) {
    let p = root.join("scripts/check-file-size.sh");
    let text = fs::read_to_string(&p).expect("게이트 사본을 읽을 수 없다");
    let narrowed = text.replace(
        "SHIPPING_JUDGE_FLAGS=(--neutralize-char-literal-quotes --blank-test-only-files)",
        "SHIPPING_JUDGE_FLAGS=(--neutralize-char-literal-quotes)",
    );
    assert_ne!(
        narrowed, text,
        "변경할 SHIPPING_JUDGE_FLAGS 선언을 찾지 못했다. 게이트의 플래그 선언 형식을 확인한다."
    );
    fs::write(&p, narrowed).expect("게이트 사본 쓰기");
}

#[test]
fn a_test_only_file_over_the_threshold_is_erased_not_measured() {
    let d = scan_synth_root();
    write_test_only_rs(d.path(), "src/zz_declared_test_only.rs", 1500);

    let (code, text) = run_scan_gate_with(d.path(), &stub_strip_honoring_blank_flag());
    assert_eq!(
        code, 0,
        "test 전용 파일을 제품 코드의 SLOC에 포함했다:\n{text}"
    );

    drop_whole_file_erasure(d.path());
    let (code, text) = run_scan_gate_with(d.path(), &stub_strip_honoring_blank_flag());
    assert_eq!(
        code, 1,
        "파일 제외 플래그를 뺐는데도 위반이 검출되지 않았다. 스텁이 플래그에 따라 동작하는지 확인한다:\n{text}"
    );
    assert!(
        text.contains("src/zz_declared_test_only.rs"),
        "위반의 좌표를 안 찍는다:\n{text}"
    );
}
