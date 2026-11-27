//! 릴리스 노트 생성(`scripts/release-notes.sh`)을 셸 시험(`scripts/tests/release-notes.sh`)으로 실행한다.
//! 셸 시험은 일회용 저장소에 커밋을 만들고 스크립트 출력을 기대 노트와 글자 단위로 비교한다.
//! Bash와 Git이 필요하다. Windows에서는 Git과 함께 설치된 Bash를 쓴다.

use std::path::{Path, PathBuf};
use std::process::Command;
use tasty_doc_guards::repo_root;

/// Windows의 시스템 bash.exe는 배포판 없는 WSL 실행기일 수 있어 Git의 Bash를 쓴다.
fn bash() -> PathBuf {
    if cfg!(windows) {
        let shell = Command::new("git")
            .args(["var", "GIT_SHELL_PATH"])
            .output()
            .expect("git var GIT_SHELL_PATH");
        assert!(shell.status.success(), "git var GIT_SHELL_PATH failed");
        Path::new(String::from_utf8_lossy(&shell.stdout).trim())
            .parent()
            .expect("GIT_SHELL_PATH has a directory")
            .join("bash.exe")
    } else {
        PathBuf::from("bash")
    }
}

/// 임시 저장소를 만드므로 저장소 지정 Git 변수를 지운다. Windows에서는 Git Bash의 도구 폴더를 PATH 앞에 둔다.
fn bash_command() -> Command {
    let bash = bash();
    #[cfg(windows)]
    {
        let dir = bash.parent().expect("bash has a directory").to_path_buf();
        let rest = std::env::var_os("PATH").unwrap_or_default();
        let path = std::env::join_paths(std::iter::once(dir).chain(std::env::split_paths(&rest)))
            .expect("PATH entries join");
        let mut command = tasty_doc_guards::git_env::command(bash);
        command.env("PATH", path);
        command
    }
    #[cfg(not(windows))]
    tasty_doc_guards::git_env::command(bash)
}

fn shell_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[test]
fn release_notes_drop_trailers_and_escape_html_outside_code() {
    let scratch = tasty_doc_guards::temp_scratch::Scratch::new("release-notes");
    let output = bash_command()
        .arg(shell_path(
            &repo_root().join("scripts/tests/release-notes.sh"),
        ))
        .arg(shell_path(&repo_root()))
        .arg(shell_path(scratch.path()))
        .output()
        .expect("run release notes shell test");
    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
