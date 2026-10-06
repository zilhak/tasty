//! reduce 의 custom 셸 환경. 바깥 신원 변수를 둔 프로세스에서 실제 셸을 실행해 확인한다.
//! 시험 프로세스의 환경을 바꾸지 않도록 테스트 바이너리를 helper 로 다시 실행한다.

use std::path::Path;

const DIR_ENV: &str = "TASTY_REDUCE_ENV_TEST_DIR";

/// helper: 이 프로세스의 환경을 상속하는 custom 셸로 `env` 를 파일에 쓴다. 환경변수가 없으면
/// 아무것도 하지 않는다.
#[test]
fn custom_shell_env_helper() {
    let Some(dir) = std::env::var_os(DIR_ENV) else {
        return;
    };
    let out = Path::new(&dir).join("env.txt");
    let stdout = crate::run_custom_shell("env", "{}").expect("custom shell");
    std::fs::write(out, stdout).expect("write env dump");
}

#[test]
fn the_reduce_custom_shell_does_not_inherit_the_outer_session_identities() {
    let td = tempfile::tempdir().expect("tempdir");
    let status = std::process::Command::new(std::env::current_exe().expect("exe"))
        .args([
            "--exact",
            "child_env::tests::custom_shell_env_helper",
            "--test-threads=1",
        ])
        .env(DIR_ENV, td.path())
        // 바깥 Tasty 인스턴스·Claude Code 세션이 남긴 값.
        .env("TASTY_SESSION_TOKEN", "outer-token")
        .env("TASTY_SURFACE_ID", "777")
        .env("TASTY_PARENT_HOME", "/tmp/outer-home")
        .env("TASTY_AGENT_ID", "outer-agent")
        .env("CLAUDECODE", "1")
        .env("CLAUDE_CODE_SESSION_ID", "outer-session")
        // 그대로 넘어가야 하는 값.
        .env("TASTY_LOCALE", "ko")
        .env("ANTHROPIC_API_KEY", "user-key")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("helper");
    assert!(status.success(), "helper failed: {status:?}");
    let env = std::fs::read_to_string(td.path().join("env.txt")).expect("env dump");
    let names: Vec<&str> = env
        .lines()
        .filter_map(|l| l.split_once('=').map(|(k, _)| k))
        .collect();
    for gone in [
        "TASTY_SESSION_TOKEN",
        "TASTY_SURFACE_ID",
        "TASTY_PARENT_HOME",
        "TASTY_AGENT_ID",
        "CLAUDECODE",
        "CLAUDE_CODE_SESSION_ID",
    ] {
        assert!(
            !names.contains(&gone),
            "{gone} reached the custom shell:\n{env}"
        );
    }
    for kept in ["TASTY_LOCALE=ko", "ANTHROPIC_API_KEY=user-key"] {
        assert!(env.lines().any(|l| l == kept), "{kept} missing:\n{env}");
    }
}
