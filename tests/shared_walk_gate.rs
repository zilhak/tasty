//! 합성 트리에서 순회 횟수 검사의 범위 전달과 증가·감소·빈 수집 판정을 확인한다.
//! 범위만 검사할 때는 합성 입력의 수에 CAP를 맞춰 모든 수집 단계가 반영되면 통과하도록 한다.
//! 이와 별도로 CAP 비교 자체의 동작도 검사한다.

#![cfg(unix)]

use std::fs;
use std::path::Path;
use std::process::Command;

fn gate_src() -> String {
    format!(
        "{}/scripts/check-shared-walk-ratchet.sh",
        env!("CARGO_MANIFEST_DIR")
    )
}

/// git ls-files를 사용하는 게이트이므로 합성 트리도 Git 저장소로 초기화한다.
fn synth_root() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("임시 디렉토리");
    let root = dir.path();
    let here = env!("CARGO_MANIFEST_DIR");
    fs::create_dir_all(root.join("scripts/lib")).expect("scripts/lib");
    fs::copy(
        gate_src(),
        root.join("scripts/check-shared-walk-ratchet.sh"),
    )
    .expect("게이트 복사");
    fs::copy(
        format!("{here}/scripts/lib/judge-bin.sh"),
        root.join("scripts/lib/judge-bin.sh"),
    )
    .expect("판정기 찾기 공용 복사");
    write_file(root, "tests/zz_quiet.rs", "fn f() {}\n");
    write_file(root, "crates/zz/tests/zz_quiet.rs", "fn g() {}\n");
    git(root, &["init", "-q"]);
    git(root, &["add", "-A"]);
    dir
}

