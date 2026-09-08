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
//! **자동 채널**: 통합 테스트라 헤드리스 잡(전체 스위트)이 본다.
//!
//! **Windows 에서 이 타깃은 빈다 — 커버리지 0 이고, 그 0 은 이제 미측정이 아니라 잰
//! 값이다.** `#![cfg(unix)]` 라 그렇고, 그 cfg 가 그 타깃에서 실제로 거짓인 것을 재서
//! 안다: `rustc --print cfg --target x86_64-pc-windows-gnu` 에 `unix` 선언이 **없다**
//! (호스트에는 있다). 컴파일은 거기서도 통과한다 — `cargo clippy -p tasty --all-targets
//! --locked --target x86_64-pc-windows-gnu` rc=0, 이 파일 진단 0 (실측 2026-09-08).
//! 즉 **Windows 잡의 초록은 이 게이트가 거기서 돈다는 뜻이 아니다.** 한때 이 자리에
//! "Windows 잡은 `--lib --bins` 라 안 본다" 고 적혀 있었는데, 그 잡은 `--all-targets`
//! 로 이 타깃을 **컴파일한다** — 안 보는 것은 잡이 아니라 `cfg` 다.

#![cfg(unix)]

mod gate_env;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
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
///
/// 스텁이 `echo 0` 을 찍는 이유: 게이트가 **판정기가 만들었다는 사본 수**와 디스크의
/// 사본 수를 맞춰 본다. 사본을 하나도 안 만드는 이 스텁은 0 을 말해야 그 대조를 통과한다.
/// 수를 아예 안 내면 게이트는 판정 불가로 나가고, 그러면 아래 시험들이 재려는 것이
/// 아니라 **대조 실패**를 재게 된다.
fn run_gate_with(body: &str) -> i32 {
    run_gate(body, "mkdir -p \"$1\"\necho 0\nexit 0")
}

/// tokei 스텁과 판정기 스텁을 주입하고 게이트를 돌려 **종료코드만** 얻는다.
fn run_gate(tokei_body: &str, strip_body: &str) -> i32 {
    run_gate_full(tokei_body, strip_body).0
}

/// 종료코드와 표준출력을 함께 얻는다.
///
/// 경고 띠(`WARN_BAND`)는 **rc 에 안 들어가므로 종료코드로는 관측이 안 된다** — 띠가
/// 조용히 사라져도 위 테스트들은 전부 초록이다. 그래서 그 칸만 출력을 본다. 단정은
/// 안정된 조각(`경고(900↑)` 와 경로)만 걸고 문장 전체를 걸지 않는다.
fn run_gate_full(tokei_body: &str, strip_body: &str) -> (i32, String) {
    // 신선도 질문에는 3(= 소스가 없는 트리라 물을 수 없다)으로 답한다. 그것이 이
    // 상황의 참값이다 — 합성 스텁에는 판정기 소스가 없다. 각 시험이 그 갈래를 매번
    // 다시 쓰지 않도록 여기서 붙인다.
    run_gate_raw(
        tokei_body,
        &format!("if [ \"$1\" = \"--check-fresh\" ]; then exit 3; fi\n{strip_body}"),
    )
}

/// 판정기 스텁을 **손대지 않고** 그대로 심는다 — 신선도 갈래까지 시험이 정한다.
///
/// 위 헬퍼는 그 갈래를 대신 써 주므로, 그 갈래 자체가 판정 대상인 시험은 여기를 쓴다.
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
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
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

