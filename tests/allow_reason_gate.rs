//! 근거 없는 lint 억제를 세는 게이트를 합성 Git 저장소에서 실행한다.
//! SCAN_SPECS를 넓혔을 때 rg·Git·미추적 파일 검사·두 마스킹 결과가 같은 범위를 쓰는지 확인한다.
//! 억제 수뿐 아니라 출력한 수집 파일 수도 비교한다.
//!
//! 마스킹 결과가 없는 파일은 원문으로 세므로 단순한 숫자 비교로 누락을 찾지 못할 수 있다.
//! 문자열 안의 억제와 근거 표지를 입력에 넣어 원문·마스킹 결과가 달라지게 한다.
//! 수집 범위 검사와 별도로, 억제 수가 기록보다 크거나 작을 때 실패하고 같을 때 통과하는지도 검증한다.
//! Git 분기 시험은 PATH에 rg가 없다는 전제가 있다. rg 분기는 별도 스텁으로 실행한다.

#![cfg(unix)]

use std::fs;
use std::path::Path;
use std::process::Command;

fn gate_src() -> String {
    format!(
        "{}/scripts/check-allow-reason.sh",
        env!("CARGO_MANIFEST_DIR")
    )
}

/// Git 목록을 쓰는 분기를 실제로 실행할 수 있도록 빈 저장소를 초기화한다.
fn synth_root() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("임시 디렉토리");
    let root = dir.path();
    let here = env!("CARGO_MANIFEST_DIR");
    fs::create_dir_all(root.join("scripts/lib")).expect("scripts/lib");
    fs::copy(gate_src(), root.join("scripts/check-allow-reason.sh")).expect("게이트 복사");
    fs::copy(
        format!("{here}/scripts/lib/judge-bin.sh"),
        root.join("scripts/lib/judge-bin.sh"),
    )
    .expect("판정기 찾기 공용 복사");
    write_file(root, "src/zz_quiet.rs", "fn f() {}\n");
    write_file(root, "crates/zz/src/zz_quiet.rs", "fn g() {}\n");
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

/// 시험 안에서 Cargo를 다시 빌드하면 바깥 빌드와 잠금을 기다릴 수 있어 마스킹 스텁을 쓴다. 원문을 그대로 복사하지 않고 입력에 필요한 문자열·주석을 가린다.
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

/// 호스트의 rg 설치 여부와 무관하게 rg 분기를 실행하도록 스텁을 앞 PATH에 둔다.
fn install_stub_rg(root: &Path) -> std::path::PathBuf {
    let dir = root.join("stubbin");
    fs::create_dir_all(&dir).expect("스텁 디렉토리");
    let bin = dir.join("rg");
    fs::write(
        &bin,
        "#!/bin/sh\n\
         # `rg --files -g '*.rs' <dirs...>` 만 흉내낸다 — 게이트가 그 형태로만 부른다.\n\
         shift 3\n\
         for d in \"$@\"; do find \"$d\" -name '*.rs' -type f 2>/dev/null; done\n",
    )
    .expect("스텁 rg");
    let mut perm = fs::metadata(&bin).expect("스텁 권한 읽기").permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut perm, 0o755);
    fs::set_permissions(&bin, perm).expect("스텁 실행권한");
    dir
}

