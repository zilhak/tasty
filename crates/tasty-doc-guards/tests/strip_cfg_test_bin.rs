//! `strip-cfg-test` 의 **전체-테스트 파일** 축.
//!
//! 인라인 `#[cfg(test)]` 축은 `cfg_predicate` 의 단위 테스트가 본다. 여기서 보는 것은
//! 그 위에 얹힌 축 하나다 — `#[cfg(test)] mod x;` 로만 선언된 **파일 전체**를 비우는
//! 옵션이 (가) 켜면 비우고 (나) 안 켜면 안 비우는가.
//!
//! **두 방향을 다 보는 이유**: 이 옵션의 소비자가 둘이고 물음이 다르다. 파일 SLOC
//! 게이트는 이 축을 스크립트의 `skip()` 으로 이미 처리하므로 기본값이 바뀌면 그쪽
//! 측정이 조용히 움직인다. 그래서 "켜면 된다" 만큼 **"안 켜면 그대로다"** 가 단언이다.
//!
//! `cfg_attr` 축도 여기서 본다. 술어 판정 자체는 `cfg_predicate` 의 단위 테스트가 보지만,
//! **바이너리가 그 판정을 실제로 부르는가**는 별개 사건이다 — 그 배선이 빠져 있던 것이
//! 이 게이트의 실회차 첫 발화를 거짓 양성으로 만들었다.

// 이유: 이 타깃은 전부 테스트다. 테스트의 `let _` 무시는 정책이 사유를 요구하지
// 않으므로 `clippy::let_underscore_must_use` 명부(프로덕션 전용)에 섞이면 안 된다
// — docs/dev-guide/error-handling.md.
#![allow(clippy::let_underscore_must_use)]

use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_strip-cfg-test");

