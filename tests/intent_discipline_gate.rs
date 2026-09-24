//! Intent 규칙 게이트의 통과(0)·위반(1)·측정 실패(2)를 합성 저장소에서 확인한다.
//! 실제 스크립트에서 읽은 예외 경로를 만들고 입력만 바꿔 실행한다.

#![cfg(unix)]

use std::fs;
use std::path::Path;
use std::process::Command;

const CALL: &str = "    app.popups.open(PopupId::Settings);";

fn gate_src() -> String {
    format!(
        "{}/scripts/check-intent-discipline.sh",
        env!("CARGO_MANIFEST_DIR")
    )
}

/// 예외 경로가 없으면 먼저 측정 실패가 나므로 다른 조건을 검사할 때는 모두 만들어 둔다.
fn exempt_paths() -> Vec<String> {
    let text = fs::read_to_string(gate_src()).expect("게이트를 읽을 수 없다");
    let mut out = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("EXEMPT_ALL=(") || t.starts_with("EXEMPT_PANE=(") {
            inside = true;
            continue;
        }
        if !inside {
            continue;
        }
        if t == ")" {
            inside = false;
            continue;
        }
        if let Some(p) = t.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
            out.push(p.to_string());
        }
    }
    assert!(
        !out.is_empty(),
        "게이트에서 면제 명부를 못 읽었다 — 배열 형식이 바뀌었다"
    );
    out
}

fn synth_root() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("임시 디렉토리");
    let root = dir.path();
    let here = env!("CARGO_MANIFEST_DIR");
    fs::create_dir_all(root.join("scripts/lib")).expect("scripts/lib");
    fs::copy(gate_src(), root.join("scripts/check-intent-discipline.sh")).expect("게이트 복사");
    fs::copy(
        format!("{here}/scripts/lib/judge-bin.sh"),
        root.join("scripts/lib/judge-bin.sh"),
    )
    .expect("판정기 찾기 공용 복사");
    for p in exempt_paths() {
        let f = root.join(&p);
        fs::create_dir_all(f.parent().expect("면제 경로의 부모")).expect("면제 경로 디렉토리");
        fs::write(&f, "// 합성 픽스처: 이 경로가 실재한다는 사실만 만든다.\n")
            .expect("면제 경로 파일");
    }
    dir
}

fn write_src(root: &Path, rel: &str, body: &str) {
    let f = root.join(rel);
    fs::create_dir_all(f.parent().expect("소스의 부모")).expect("소스 디렉토리");
    fs::write(&f, body).expect("합성 소스");
}

/// 중첩 Cargo 빌드가 바깥 시험의 잠금을 기다리지 않도록 복사만 하는 스텁을 쓴다.
/// 이 입력에서는 마스킹할 주석·문자열 내부의 호출이 없다. 그런 입력을 추가하면 스텁도 조정해야 한다.
fn install_stub_masker(root: &Path) -> std::path::PathBuf {
    let bin = root.join("stub-mask-source");
    fs::write(
        &bin,
        "#!/bin/sh\n\
         # --check-fresh <root> 는 신선하다고 답한다(스텁에는 낡을 소스가 없다).\n\
         if [ \"$1\" = \"--check-fresh\" ]; then exit 0; fi\n\
         if [ \"$1\" = \"--keep-comments\" ]; then shift; fi\n\
         out=$1; shift; src=$1; shift\n\
         mkdir -p \"$out\" || exit 1\n\
         for d in \"$@\"; do\n\
         [ -d \"$src/$d\" ] || continue\n\
         cp -r \"$src/$d\" \"$out/$d\" || exit 1\n\
         done\n\
         exit 0\n",
    )
    .expect("스텁 판정기");
    let mut perm = fs::metadata(&bin).expect("스텁 권한 읽기").permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut perm, 0o755);
    fs::set_permissions(&bin, perm).expect("스텁 실행권한");
    bin
}

fn run(root: &Path) -> (i32, String) {
    let stub = install_stub_masker(root);
    let out = Command::new("bash")
        .arg(root.join("scripts/check-intent-discipline.sh"))
        .current_dir(root)
        .env("TASTY_MASK_SOURCE_BIN", &stub)
        .output()
        .expect("게이트 실행");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.code().unwrap_or(-1), text)
}

#[test]
fn a_direct_domain_call_is_a_violation() {
    let d = synth_root();
    write_src(
        d.path(),
        "src/zz_direct_call.rs",
        &format!("fn f(app: &mut App) {{\n{CALL}\n}}\n"),
    );
    let (code, text) = run(d.path());
    assert_eq!(code, 1, "직접 호출인데 위반이 아니다:\n{text}");
    assert!(
        text.contains("src/zz_direct_call.rs"),
        "위반의 좌표를 안 찍는다 — 어디를 고쳐야 하는지가 안 나온다:\n{text}"
    );
}

/// 빈 트리 대신 같은 호출에 사유만 붙여 정상 예외가 통과하는지 확인한다.
#[test]
fn the_same_call_with_a_reason_passes() {
    let d = synth_root();
    write_src(
        d.path(),
        "src/zz_direct_call.rs",
        &format!(
            "fn f(app: &mut App) {{\n    // intent-exempt: 합성 픽스처의 대조군\n{CALL}\n}}\n"
        ),
    );
    let (code, text) = run(d.path());
    assert_eq!(code, 0, "사유가 달렸는데 막혔다:\n{text}");
}

