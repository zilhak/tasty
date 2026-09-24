//! 잘못된 CLI 인자가 서버 연결보다 먼저 거절되는지 확인한다.
//! 포트 파일 없는 임시 홈을 사용하고 번역되지 않는 옵션 이름·포트 파일명으로 오류를 비교한다.

#[path = "spawn_diag/mod.rs"]
mod spawn_diag;

use std::path::Path;
use std::process::{Command, Stdio};

const NO_INSTANCE_MARK: &str = "tasty.port";

fn run(home: &Path, args: &[&str]) -> (Option<i32>, String) {
    let out = Command::new(spawn_diag::instance_bin())
        .args(args)
        .env("TASTY_HOME", home)
        .env_remove("TASTY_SURFACE_ID")
        .env_remove("TASTY_SESSION_TOKEN")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .output()
        .expect("run tasty");
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// 같은 격리 환경의 정상 인자는 인스턴스 없음으로 실패해야 잘못된 인자의 거절 순서를 비교할 수 있다.
#[test]
fn valid_arguments_without_an_instance_still_end_in_no_instance() {
    let home = tempfile::tempdir().expect("tempdir");
    for args in [
        &["webhook", "register", "--sequence", "[]"][..],
        &["new", "workspace", "--cwd", "."][..],
    ] {
        let (code, stderr) = run(home.path(), args);
        assert!(stderr.contains(NO_INSTANCE_MARK), "{args:?}: {stderr}");
        assert_ne!(code, Some(0), "{args:?}");
    }
}

#[test]
fn a_broken_json_argument_is_reported_before_connecting() {
    let home = tempfile::tempdir().expect("tempdir");
    let (code, stderr) = run(
        home.path(),
        &["webhook", "register", "--sequence", "{broken"],
    );
    assert!(stderr.contains("--sequence"), "{stderr}");
    assert!(!stderr.contains(NO_INSTANCE_MARK), "{stderr}");
    assert_eq!(code, Some(1), "{stderr}");
}

#[test]
fn a_missing_cwd_is_reported_before_connecting() {
    let home = tempfile::tempdir().expect("tempdir");
    let missing = home.path().join("no-such-dir");
    let missing = missing.to_str().expect("utf-8 path");
    let (code, stderr) = run(home.path(), &["new", "workspace", "--cwd", missing]);
    assert!(stderr.contains("--cwd"), "{stderr}");
    assert!(!stderr.contains(NO_INSTANCE_MARK), "{stderr}");
    assert_eq!(code, Some(1), "{stderr}");
}