/// 판정기 갈래의 **세 번째**: rc 0 으로 성공하면서 사본을 모자라게 만드는 것.
///
/// 위 둘(죽는다 · 없다)은 rc 와 경로로 갈리지만 이것은 rc 에 흔적을 안 남긴다.
/// 실측(2026-09-07): 진짜 판정기를 부른 뒤 사본의 절반을 지우는 스텁을 물리자 판정
/// 대상이 1117 에서 601 로 반토막 났는데 rc 는 0 이었다 — 게이트를 한 글자도 안
/// 고치므로 skip 목록을 텍스트로 읽는 대리 판정도 안 걸리고, allowlist 파일이 남아
/// 있으면 자매 게이트도 안 움직인다. 그 상태의 "임계 초과 0" 은 "큰 파일이 없다" 와
/// "큰 파일을 안 봤다" 를 같은 줄로 만든다.
///
/// 스텁은 사본 하나만 만들고 다섯을 만들었다고 말한다. 위반이 아니라 **판정 불가**를
/// 요구하는 것이 요점이다 — tokei 스텁은 임계 이하만 보고하므로 여기서 1 이 나오면
/// 그건 이 갈래가 위반으로 붕괴한 것이다.
/// 판정기가 **신선도 갈래에서** 무언가 찍어도 게이트가 그것을 경로로 삼지 않는다.
///
/// 공용 판정기 찾기(`scripts/lib/judge-bin.sh`)는 한때 경로를 표준출력으로 반환했다.
/// 그러면 그 함수의 **모든 갈래**가 "아무것도 안 찍는다" 는 계약을 지는데, 그 계약이
/// 어디에도 안 박혀 있었다. 실측(2026-09-07): 신선도 갈래에서 한 줄을 찍는 스텁을
/// 물리자 경로가 "POLLUTION\n/tmp/…/strip-cfg-test" 가 되고 게이트가 **그 줄을 명령으로
/// 실행했다** — 이 파일의 극성 다섯이 죽었다. 진짜 판정기는 그 갈래에서 아무것도 안
/// 찍으므로 그날까지 조용했고, 게이트가 판정기의 표준출력을 값으로 읽기 시작한
/// 순간 실효화됐다.
///
/// 그래서 "안 찍는다" 를 지키는 대신 **찍혀도 안 터지게** 바꿨다(반환이 변수다).
/// 이 시험은 그 성질을 건다 — 스텁이 신선도에서 한 줄 찍어도 판정은 평소와 같다.
#[test]
fn a_judge_that_prints_while_answering_freshness_does_not_poison_the_path() {
    let (code, _out) = run_gate_raw(
        UNDER_THRESHOLD,
        "if [ \"$1\" = \"--check-fresh\" ]; then echo POLLUTION; exit 3; fi\n\
         mkdir -p \"$1\"\necho 0\nexit 0",
    );
    assert_eq!(
        code, 0,
        "신선도 갈래의 출력이 판정기 경로를 오염시켰다 — 반환이 표준출력으로 되돌아갔다"
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
    assert_eq!(out.status.code(), Some(2));
}

// ── 경고 띠 (900↑) ────────────────────────────────────────────────────────
//
// 임계만 있으면 게이트는 아무 말도 안 하다가 갑자기 막는다. 실측(2026-09-07)에서
// 임계 이하 최댓값 999 뒤에 997·997·995 가 붙어 있었고 900 대가 아홉이었는데,
// 통과할 때 수를 안 찍어 아무도 몰랐다. 띠는 그 상태를 값으로 낸다.

/// 띠 안(950) 하나. 임계 미만이라 위반이 아니다.
const IN_WARN_BAND: &str =
    r#"echo '{"Rust":{"reports":[{"name":"src/zz_stub_warn.rs","stats":{"code":950}}]}}'"#;

/// 띠 바로 아래(899). 대조군용.
const BELOW_WARN_BAND: &str =
    r#"echo '{"Rust":{"reports":[{"name":"src/zz_stub_ok.rs","stats":{"code":899}}]}}'"#;

/// 위반과 경고가 한 트리에 함께 있는 경우 — 둘이 갈리는지 본다.
const VIOLATION_AND_WARNING: &str = r#"echo '{"Rust":{"reports":[{"name":"src/zz_stub_violation.rs","stats":{"code":9999}},{"name":"src/zz_stub_warn.rs","stats":{"code":950}}]}}'"#;

#[test]
fn nothing_in_the_band_prints_no_warning_section() {
    // ⓪ 대조군. 이게 없으면 아래 두 테스트는 "경고 절을 항상 찍는 게이트" 로도 통과한다.
    let (code, out) = run_gate_full(BELOW_WARN_BAND, "mkdir -p \"$1\"\necho 0\nexit 0");
    assert_eq!(code, 0, "899 는 임계 미만이라 통과여야 한다");
    assert!(
        out.contains("경고(900↑) 0"),
        "경고 0 을 값으로 찍어야 한다 — 침묵은 '띠가 없다' 와 구분이 안 된다: {out}"
    );
    assert!(
        !out.contains("경고 —"),
        "띠에 든 파일이 없으면 경고 목록 절을 내지 않는다: {out}"
    );
}

#[test]
fn the_warning_band_names_the_file_and_keeps_the_exit_code() {
    let (code, out) = run_gate_full(IN_WARN_BAND, "mkdir -p \"$1\"\necho 0\nexit 0");
    assert_eq!(code, 0, "경고는 rc 에 안 들어간다 — 띠 안이어도 통과다");
    assert!(out.contains("경고(900↑) 1"), "경고 수를 찍어야 한다: {out}");
    assert!(
        out.contains("src/zz_stub_warn.rs"),
        "경고는 **이름으로** 나와야 한다 — 수만 나오면 어느 파일인지 아무도 모른다: {out}"
    );
    // 위 대조군(`nothing_in_the_band_prints_no_warning_section`)이 이 리터럴을 **부정**으로
    // 건다. 부정 단언은 문구가 바뀌어도 안 빨개지므로 — 절이 영영 안 나와도 초록이다 —
    // 여기서 같은 리터럴을 긍정으로 걸어 그 조용함을 이 시끄러움에 묶는다.
    // `tests/negative_output_assertions_are_paired.rs` 가 그 짝지음을 강제한다.
    assert!(
        out.contains("경고 —"),
        "띠에 든 파일이 있으면 경고 목록 절의 머리를 내야 한다: {out}"
    );
}

#[test]
fn a_violation_and_a_warning_stay_separate() {
    // 같은 트리에 둘이 있으면 서로 다른 칸으로 세어야 한다. 한 칸으로 뭉치면
    // "임계초과 2" 가 되어 위반 수가 부풀고, 그 수를 보고 lane 이 잘못 움직인다.
    let (code, out) = run_gate_full(VIOLATION_AND_WARNING, "mkdir -p \"$1\"\necho 0\nexit 0");
    assert_eq!(code, 1, "임계 초과가 있으면 여전히 exit 1 이다");
    assert!(
        out.contains("임계초과 1") && out.contains("경고(900↑) 1"),
        "위반과 경고가 서로 다른 칸이어야 한다: {out}"
    );
}

// ── 좌변이 갈리면 죽는다 ─────────────────────────────────────────────────
//
// 이 게이트는 같은 트리를 **두 번** 지목한다: 출하 사본을 뜰 때(판정기)와 그 사본을
// 잴 때(tokei). 둘이 따로 적혀 있던 동안 한쪽만 손대면 나머지가 안 따라갔고, 그
// 어긋남의 두 방향이 서로 다르게 조용했다 — 판정기만 좁으면 tokei 가 없는 디렉토리를
// 열다 죽어 시끄럽지만(2), **tokei 만 좁으면 위반 0 · rc=0 으로 조용하다.**
//
// **판정 방법**: 위 시험들과 달리 진짜 레포를 안 본다. 합성 루트에 게이트를 깔고
// 좌변 값을 `extra/` 만큼 늘린 뒤, 소비처가 따라오는지를 종료 코드로 묻는다.
// "게이트에 `SCAN_DIRS` 가 나온다" 를 세는 문자열 확인은 **철자를 보는 것**이지 두
// 소비처가 같은 값을 낸다는 것을 보는 게 아니다 — 변수를 선언해 놓고 한쪽이 옛
// 문자열을 그대로 쓰는 상태(= 고치기 전의 모양)를 그대로 통과시킨다.
//
// ★ 스텁 `tokei` 가 **디스크를 실제로 읽는다.** 위 시험들의 스텁은 고정 JSON 을
// 찍는데, 그러면 판정기가 사본을 어디까지 떴는지가 값에 안 나타나 이 물음에 답할 수
// 없다. 그래서 이 절의 스텁만 인자로 받은 디렉토리 아래의 `.rs` 를 세고, 없는
// 디렉토리에는 진짜 tokei 처럼 비영으로 죽는다 — 그 갈래가 판정기 좌변을 관측하는
// 유일한 축이다.

/// 합성 루트에 게이트와 그것이 source 하는 공용 판정기 찾기를 깐다.
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
    // 좌변이 안 비게 두 뿌리를 하나씩 실재시킨다. 둘 다 임계 이하다.
    write_rs(root, "src/zz_small.rs", 10);
    write_rs(root, "crates/zz/src/zz_small.rs", 10);
    dir
}