#[test]
fn an_exempt_path_that_does_not_exist_is_undecidable() {
    let d = synth_root();
    write_src(d.path(), "src/zz_quiet.rs", "fn f() {}\n");
    let victim = exempt_paths().remove(0);
    fs::remove_file(d.path().join(&victim)).expect("면제 경로 삭제");
    let (code, text) = run(d.path());
    assert_eq!(
        code, 2,
        "예외 경로가 없는데 측정 실패로 처리하지 않았다:\n{text}"
    );
    assert!(
        text.contains(&victim),
        "누락된 예외 경로가 진단에 없다:\n{text}"
    );
}

/// 판정기가 없으면 원문을 대신 검사해 주석의 호출을 위반으로 오인할 수 있다.
/// 위반이 있는 입력으로 통과뿐 아니라 위반 코드와도 구별되는 측정 실패를 요구한다.
#[test]
fn a_missing_judge_is_undecidable_not_a_violation() {
    let d = synth_root();
    write_src(
        d.path(),
        "src/zz_direct_call.rs",
        &format!("fn f(app: &mut App) {{\n{CALL}\n}}\n"),
    );
    let out = Command::new("bash")
        .arg(d.path().join("scripts/check-intent-discipline.sh"))
        .current_dir(d.path())
        // 명시한 도구가 없을 때 기본 경로의 다른 도구를 대신 쓰지 않는지도 확인한다.
        .env("TASTY_MASK_SOURCE_BIN", "/nonexistent/mask-source")
        .output()
        .expect("게이트 실행");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        out.status.code().unwrap_or(-1),
        2,
        "판정기가 없는데 측정 실패로 처리하지 않았다. 원문을 대신 검사해 예외 추가를 요구하면 안 된다:\n{text}"
    );
    assert!(
        text.contains("mask-source"),
        "필요한 mask-source 도구가 진단에 없다:\n{text}"
    );
}

// 수집 범위를 늘린 뒤 직접 호출 검사와 예외 사유 검사에 모두 전달되는지 확인한다.
// 한쪽의 실패가 다른 쪽 누락을 가리지 않도록 사유 없는 호출과 잘못된 태그를 별도로 넣는다.

fn widen_scan_dirs(root: &Path) {
    let p = root.join("scripts/check-intent-discipline.sh");
    let text = fs::read_to_string(&p).expect("게이트 사본을 읽을 수 없다");
    let widened = text.replace("SCAN_DIRS=(src)", "SCAN_DIRS=(src extra)");
    assert_ne!(
        widened, text,
        "변경할 SCAN_DIRS=(src) 선언을 찾지 못했다. 게이트의 선언 형식을 확인한다."
    );
    fs::write(&p, widened).expect("게이트 사본 쓰기");
}

#[test]
fn widening_the_left_side_moves_the_scan() {
    let d = synth_root();
    write_src(d.path(), "src/zz_quiet.rs", "fn f() {}\n");
    write_src(
        d.path(),
        "extra/zz_probe.rs",
        &format!("fn f(app: &mut App) {{\n{CALL}\n}}\n"),
    );
    widen_scan_dirs(d.path());
    let (code, text) = run(d.path());
    assert_eq!(
        code, 1,
        "추가 범위의 직접 호출이 검출되지 않았다. 마스킹과 수집 인자를 확인한다:\n{text}"
    );
    assert!(
        text.contains("extra/zz_probe.rs"),
        "추가 범위의 경로가 진단에 없다:\n{text}"
    );
}

/// 사유 판독에도 추가 범위가 전달되는지 확인한다. [겨과사용]은 의도한 오타 입력이다.
#[test]
fn widening_the_left_side_moves_the_reason_check() {
    let d = synth_root();
    write_src(d.path(), "src/zz_quiet.rs", "fn f() {}\n");
    write_src(
        d.path(),
        "extra/zz_probe.rs",
        &format!(
            "fn f(app: &mut App) {{\n    // intent-exempt: [겨과사용] 오타 태그\n{CALL}\n}}\n"
        ),
    );
    widen_scan_dirs(d.path());
    let (code, text) = run(d.path());
    assert_eq!(
        code, 1,
        "추가 범위의 잘못된 사유 태그가 검출되지 않았다:\n{text}"
    );
    assert!(
        text.contains("모르는 사유 태그"),
        "종료 코드는 위반이지만 사유 태그 오류를 나타내지 않는다:\n{text}"
    );
    assert!(
        text.contains("extra/zz_probe.rs"),
        "추가 범위의 경로가 진단에 없다:\n{text}"
    );
}

/// 수집 개수는 종료 코드에 반영되지 않아 출력도 비교한다. 명부 크기 대신 파일 하나 추가 전후의 차이를 확인한다.
#[test]
fn widening_the_left_side_moves_the_reported_count() {
    fn scanned(text: &str) -> usize {
        let tail = text
            .split(".rs ")
            .nth(1)
            .unwrap_or_else(|| panic!("통과 메시지에서 수집 개수를 읽지 못했다:\n{text}"));
        tail.split('개')
            .next()
            .and_then(|n| n.trim().parse().ok())
            .unwrap_or_else(|| panic!("훑은 수가 숫자가 아니다:\n{text}"))
    }

    let d = synth_root();
    write_src(d.path(), "src/zz_quiet.rs", "fn f() {}\n");
    write_src(d.path(), "extra/zz_quiet.rs", "fn g() {}\n");

    let (code, before) = run(d.path());
    assert_eq!(code, 0, "범위를 늘리기 전 실행이 실패했다:\n{before}");

    widen_scan_dirs(d.path());
    let (code, after) = run(d.path());
    assert_eq!(code, 0, "범위를 늘린 뒤 실행이 실패했다:\n{after}");

    assert_eq!(
        scanned(&after),
        scanned(&before) + 1,
        "범위를 늘렸지만 출력의 수집 개수가 증가하지 않았다.\n추가 전:\n{before}\n추가 후:\n{after}"
    );
}
