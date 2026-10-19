//! 생성 bash·zsh 스크립트의 셸 통합 확장점을 실제 셸로 실행해 계약을 확인한다.
use super::*;
use std::process::Command;

/// 확장 디렉터리 하나를 만든다. 실패하는 확장과 사용자 함수를 덮어쓰려는 확장을 함께 둔다.
fn extension_home() -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    let dir = home.path().join("shell-integrations");
    std::fs::create_dir_all(&dir).unwrap();
    let write = |name: &str, body: &str| std::fs::write(dir.join(name), body).unwrap();
    write("b.sh", "__EXT_ORDER=\"${__EXT_ORDER}b\"\nreturn 7\n");
    write("a.sh", "__EXT_ORDER=\"${__EXT_ORDER}a\"\nexport EXT_A=1\n");
    write(
        "c.sh",
        "__EXT_ORDER=\"${__EXT_ORDER}c\"\nif ! typeset -f codex > /dev/null; then\ncodex() { echo plugin; }\nfi\n",
    );
    write("ignored.txt", "__EXT_ORDER=\"${__EXT_ORDER}x\"\n");
    std::fs::create_dir_all(dir.join("dir.sh")).unwrap();
    home
}

/// 사용자 함수를 정의하고 직전 종료 코드를 23으로 만든 뒤 프롬프트 함수를 실행한다.
const SCENARIO: &str = r#"
codex() { echo user; }
(exit 23)
__tasty_osc133_precmd
echo
echo "order=$__EXT_ORDER a=$EXT_A codex=$(codex)"
(exit 23)
__tasty_osc133_precmd
echo
echo "order=$__EXT_ORDER"
"#;

fn run(shell: &str, args: &[&str], script: &str, home: &std::path::Path) -> Option<String> {
    let out = match Command::new(shell)
        .args(args)
        .arg(script)
        .env("TASTY_PARENT_HOME", home)
        .env("HOME", home)
        .env_remove("ZDOTDIR")
        .output()
    {
        Ok(out) => out,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            tracing::warn!("{shell} not installed; skipped");
            return None;
        }
        Err(error) => panic!("{shell}: {error}"),
    };
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        out.status.success(),
        "{shell} failed: {stdout}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    Some(stdout)
}

fn assert_contract(shell: &str, stdout: &str) {
    assert_eq!(
        stdout.matches("\x1b]133;D;23\x1b\\").count(),
        2,
        "{shell}: D must carry the previous status: {stdout:?}"
    );
    assert!(stdout.contains("\x1b]133;A\x1b\\"), "{shell}: {stdout:?}");
    assert!(
        stdout.contains("order=abc a=1 codex=user"),
        "{shell}: byte order, failing script, user function: {stdout:?}"
    );
    assert!(
        stdout.contains("order=abcabc"),
        "{shell}: every prompt: {stdout:?}"
    );
}

#[test]
fn bash_prompt_sources_every_extension_and_survives_a_failing_one() {
    let home = extension_home();
    // 사용자가 failglob을 켜도 확장 목록은 동작하고 옵션은 복원된다.
    let script = format!(
        "shopt -s failglob\n{BUILTIN_BASHRC_PRE}\n{SCENARIO}\nshopt -q failglob && shopt -q nullglob || echo glob-restored\n"
    );
    let Some(stdout) = run(
        "bash",
        &["--norc", "--noprofile", "-c"],
        &script,
        home.path(),
    ) else {
        return;
    };
    assert_contract("bash", &stdout);
    assert!(stdout.contains("glob-restored"), "{stdout:?}");
}

#[test]
fn zsh_prompt_sources_every_extension_and_survives_a_failing_one() {
    let home = extension_home();
    let script = format!("{BUILTIN_ZSHENV_BODY}\n{SCENARIO}");
    let Some(stdout) = run("zsh", &["-f", "-c"], &script, home.path()) else {
        return;
    };
    assert_contract("zsh", &stdout);
}

#[test]
fn prompt_without_extension_directory_keeps_the_status_marker() {
    let home = tempfile::tempdir().unwrap();
    let script = "(exit 23)\n__tasty_osc133_precmd\n";
    for (shell, args, body) in [
        (
            "bash",
            &["--norc", "--noprofile", "-c"][..],
            BUILTIN_BASHRC_PRE,
        ),
        ("zsh", &["-f", "-c"][..], BUILTIN_ZSHENV_BODY),
    ] {
        let Some(stdout) = run(shell, args, &format!("{body}\n{script}"), home.path()) else {
            continue;
        };
        assert!(
            stdout.contains("\x1b]133;D;23\x1b\\"),
            "{shell}: {stdout:?}"
        );
    }
}
