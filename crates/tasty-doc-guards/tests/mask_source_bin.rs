//! mask-source 바이너리의 기본 모드와 --keep-comments 출력이 구분되는지 확인한다.
//! 마스킹 알고리즘은 source_text 단위 테스트에서 검사한다.

// 이유: 테스트의 값 무시를 제품 코드의 lint 목록에서 제외한다.
#![allow(clippy::let_underscore_must_use)]

use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_mask-source");

/// 의존성 없이 임시 디렉터리를 만들고 정리한다.
struct Tmp(PathBuf);
impl Tmp {
    fn new(tag: &str) -> Self {
        let d = std::env::temp_dir().join(format!("tasty-masksrc-{}-{tag}", std::process::id()));
        // 이전 실행의 임시 파일을 정리한다. 삭제 실패는 무시한다.
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("임시 디렉토리");
        Self(d)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for Tmp {
    fn drop(&mut self) {
        // 정리 실패가 원래 테스트 실패를 가리지 않게 한다.
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// 셸 검사가 합성 입력을 실제 위반으로 세지 않도록 문자열을 조립한다.
fn fixture(root: &Path) {
    let src = root.join("crates/demo/src");
    std::fs::create_dir_all(&src).expect("픽스처 디렉토리");
    let discard = format!("let _ {} drop_me();", "=");
    std::fs::write(
        src.join("lib.rs"),
        format!(
            "pub fn ship() {{}}\n\
             const MSG: &str = \"{discard}\";\n\
             // 이유: 주석 한 줄\n"
        ),
    )
    .expect("픽스처 파일");
}

fn run(root: &Path, out: &Path, flag: Option<&str>) {
    let mut cmd = Command::new(BIN);
    if let Some(f) = flag {
        cmd.arg(f);
    }
    // 자식 stderr를 수집해 병렬 실행에서도 해당 테스트의 실패 메시지에 포함한다.
    let result = cmd
        .arg(out)
        .arg(root)
        .arg("crates")
        .output()
        .expect("mask-source 를 실행할 수 없다");
    assert!(
        result.status.success(),
        "종료코드 {:?}\nstderr:\n{}",
        result.status,
        String::from_utf8_lossy(&result.stderr)
    );
}

fn masked(flag: Option<&str>, tag: &str) -> String {
    let root = Tmp::new(&format!("{tag}-root"));
    let out = Tmp::new(&format!("{tag}-out"));
    fixture(root.path());
    run(root.path(), out.path(), flag);
    std::fs::read_to_string(out.path().join("crates/demo/src/lib.rs")).expect("사본")
}

#[test]
fn the_default_mode_hides_both_literals_and_comments() {
    let got = masked(None, "default");
    assert!(got.contains("pub fn ship()"), "코드가 사라졌다: {got:?}");
    assert!(
        !got.contains("drop_me"),
        "문자열 안의 코드가 마스킹되지 않았다: {got:?}"
    );
    assert!(!got.contains("이유"), "기본 모드에 주석이 남았다: {got:?}");
}

#[test]
fn keep_comments_hides_only_literals() {
    let got = masked(Some("--keep-comments"), "keep");
    assert!(got.contains("pub fn ship()"), "코드가 사라졌다: {got:?}");
    assert!(
        !got.contains("drop_me"),
        "문자열 안의 금지 형태가 남았다: {got:?}"
    );
    assert!(
        got.contains("이유"),
        "--keep-comments 모드에서 주석이 사라졌다: {got:?}"
    );
}

/// 원본 위치를 보고할 수 있도록 두 모드 모두 줄 수를 보존해야 한다.
#[test]
fn line_numbers_are_preserved_in_both_modes() {
    for (i, flag) in [None, Some("--keep-comments")].into_iter().enumerate() {
        let got = masked(flag, &format!("lines-{i}"));
        assert_eq!(
            got.lines().count(),
            3,
            "줄 수가 달라졌다 (flag={flag:?}) — 좌표가 어긋난다: {got:?}"
        );
    }
}

#[test]
fn an_empty_scan_is_a_failure_not_a_success() {
    let root = Tmp::new("empty-root");
    let out = Tmp::new("empty-out");
    std::fs::create_dir_all(root.path().join("crates")).expect("빈 스캔 루트");
    let result = Command::new(BIN)
        .arg(out.path())
        .arg(root.path())
        .arg("crates")
        .output()
        .expect("실행");
    assert_eq!(
        result.status.code(),
        Some(2),
        "파일이 없는 입력을 성공으로 처리했다\nstderr:\n{}",
        String::from_utf8_lossy(&result.stderr)
    );
}