fn git(root: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .expect("git 실행");
    assert!(
        out.status.success(),
        "git {args:?} 실패: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn write_file(root: &Path, rel: &str, body: &str) {
    let f = root.join(rel);
    fs::create_dir_all(f.parent().expect("부모")).expect("디렉토리");
    fs::write(&f, body).expect("파일 쓰기");
}

/// 중첩 Cargo 빌드 대신 스텁을 사용한다. 문자열 속 순회 이름이 입력에 있어 단순 복사만 해서는 안 된다.
/// 스텁은 이 합성 입력에 필요한 문자열·줄 주석 제거만 수행하며 완전한 Rust 파서는 아니다.
fn install_stub_masker(root: &Path) -> std::path::PathBuf {
    let bin = root.join("stub-mask-source");
    fs::write(
        &bin,
        "#!/bin/sh\n\
         if [ \"$1\" = \"--check-fresh\" ]; then exit 0; fi\n\
         keep=0\n\
         if [ \"$1\" = \"--keep-comments\" ]; then keep=1; shift; fi\n\
         out=$1; shift; src=$1; shift\n\
         mkdir -p \"$out\" || exit 1\n\
         for d in \"$@\"; do\n\
         [ -d \"$src/$d\" ] || continue\n\
         cp -r \"$src/$d\" \"$out/$d\" || exit 1\n\
         done\n\
         # 줄 수는 보존한 채 문자열을 덮는다. 주석은 --keep-comments 일 때만 남긴다.\n\
         find \"$out\" -name '*.rs' -type f | while IFS= read -r f; do\n\
         if [ \"$keep\" = 1 ]; then sed -i 's/\"[^\"]*\"/\"\"/g' \"$f\";\n\
         else sed -i 's/\"[^\"]*\"/\"\"/g; s|//.*||' \"$f\"; fi\n\
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
        .arg(root.join("scripts/check-shared-walk-ratchet.sh"))
        .current_dir(root)
        .env("TASTY_MASK_SOURCE_BIN", &stub)
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

fn widen_and_cap(root: &Path, cap: usize) {
    let p = root.join("scripts/check-shared-walk-ratchet.sh");
    let text = fs::read_to_string(&p).expect("게이트 사본을 읽을 수 없다");
    let widened = text.replace(
        "SCAN_SPECS=('tests/*.rs' 'crates/*/tests/*.rs')",
        "SCAN_SPECS=('tests/*.rs' 'crates/*/tests/*.rs' 'extra/*.rs')",
    );
    assert_ne!(
        widened, text,
        "변경할 SCAN_SPECS 선언을 찾지 못했다. 게이트의 선언 형식을 확인한다."
    );
    fs::write(&p, widened).expect("게이트 사본 쓰기");
    set_cap(root, cap);
}

/// 합성 입력에 맞춘 CAP로 비교 동작을 확인한다. 실제 저장소의 CAP가 적절한지는 이 시험에서 판단하지 않는다.
fn set_cap(root: &Path, cap: usize) {
    let p = root.join("scripts/check-shared-walk-ratchet.sh");
    let text = fs::read_to_string(&p).expect("게이트 사본을 읽을 수 없다");
    let capped = text.replace("CAP=54", &format!("CAP={cap}"));
    assert_ne!(
        capped, text,
        "변경할 CAP=54 선언을 찾지 못했다. 합성 입력에 맞출 상한의 선언 형식을 확인한다."
    );
    fs::write(&p, capped).expect("게이트 사본 쓰기");
}

/// 이 입력은 추적 파일 수집만 검증한다. 마스킹 사본이 빠져도 원문으로 세는 대체 경로가 같은 수를 낼 수 있다.
#[test]
fn widening_the_left_side_moves_the_tracked_scan() {
    let d = synth_root();
    write_file(
        d.path(),
        "extra/zz_probe.rs",
        "fn f() {\n    let _ = std::fs::read_dir(\".\");\n}\n",
    );
    git(d.path(), &["add", "-A"]);
    widen_and_cap(d.path(), 1);
    let (code, text) = run(d.path());
    assert_eq!(
        code, 0,
        "추가 범위의 추적 타깃에서 순회를 수집하지 못했다:\n{text}"
    );
    assert!(
        text.contains("타깃 3개"),
        "출력된 타깃 수에 추가 범위가 반영되지 않았다:\n{text}"
    );
}

/// 문자열 안에 순회 이름을 두어 마스킹 여부에 따라 결과가 달라지게 한다. 단순 실제 호출로는 두 경로를 구별하지 못한다.
#[test]
fn widening_the_left_side_moves_the_masking() {
    let d = synth_root();
    write_file(
        d.path(),
        "extra/zz_probe.rs",
        "fn f() -> &'static str {\n    \"read_dir(\"\n}\n",
    );
    git(d.path(), &["add", "-A"]);
    widen_and_cap(d.path(), 0);
    let (code, text) = run(d.path());
    assert_eq!(
        code, 0,
        "추가 범위가 마스킹되지 않아 문자열 속 순회 이름까지 집계됐다:\n{text}"
    );
}

#[test]
fn widening_the_left_side_moves_the_untracked_check() {
    let d = synth_root();
    // 미추적 상태를 검사하므로 git add하지 않는다.
    write_file(
        d.path(),
        "extra/zz_probe.rs",
        "fn f() {\n    let _ = std::fs::read_dir(\".\");\n}\n",
    );
    widen_and_cap(d.path(), 0);
    let (code, text) = run(d.path());
    assert_eq!(
        code, 2,
        "추가 범위의 미추적 타깃이 검출되지 않았다. 이 파일이 빠진 값으로 판정하면 안 된다:\n{text}"
    );
    assert!(
        text.contains("extra/zz_probe.rs"),
        "무엇이 인덱스에 없는지를 안 찍는다:\n{text}"
    );
}

fn probe_with(root: &Path, n: usize) {
    let body: String = (0..n)
        .map(|i| format!("fn f{i}() {{\n    let _ = std::fs::read_dir(\".\");\n}}\n"))
        .collect();
    write_file(root, "tests/zz_probe.rs", &body);
    git(root, &["add", "-A"]);
}

#[test]
fn a_count_at_the_cap_passes() {
    let d = synth_root();
    probe_with(d.path(), 2);
    set_cap(d.path(), 2);
    let (code, text) = run(d.path());
    assert_eq!(code, 0, "값이 CAP와 같은데 검사가 실패했다:\n{text}");
}

#[test]
fn a_count_over_the_cap_is_a_violation() {
    let d = synth_root();
    probe_with(d.path(), 2);
    set_cap(d.path(), 1);
    let (code, text) = run(d.path());
    assert_eq!(code, 1, "상한을 넘었는데 위반이 아니다:\n{text}");
    assert!(
        text.contains("상한을 올려서 통과시키지 마라"),
        "상한을 올려서 통과시키지 말라는 안내가 없다:\n{text}"
    );
}

#[test]
fn a_count_under_the_cap_is_also_a_violation() {
    let d = synth_root();
    probe_with(d.path(), 2);
    set_cap(d.path(), 3);
    let (code, text) = run(d.path());
    assert_eq!(code, 1, "값이 CAP보다 작은데 검사가 통과했다:\n{text}");
    assert!(
        text.contains("CAP 을"),
        "상한을 내리라는 처방이 없다:\n{text}"
    );
}

#[test]
fn an_empty_left_side_is_undecidable() {
    let d = synth_root();
    for rel in ["tests/zz_quiet.rs", "crates/zz/tests/zz_quiet.rs"] {
        fs::remove_file(d.path().join(rel)).expect("프로브 제거");
    }
    git(d.path(), &["add", "-A"]);
    let (code, text) = run(d.path());
    assert_eq!(
        code, 2,
        "수집 대상이 비었는데 측정 실패로 처리하지 않았다:\n{text}"
    );
    assert!(
        text.contains("아무것도 안 봤다"),
        "빈 수집과 순회 호출이 없는 상태를 구별하는 진단이 없다:\n{text}"
    );
}