/// 임시 디렉토리. **이 크레이트는 의존이 0 이다**(ADR-0138) — `tempfile` 을 dev-의존으로
/// 들이면 doc-guards 잡이 그만큼 더 컴파일한다. 같은 크레이트의 다른 통합 테스트가 쓰는
/// 형태를 그대로 쓴다: pid 로 다른 완주와 갈리고, 태그로 같은 완주 안에서 갈린다.
struct Tmp(PathBuf);
impl Tmp {
    fn new(tag: &str) -> Self {
        let d = std::env::temp_dir().join(format!("tasty-stripbin-{}-{tag}", std::process::id()));
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
        // 뒷정리다. 여기서 실패해도 테스트 판정은 이미 끝났고, panic 중에 unwrap 하면
        // 진짜 실패 원인을 이 뒷정리가 덮어쓴다.
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `lib.rs` 가 `helper` 는 그냥, `guard` 는 test 게이트로 선언한 최소 크레이트.
fn fixture(root: &Path) {
    let src = root.join("crates/demo/src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(
        src.join("lib.rs"),
        "mod helper;\n#[cfg(test)]\nmod guard;\n",
    )
    .unwrap();
    std::fs::write(
        src.join("helper.rs"),
        "pub fn ship() {}\npub fn also() {}\n",
    )
    .unwrap();
    std::fs::write(
        src.join("guard.rs"),
        "fn a() {}\nfn b() {}\nfn c() {}\nfn d() {}\n",
    )
    .unwrap();
    std::fs::write(src.join("attrs.rs"), attrs_fixture()).unwrap();
}

/// `cfg_attr` 두 극성이 한 파일에 있는 픽스처.
///
/// 억제 이름을 조립 인자로 빼는 이유는 `scripts/check-allow-reason.sh` 가 줄 단위
/// 렉서라 이 문자열을 실제 억제로 세기 때문이다 — 픽스처가 그 게이트의 모수에
/// 들어가면 안 된다.
fn attrs_fixture() -> String {
    let a = "allow";
    format!(
        "#![cfg_attr(test, {a}(clippy::x))]\n\
         pub fn ship_one() {{}}\n\
         #[cfg_attr(not(test), {a}(clippy::y))]\n\
         pub fn ship_two() {{}}\n"
    )
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
        .expect("strip-cfg-test 를 실행할 수 없다");
    assert!(status.success(), "종료코드 {status:?}");
}

#[test]
fn the_flag_blanks_a_file_that_is_declared_test_only() {
    let root = Tmp::new("blank-root");
    let out = Tmp::new("blank-out");
    fixture(root.path());
    run(root.path(), out.path(), Some("--blank-test-only-files"));

    let guard = std::fs::read_to_string(out.path().join("crates/demo/src/guard.rs")).unwrap();
    assert!(
        guard.trim().is_empty(),
        "전체-테스트 파일이 안 비워졌다: {guard:?}"
    );
    // 줄 수 보존 — 사본이 짧아지면 "지운 결과" 와 "안 읽힌 결과" 가 구분되지 않는다.
    let original = std::fs::read_to_string(root.path().join("crates/demo/src/guard.rs")).unwrap();
    assert_eq!(
        guard.split('\n').count(),
        original.split('\n').count(),
        "줄 수가 달라졌다"
    );
}

#[test]
fn without_the_flag_that_same_file_is_left_alone() {
    let root = Tmp::new("keep-root");
    let out = Tmp::new("keep-out");
    fixture(root.path());
    run(root.path(), out.path(), None);

    let guard = std::fs::read_to_string(out.path().join("crates/demo/src/guard.rs")).unwrap();
    assert!(
        guard.contains("fn a()"),
        "기본값이 전체-테스트 파일을 건드렸다 — 파일 SLOC 게이트의 측정이 움직인다: {guard:?}"
    );
}

#[test]
fn a_shipping_file_survives_either_way() {
    for (i, flag) in [None, Some("--blank-test-only-files")]
        .into_iter()
        .enumerate()
    {
        let root = Tmp::new(&format!("ship-root-{i}"));
        let out = Tmp::new(&format!("ship-out-{i}"));
        fixture(root.path());
        run(root.path(), out.path(), flag);
        let helper = std::fs::read_to_string(out.path().join("crates/demo/src/helper.rs")).unwrap();
        assert!(
            helper.contains("pub fn ship()"),
            "출하되는 파일이 지워졌다 (flag={flag:?}): {helper:?}"
        );
    }
}

/// `cfg_attr` 이 실제로 배선돼 있는가 — 그리고 **양극성**으로.
///
/// `test` 를 요구하는 속성만 출하 밖이다. `not(test)` 는 프로덕션 전용이라 남아야
/// 하고, 두 경우 모두 속성이 붙은 **항목은 출하된다** — 항목까지 지우면 출하 코드가
/// 조용히 판정 밖으로 나간다.
#[test]
fn a_cfg_attr_that_requires_test_is_stripped_and_its_opposite_is_not() {
    for (i, flag) in [None, Some("--blank-test-only-files")]
        .into_iter()
        .enumerate()
    {
        let root = Tmp::new(&format!("attr-root-{i}"));
        let out = Tmp::new(&format!("attr-out-{i}"));
        fixture(root.path());
        run(root.path(), out.path(), flag);
        let got = std::fs::read_to_string(out.path().join("crates/demo/src/attrs.rs")).unwrap();
        assert!(
            !got.contains("cfg_attr(test"),
            "`test` 를 요구하는 속성이 출하 사본에 남았다 (flag={flag:?}): {got:?}"
        );
        assert!(
            got.contains("cfg_attr(not(test)"),
            "프로덕션 전용 속성을 지웠다 — 출하 코드가 판정 밖으로 나간다 (flag={flag:?}): {got:?}"
        );
        assert!(
            got.contains("pub fn ship_one()") && got.contains("pub fn ship_two()"),
            "속성이 붙은 항목까지 지웠다 (flag={flag:?}): {got:?}"
        );
    }
}