/// `n` 줄짜리 `.rs` 를 만든다. 스텁 tokei 가 줄 수를 그대로 code 로 읽는다.
fn write_rs(root: &Path, rel: &str, n: usize) {
    let f = root.join(rel);
    fs::create_dir_all(f.parent().expect("부모")).expect("디렉토리");
    let body: String = (0..n).map(|i| format!("fn f{i}() {{}}\n")).collect();
    fs::write(&f, body).expect("파일 쓰기");
}

/// 디스크를 실제로 읽는 스텁 `tokei` 를 PATH 앞에 깔 디렉토리를 만든다.
///
/// 없는 디렉토리에 **비영으로 죽는다** — 진짜 tokei 와 같은 갈래이고, 판정기 좌변이
/// 안 따라왔을 때 그 사실이 rc 에 나타나는 유일한 통로다.
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
         n=$(wc -l < \"$f\" | tr -d ' ')\n\
         printf '%s{\"name\":\"%s\",\"stats\":{\"code\":%s}}' \"$sep\" \"$f\" \"$n\"\n\
         sep=\",\"\n\
         done\n\
         printf ']}}'\n",
    )
    .expect("스텁 tokei");
    make_executable(&bin);
    dir
}

/// 인자로 받은 디렉토리를 사본으로 옮기고 옮긴 `.rs` 수를 찍는 스텁 판정기.
///
/// 게이트가 **판정기가 말한 수**와 디스크의 사본 수를 맞춰 보므로 그 수를 정직하게 낸다.
fn stub_strip_copying() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("스텁 디렉토리");
    let bin = dir.path().join("strip-cfg-test");
    fs::write(
        &bin,
        "#!/bin/sh\n\
         if [ \"$1\" = \"--check-fresh\" ]; then exit 0; fi\n\
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

/// 게이트 사본의 좌변을 `extra` 만큼 늘린다. 치환 실패는 그 자리에서 죽는다 —
/// 좌변이 다시 여럿으로 흩어졌다는 뜻이라 그것도 잡아야 할 회귀다.
fn widen_scan_dirs(root: &Path) {
    let p = root.join("scripts/check-file-size.sh");
    let text = fs::read_to_string(&p).expect("게이트 사본을 읽을 수 없다");
    let widened = text.replace("SCAN_DIRS=(src crates)", "SCAN_DIRS=(src crates extra)");
    assert_ne!(
        widened, text,
        "게이트에 `SCAN_DIRS=(src crates)` 한 줄이 없다 — 좌변이 다시 여럿으로 \
         흩어졌거나 이름이 바뀌었다. 이 시험은 그 한 값을 늘려 소비처를 재므로 멈춘다."
    );
    fs::write(&p, widened).expect("게이트 사본 쓰기");
}

fn run_scan_gate(root: &Path) -> (i32, String) {
    let tokei = stub_tokei_reading_disk();
    let strip = stub_strip_copying();
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

/// 좌변을 늘리면 **둘 다** 따라와 새 영토의 임계 초과가 위반으로 나온다.
///
/// 죽이는 변이 둘이 서로 다른 코드로 갈린다:
///   tokei 좌변만 옛 문자열   → 사본은 다 떴는데 안 재서 **rc=0**(조용한 방향).
///   판정기 좌변만 옛 문자열  → 사본에 없는 디렉토리를 tokei 가 열다 죽어 **rc=2**.
#[test]
fn widening_the_left_side_moves_both_consumers() {
    let d = scan_synth_root();
    write_rs(d.path(), "extra/zz_big.rs", 1500);
    widen_scan_dirs(d.path());
    let (code, text) = run_scan_gate(d.path());
    assert_eq!(
        code, 1,
        "좌변을 늘렸는데 새 영토의 임계 초과가 위반으로 안 나온다 — 판정기와 tokei 중 \
         하나가 안 따라왔다:\n{text}"
    );
    assert!(
        text.contains("extra/zz_big.rs"),
        "위반의 좌표를 안 찍는다:\n{text}"
    );
}

/// ★ R1056 축 — **판정 대상 수는 rc 에 안 들어간다.**
///
/// 위 시험은 임계를 넘는 파일로 재므로 rc 가 갈린다. 그런데 좌변이 갈리는 흔한 모양은
/// 그게 아니다: 새 영토에 임계 **이하** 파일만 있으면 tokei 좌변이 안 따라와도 위반은
/// 여전히 0 이고 rc 는 0 이다. 바뀌는 것은 `판정 N 개` 한 수뿐인데 그 수는 판정에
/// 안 들어간다(0 일 때만 판정 불가로 갈린다). 그래서 그 자리는 **rc 로는 관측이 안
/// 되고**, 이 시험이 없으면 tokei 좌변을 되돌리는 변이가 안 죽는다.
///
/// 절대값을 외우지 않는다 — 늘리기 전후의 **차**를 본다. 늘린 것이 파일 하나이므로 1 이다.
#[test]
fn widening_the_left_side_moves_the_judged_count() {
    fn judged(text: &str) -> usize {
        let tail = text
            .split("게이트 판정 ")
            .nth(1)
            .unwrap_or_else(|| panic!("초록 문구에서 판정 수를 못 읽었다:\n{text}"));
        tail.split(' ')
            .next()
            .and_then(|n| n.trim().parse().ok())
            .unwrap_or_else(|| panic!("판정 수가 숫자가 아니다:\n{text}"))
    }

    let d = scan_synth_root();
    write_rs(d.path(), "extra/zz_small.rs", 10);
    let (code, before) = run_scan_gate(d.path());
    assert_eq!(code, 0, "늘리기 전인데 초록이 아니다:\n{before}");

    widen_scan_dirs(d.path());
    let (code, after) = run_scan_gate(d.path());
    assert_eq!(code, 0, "좌변을 늘렸더니 초록이 아니다:\n{after}");

    assert_eq!(
        judged(&after),
        judged(&before) + 1,
        "좌변을 늘렸는데 판정 대상 수가 안 늘었다 — 새 영토를 안 보고도 초록이 같은 수를 \
         찍는다. 임계 초과가 없는 동안 이 수가 그 좌변을 관측하는 유일한 자리다.\
         \n늘리기 전:\n{before}\n늘린 뒤:\n{after}"
    );
}

/// ★ R1056 축 — **최댓값과 남은 여유도 rc 에 안 들어간다.**
///
/// 위 `judged` 축과 같은 성질이지만 다른 수다. 게이트 본문은 이 값을 "여유가 0 인지
/// 900 인지가 통과/실패에 안 나타난다" 는 이유로 찍는다 — 실제로 임계 이하 최댓값이
/// 999 인데 아무도 그것을 모르던 자리를 실측으로 밟고 넣은 값이다.
///
/// 실측(2026-09-08): `${max_code}` 를 상수 `0` 으로 바꾸는 변이에서 위 열다섯이 전부
/// 통과했다. 그 수가 경고 띠의 근거이자 이 게이트의 유일한 조기 신호인데, 그것이
/// 맞는 파일을 가리키는지는 아무도 안 봤다.
///
/// 절대값을 외우지 않는다 — 프로브 파일의 줄 수를 그대로 요구한다(스텁 tokei 가
/// 줄 수를 code 로 읽는다). 최댓값은 임계 이하 중 가장 큰 것이므로 그 파일이다.
#[test]
fn the_reported_maximum_names_the_largest_judged_file() {
    let d = scan_synth_root();
    // 임계(1000) 이하에서 가장 크다. 바닥 파일 둘은 10 줄이라 이것이 최댓값이다.
    write_rs(d.path(), "extra/zz_tall.rs", 500);
    widen_scan_dirs(d.path());
    let (code, text) = run_scan_gate(d.path());
    assert_eq!(code, 0, "임계 이하인데 초록이 아니다:\n{text}");
    assert!(
        text.contains("최대 500 (extra/zz_tall.rs)"),
        "최댓값이 가장 큰 판정 대상을 안 가리킨다 — 이 수가 이 게이트의 유일한 조기 \
         신호이고, 여유가 0 인지 900 인지는 rc 에 안 나타난다:\n{text}"
    );
    assert!(
        text.contains("남은 여유 500"),
        "남은 여유가 최댓값에서 안 나온다 — 두 수가 갈리면 어느 쪽을 믿을지가 없다:\n{text}"
    );
}

// ── 전제 도구가 없을 때: 판정 불가여야 한다 ────────────────────────────────
//
// 이 게이트의 다른 "측정이 안 됐다" 갈래는 전부 시험이 rc=2 로 박아 뒀는데, **도구
// 부재 두 갈래만 안 박혀 있었다.** 실측 2026-09-08: 그 두 `exit 2` 를 `exit 0` 으로
// 바꾸고 이 계열 네 타깃(47 시험)을 돌리면 **한 건도 안 죽는다.** 즉 그 자리가 조용한
// 통과로 격하돼도 아무도 안 빨개진다 — `gates_pin_their_judge_absence.rs` 의 논거가
// 그대로 여기 적용된다(격하 갈래는 어디서도 안 돈다).
//
// 도구의 부재는 환경변수로 못 만든다. `command -v` 는 **자리**를 보므로 PATH 를 새로
// 짓는다(`tests/gate_env`).

#[test]
fn a_missing_tokei_is_not_a_pass() {
    // `bash` 와 `dirname` 만 보이는 PATH — 게이트가 자기 루트를 찾는 데까지는 가고
    // tokei 에서 멈춘다. rc 만 보면 "다른 이유로 죽었다" 와 안 갈리므로 메시지도 본다.
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
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert_eq!(
        out.status.code(),
        Some(2),
        "tokei 가 없으면 판정 불가여야 한다 — 0 이면 아무것도 안 재고 통과한 것이다:\n{text}"
    );
    assert!(text.contains("tokei 미설치"), "{text}");
}

#[test]
fn a_missing_python_is_not_a_pass() {
    // tokei 는 있고 python 만 없는 자리. tokei 스텁을 그 PATH 안에 직접 쓴다.
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
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert_eq!(
        out.status.code(),
        Some(2),
        "python 이 없으면 판정 불가여야 한다:\n{text}"
    );
    assert!(text.contains("python 미설치"), "{text}");
}

// ── 판정기가 사본 수를 안 낼 때 ────────────────────────────────────────────
//
// 위 두 갈래와 같은 성질이다. 판정기가 **숫자가 아닌 것**을 내거나 사본 디렉토리를
// 아예 안 만들면 게이트는 사본이 모자란지 알 수 없다. 형제 갈래(사본 수 **불일치**)는
// `a_partially_successful_stripper_is_not_a_pass` 가 박아 뒀는데 이 둘은 비어 있었다.

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
    // 사본 디렉토리를 안 만드는 것으로는 이 갈래에 **못 닿는다** — 게이트가 그 디렉토리를
    // 자기가 미리 만들어서 `find` 가 0 을 내고, 그러면 형제 갈래(수 **불일치**)로 간다.
    // 실측으로 밟았다. 세는 수단 자체가 죽어야 이 갈래다.
    let dir = tempfile::tempdir().expect("임시 디렉토리");
    for (name, body) in [
        ("tokei", format!("#!/bin/sh\n{UNDER_THRESHOLD}\n")),
        ("strip-cfg-test", "#!/bin/sh\nif [ \"$1\" = \"--check-fresh\" ]; then exit 3; fi\nmkdir -p \"$1\"\necho 0\nexit 0\n".to_string()),
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
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert_eq!(
        out.status.code(),
        Some(2),
        "사본 수를 못 셌는데 판정 불가가 아니다:\n{text}"
    );
    assert!(text.contains("사본 수를 셀 수 없다"), "{text}");
}
