//! `mask-source` 의 **두 물음**을 양극성으로 고정한다.
//!
//! 이 판정기의 존재 이유는 셸 게이트가 자기 렉서를 만들지 않게 하는 것이다. 그러니
//! 여기서 재는 것은 마스킹 알고리즘(그건 `source_text` 의 단위 테스트가 본다)이 아니라
//! **바이너리가 두 모드를 실제로 구분해 내보내는가**다 — 모드가 하나로 붙으면 소비자
//! 둘 중 하나는 자기 물음의 답을 잃는다.

// 이유: 이 타깃은 전부 테스트다. 테스트의 `let _` 무시는 정책이 사유를 요구하지
// 않으므로 `clippy::let_underscore_must_use` 명부(프로덕션 전용)에 섞이면 안 된다
// — docs/dev-guide/error-handling.md.
#![allow(clippy::let_underscore_must_use)]

use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_mask-source");

/// 임시 디렉토리. 이 크레이트는 의존이 0 이라(ADR-0138) `tempfile` 을 안 들인다 —
/// 같은 크레이트의 다른 통합 테스트와 같은 형태를 쓴다.
struct Tmp(PathBuf);
impl Tmp {
    fn new(tag: &str) -> Self {
        let d = std::env::temp_dir().join(format!("tasty-masksrc-{}-{tag}", std::process::id()));
        // 직전 완주의 잔해를 치운다. 없는 것이 정상이라 실패가 곧 정보가 아니다 —
        // 진짜로 못 지웠으면 바로 아래 create_dir_all 이 대신 말한다.
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
        // 뒷정리다. 여기서 unwrap 하면 진짜 실패 원인을 뒷정리가 덮어쓴다.
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// 코드 · 문자열 리터럴 · 주석이 각각 한 줄씩 있는 최소 파일.
///
/// 금지 형태를 **문자열 안에** 넣는 것이 이 픽스처의 요점이다 — 그것이 셸 게이트가
/// 실물로 세던 바로 그 자리다. 형태는 조립해서 만든다: 이 파일 자신이 그 게이트의
/// 모수에 들어가면 이 테스트가 재려는 문제를 자기가 일으킨다.
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
    let status = cmd
        .arg(out)
        .arg(root)
        .arg("crates")
        .status()
        .expect("mask-source 를 실행할 수 없다");
    assert!(status.success(), "종료코드 {status:?}");
}

fn masked(flag: Option<&str>, tag: &str) -> String {
    let root = Tmp::new(&format!("{tag}-root"));
    let out = Tmp::new(&format!("{tag}-out"));
    fixture(root.path());
    run(root.path(), out.path(), flag);
    std::fs::read_to_string(out.path().join("crates/demo/src/lib.rs")).expect("사본")
}

/// 기본 모드 — "코드에 X 가 있나" 를 묻는 게이트용. 문자열도 주석도 남지 않는다.
#[test]
fn the_default_mode_hides_both_literals_and_comments() {
    let got = masked(None, "default");
    assert!(got.contains("pub fn ship()"), "코드가 사라졌다: {got:?}");
    assert!(
        !got.contains("drop_me"),
        "문자열 안의 금지 형태가 남았다 — 게이트가 그것을 실물로 센다: {got:?}"
    );
    assert!(
        !got.contains("이유"),
        "주석이 남았다 — 기본 모드는 주석도 덮는다: {got:?}"
    );
}

/// `--keep-comments` — "사유 **주석**이 달려 있나" 를 함께 묻는 게이트용.
/// 주석까지 덮으면 그 물음의 답이 사라지므로 두 모드는 합칠 수 없다.
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
        "주석이 사라졌다 — 사유를 묻는 게이트가 답을 잃는다: {got:?}"
    );
}

/// 줄 번호가 보존돼야 셸이 보고하는 좌표를 원본으로 읽을 수 있다. 두 모드 모두.
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

/// 파일 0 개를 0 으로 돌려주면 게이트가 빈 모수를 재고 조용히 초록이 된다.
#[test]
fn an_empty_scan_is_a_failure_not_a_success() {
    let root = Tmp::new("empty-root");
    let out = Tmp::new("empty-out");
    std::fs::create_dir_all(root.path().join("crates")).expect("빈 스캔 루트");
    let status = Command::new(BIN)
        .arg(out.path())
        .arg(root.path())
        .arg("crates")
        .status()
        .expect("실행");
    assert_eq!(
        status.code(),
        Some(2),
        "빈 모수를 성공으로 냈다 — 게이트가 아무것도 안 세고 초록이 된다"
    );
}