fn run_inner(root: &Path, with_rg: bool) -> (i32, String) {
    let stub = install_stub_masker(root);
    let mut cmd = Command::new("bash");
    cmd.arg(root.join("scripts/check-allow-reason.sh"))
        .current_dir(root)
        .env("TASTY_MASK_SOURCE_BIN", &stub);
    if with_rg {
        let dir = install_stub_rg(root);
        let path = std::env::var("PATH").unwrap_or_default();
        cmd.env("PATH", format!("{}:{path}", dir.display()));
    }
    let out = cmd.output().expect("게이트 실행");
    (
        out.status.code().unwrap_or(-1),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}

fn run(root: &Path) -> (i32, String) {
    run_inner(root, false)
}

/// 합성 저장소의 수집 범위에 extra를 더하고 기록값을 입력의 억제 수에 맞춘다.
fn widen_and_cap(root: &Path, cap: usize) {
    set_cap(root, cap);
    widen_only(root);
}

/// 수집 범위 확장 전후를 비교할 수 있도록 기록값만 먼저 맞춘다.
fn set_cap(root: &Path, cap: usize) {
    let p = root.join("scripts/check-allow-reason.sh");
    let text = fs::read_to_string(&p).expect("게이트 사본을 읽을 수 없다");
    let capped = text.replace("CAP=142", &format!("CAP={cap}"));
    assert_ne!(
        capped, text,
        "게이트에서 CAP=142을 찾지 못했다. 합성 입력에 맞춰 기록값을 바꾸려면 현재 상수 형식을 확인해야 한다."
    );
    fs::write(&p, capped).expect("게이트 사본 쓰기");
}

fn widen_only(root: &Path) {
    let p = root.join("scripts/check-allow-reason.sh");
    let text = fs::read_to_string(&p).expect("게이트 사본을 읽을 수 없다");
    let widened = text.replace(
        "SCAN_SPECS=('crates/*.rs' 'src/*.rs')",
        "SCAN_SPECS=('crates/*.rs' 'src/*.rs' 'extra/*.rs')",
    );
    assert_ne!(
        widened, text,
        "SCAN_SPECS 선언을 찾지 못했다. 수집 범위 확장을 시험할 수 있도록 현재 변수와 형식을 확인한다."
    );
    fs::write(&p, widened).expect("게이트 사본 쓰기");
}

const BARE: &str = "#[allow(dead_code)]\nfn probe() {}\n";

#[test]
fn widening_the_left_side_moves_the_git_branch() {
    let d = synth_root();
    write_file(d.path(), "extra/zz_probe.rs", BARE);
    git(d.path(), &["add", "-A"]);
    widen_and_cap(d.path(), 1);
    let (code, text) = run(d.path());
    assert_eq!(
        code, 0,
        "범위를 넓혔는데 Git 분기가 추가 디렉터리의 억제를 세지 못했다:\n{text}"
    );
    assert!(
        text.contains("좌변=git-ls-files"),
        "이 시험이 재려던 분기가 아니다:\n{text}"
    );
}

#[test]
fn widening_the_left_side_moves_the_rg_branch() {
    let d = synth_root();
    write_file(d.path(), "extra/zz_probe.rs", BARE);
    git(d.path(), &["add", "-A"]);
    widen_and_cap(d.path(), 1);
    let (code, text) = run_inner(d.path(), true);
    assert!(
        text.contains("좌변=rg"),
        "스텁 rg 를 깔았는데 rg 분기를 안 탔다 — 이 시험이 재려던 것이 아니다:\n{text}"
    );
    assert_eq!(
        code, 0,
        "범위를 넓혔는데 rg 분기가 추가 디렉터리의 억제를 세지 못했다:\n{text}"
    );
}

/// 문자열 안 억제는 마스킹하면 0개, 원문으로 세면 1개다. 추가한 디렉터리도 탐지용 사본에 들어가는지 확인한다.
#[test]
fn widening_the_left_side_moves_the_detection_copy() {
    let d = synth_root();
    write_file(
        d.path(),
        "extra/zz_probe.rs",
        "fn probe() -> &'static str {\n    \"#[allow(dead_code)]\"\n}\n",
    );
    git(d.path(), &["add", "-A"]);
    widen_and_cap(d.path(), 0);
    let (code, text) = run(d.path());
    assert_eq!(
        code, 0,
        "탐지용 사본이 추가 디렉터리를 포함하지 않아 문자열 안의 억제를 원문에서 센다:\n{text}"
    );
}

/// 문자열 안 근거 표지는 근거용 사본에서 제거돼야 한다. 억제 구문은 남기므로 이 입력은 근거용 마스킹의 적용 여부를 구별한다.
#[test]
fn widening_the_left_side_moves_the_reason_copy() {
    let d = synth_root();
    write_file(
        d.path(),
        "extra/zz_probe.rs",
        "#[cfg_attr(feature = \"이유: 문자열 안의 가짜 근거\", allow(dead_code))]\nfn probe() {}\n",
    );
    git(d.path(), &["add", "-A"]);
    widen_and_cap(d.path(), 1);
    let (code, text) = run(d.path());
    assert_eq!(
        code, 0,
        "근거용 사본이 추가 디렉터리를 포함하지 않아 문자열 안의 표지를 근거로 인정했다:\n{text}"
    );
}

/// 추가한 디렉터리의 미추적 파일도 판정 불가(rc 2)로 처리해야 한다.
#[test]
fn widening_the_left_side_moves_the_untracked_check() {
    let d = synth_root();
    write_file(d.path(), "extra/zz_probe.rs", BARE);
    widen_and_cap(d.path(), 0);
    let (code, text) = run(d.path());
    assert_eq!(
        code, 2,
        "추가 디렉터리의 미추적 파일이 있는데도 판정값을 냈다:\n{text}"
    );
    assert!(
        text.contains("extra/zz_probe.rs"),
        "무엇이 인덱스에 없는지를 안 찍는다:\n{text}"
    );
}

/// 억제가 없는 파일은 억제 수와 종료 코드를 바꾸지 않는다. 수집 수 출력이 1만큼 늘어나는지 별도로 확인한다.
#[test]
fn widening_the_left_side_moves_the_scanned_count() {
    fn scanned(text: &str) -> usize {
        let tail = text
            .split("훑은 .rs ")
            .nth(1)
            .unwrap_or_else(|| panic!("통과 출력에서 수집 파일 수를 읽지 못했다:\n{text}"));
        tail.split('개')
            .next()
            .and_then(|n| n.trim().parse().ok())
            .unwrap_or_else(|| panic!("훑은 수가 숫자가 아니다:\n{text}"))
    }

    let d = synth_root();
    // 억제 수는 바꾸지 않고 파일 수만 늘리는 입력이다.
    write_file(d.path(), "extra/zz_quiet.rs", "fn probe() {}\n");
    git(d.path(), &["add", "-A"]);

    set_cap(d.path(), 0);
    let (code, before) = run(d.path());
    assert_eq!(code, 0, "범위 확장 전 입력이 통과하지 못했다:\n{before}");

    widen_only(d.path());
    let (code, after) = run(d.path());
    assert_eq!(code, 0, "범위 확장 후 입력이 통과하지 못했다:\n{after}");

    assert_eq!(
        scanned(&after),
        scanned(&before) + 1,
        "억제 없는 파일을 추가했는데 수집 수 출력이 늘지 않았다.\n확장 전:\n{before}\n확장 후:\n{after}"
    );
}

// 기록값과 수집값의 관계별 종료 코드를 검사한다. 수집 범위 검사만으로는 성공·실패 판정의 변경을 찾을 수 없다.

fn bare_probe(root: &Path, n: usize) {
    let body: String = (0..n)
        .map(|i| format!("#[allow(dead_code)]\nfn probe{i}() {{}}\n"))
        .collect();
    write_file(root, "src/zz_probe.rs", &body);
    git(root, &["add", "-A"]);
}

#[test]
fn a_count_at_the_cap_passes() {
    let d = synth_root();
    bare_probe(d.path(), 2);
    set_cap(d.path(), 2);
    let (code, text) = run(d.path());
    assert_eq!(code, 0, "억제 수가 기록과 같은데 통과하지 못했다:\n{text}");
}

#[test]
fn a_count_over_the_cap_is_a_violation() {
    let d = synth_root();
    bare_probe(d.path(), 2);
    set_cap(d.path(), 1);
    let (code, text) = run(d.path());
    assert_eq!(code, 1, "상한을 넘었는데 위반이 아니다:\n{text}");
    assert!(
        text.contains("상한을 올려서 통과시키지 마라"),
        "상한을 올려 통과시키지 말라는 안내가 없다:\n{text}"
    );
}

#[test]
fn a_count_under_the_cap_is_also_a_violation() {
    let d = synth_root();
    bare_probe(d.path(), 2);
    set_cap(d.path(), 3);
    let (code, text) = run(d.path());
    assert_eq!(
        code, 1,
        "억제 수가 줄었는데 남은 상한을 낮추도록 실패하지 않았다:\n{text}"
    );
    assert!(
        text.contains("CAP 을"),
        "상한을 내리라는 처방이 없다:\n{text}"
    );
}

fn reasoned_probe(root: &Path, head: &str, n: usize) {
    let body: String = (0..n)
        .map(|i| format!("{head}\n#[allow(dead_code)]\nfn probe{i}() {{}}\n"))
        .collect();
    write_file(root, "src/zz_probe.rs", &body);
    git(root, &["add", "-A"]);
}

/// 빈 근거 표지는 억제를 정당화하지 못한다. 종료 코드뿐 아니라 실제 억제 수도 확인한다.
#[test]
fn a_bare_marker_with_nothing_after_it_is_not_a_reason() {
    let d = synth_root();
    reasoned_probe(d.path(), "// 이유:", 2);
    set_cap(d.path(), 2);
    let (code, text) = run(d.path());
    assert_eq!(
        code, 0,
        "마커만 있는 자리를 근거로 세고 있다 — 값이 상한 2 에 안 닿았다:\n{text}"
    );
    assert!(
        text.contains(": 2건"),
        "빈 마커 둘이 근거 없는 억제로 안 세어졌다:\n{text}"
    );
}

/// 내용 있는 근거는 허용해야 빈 표지 검사와 함께 판정 방향을 확인할 수 있다.
#[test]
fn a_marker_followed_by_text_is_a_reason() {
    let d = synth_root();
    reasoned_probe(d.path(), "// 이유: 생성 코드라 이름이 안 쓰인다.", 2);
    set_cap(d.path(), 0);
    let (code, text) = run(d.path());
    assert_eq!(code, 0, "근거가 붙었는데 위반으로 세고 있다:\n{text}");
    assert!(text.contains(": 0건"), "값이 0 이 아니다:\n{text}");
}

#[test]
fn a_marker_whose_text_starts_on_the_next_line_is_a_reason() {
    let d = synth_root();
    reasoned_probe(d.path(), "// 이유:\n//   생성 코드라 이름이 안 쓰인다.", 2);
    set_cap(d.path(), 0);
    let (code, text) = run(d.path());
    assert_eq!(code, 0, "표지 다음 줄의 근거를 읽지 못했다:\n{text}");
    assert!(text.contains(": 0건"), "값이 0 이 아니다:\n{text}");
}

/// SAFETY는 콜론이 붙은 표지로 인식한다. 일반 설명의 단어 언급을 근거로 세지 않는다.
#[test]
fn a_safety_without_a_colon_is_not_a_reason() {
    let d = synth_root();
    reasoned_probe(
        d.path(),
        "// 이 블록의 SAFETY 조건은 다른 곳에 적혀 있다",
        2,
    );
    set_cap(d.path(), 2);
    let (code, text) = run(d.path());
    assert_eq!(code, 0, "콜론 없는 SAFETY 언급을 근거로 세고 있다:\n{text}");
    assert!(text.contains(": 2건"), "값이 2 가 아니다:\n{text}");
}

#[test]
fn a_safety_with_a_colon_is_a_reason() {
    let d = synth_root();
    reasoned_probe(
        d.path(),
        "// SAFETY: 포인터는 같은 문장에서 mmap 된 것이라 살아 있다",
        2,
    );
    set_cap(d.path(), 0);
    let (code, text) = run(d.path());
    assert_eq!(code, 0, "SAFETY: 근거를 안 읽는다:\n{text}");
    assert!(text.contains(": 0건"), "값이 0 이 아니다:\n{text}");
}

#[test]
fn an_empty_left_side_is_undecidable() {
    let d = synth_root();
    for rel in ["src/zz_quiet.rs", "crates/zz/src/zz_quiet.rs"] {
        fs::remove_file(d.path().join(rel)).expect("프로브 제거");
    }
    git(d.path(), &["add", "-A"]);
    let (code, text) = run(d.path());
    assert_eq!(code, 2, "수집 범위가 비었는데 판정값을 냈다:\n{text}");
    assert!(
        text.contains("아무것도 안 봤다"),
        "빈 수집과 근거 없는 억제가 0개인 경우를 구별하지 못했다:\n{text}"
    );
}
