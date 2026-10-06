//! 백그라운드 자식 프로세스의 공통 설정. hide_console은 Windows에서
//! CREATE_NO_WINDOW를 적용하며 다른 OS에서는 아무 일도 하지 않는다.
//! 사용자 터미널 셸은 portable-pty의 ConPTY 경로로 생성하므로 별개다.

use std::ffi::OsString;
use std::process::Command;

/// Windows 콘솔 서브시스템 자식 프로세스가 새 콘솔 창을 띄우지 않도록
/// `CREATE_NO_WINDOW` 플래그를 적용한다. 비-Windows 에서는 아무 동작도 하지 않는다.
///
/// `Command` 를 mutable 로 받아 그대로 돌려주므로 빌더 체인 중간에 끼울 수 있다.
pub fn hide_console(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // winbase.h CREATE_NO_WINDOW. windows-sys 의존 회피용 직접 상수.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Tasty 를 띄운 프로세스에서 상속되지만 Tasty 가 띄우는 자식(터미널 셸·훅 명령)에
/// 넘기지 않는 환경변수의 접두사. `CMUX_` 는 자식에서 cmux CLI 가
/// 동작하지 않게 한다.
pub const STRIPPED_ENV_PREFIXES: &[&str] = &["CMUX_"];

/// Claude Code 가 자기 세션의 자식에 표지·비밀로 넣는 환경변수. 바깥 Claude 세션에서 Tasty 를
/// 띄웠을 때 이 값이 남으면 자식에서 띄운 Claude 가 자신을 자식 세션으로 판단해 transcript 를
/// 쓰지 않고, 세션 비밀(`CLAUDE_CODE_MESSAGING_TOKEN`)이 무관한 프로세스로 샌다.
///
/// 접두사(`CLAUDE_CODE_`)로 지우지 않는다. 그 이름공간의 대부분은 사용자가 직접 넣는 설정
/// (`CLAUDE_CODE_GIT_BASH_PATH`·`CLAUDE_CODE_OAUTH_TOKEN` 등)이다. 목록은 Claude Code 2.1.291
/// 기준이며 근거는 다음과 같다.
/// - Bash·훅 자식에 넣는 키: `CLAUDECODE`·`CLAUDE_CODE_SESSION_ID`·`CLAUDE_CODE_CHILD_SESSION`·
///   `CLAUDE_CODE_SESSION_ATTENDED`·`CLAUDE_PID`·`CLAUDE_EFFORT`, 셸 스냅숏 키
///   `CLAUDE_CODE_EXECPATH`·`CLAUDE_CODE_INVOKED_SKILLS`.
/// - 플러그인 훅·MCP 자식에 넣는 키: `CLAUDE_PLUGIN_DATA`.
/// - Claude 가 자기 프로세스에 두어 상속되는 키: `CLAUDE_CODE_ENTRYPOINT`·
///   `CLAUDE_CODE_BRIDGE_SESSION_ID`·`CLAUDE_CODE_MESSAGING_SOCKET`·`CLAUDE_CODE_MESSAGING_TOKEN`.
///   마지막 셋과 세션 키는 Claude 가 새 Claude 를 띄울 때 스스로 지우는 목록에도 있다.
///
/// `AI_AGENT`·`TRACEPARENT` 는 Claude 전용 이름이 아니어서 넣지 않는다.
pub const STRIPPED_ENV_NAMES: &[&str] = &[
    "CLAUDECODE",
    "CLAUDE_PID",
    "CLAUDE_EFFORT",
    "CLAUDE_PLUGIN_DATA",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDE_CODE_SESSION_ATTENDED",
    "CLAUDE_CODE_EXECPATH",
    "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDE_CODE_MESSAGING_SOCKET",
    "CLAUDE_CODE_MESSAGING_TOKEN",
    "CLAUDE_CODE_INVOKED_SKILLS",
    "CLAUDE_CODE_BRIDGE_SESSION_ID",
];

/// `key` 가 자식에게 넘기지 않는 상속 환경변수인지 판정한다. Tasty 가 넣는 TASTY_* 는 대상이 아니다.
pub fn is_stripped_inherited_env(key: &str) -> bool {
    STRIPPED_ENV_NAMES.contains(&key) || STRIPPED_ENV_PREFIXES.iter().any(|p| key.starts_with(p))
}

/// `inherited` 키 중 자식 환경에서 지울 키를 고른다. UTF-8 이 아닌 키는 목록과 같을 수 없어 건너뛴다.
pub fn env_keys_to_strip(inherited: impl IntoIterator<Item = OsString>) -> Vec<OsString> {
    inherited
        .into_iter()
        .filter(|k| k.to_str().is_some_and(is_stripped_inherited_env))
        .collect()
}

/// 현재 프로세스 환경에서 자식에게 넘기지 않을 키.
pub fn process_env_keys_to_strip() -> Vec<OsString> {
    env_keys_to_strip(std::env::vars_os().map(|(k, _)| k))
}

/// 현재 실행 파일의 디렉터리를 base PATH 앞에 붙인다. 패키징된 앱의 제한된 PATH에서도
/// 자식이 tasty를 찾도록 한다. 기존 항목은 유지하며 사용자 로그인 셸의 PATH를 읽지는 않는다.
/// 실행 파일·부모 경로를 얻거나 PATH를 조립하지 못하면 None이다.
pub fn path_prepending_self_dir(base: Option<OsString>) -> Option<OsString> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let mut entries = vec![dir.to_path_buf()];
    if let Some(base) = base.as_deref() {
        entries.extend(std::env::split_paths(base));
    }
    std::env::join_paths(entries).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// self-binary 디렉토리가 최소 PATH 맨 앞에 붙는지 검증한다 — 패키징된
    /// macOS `.app` 이 받는 `/usr/bin:/bin:...` 같은 최소 PATH 를 흉내낸다.
    #[test]
    fn prepends_self_binary_dir_to_minimal_path() {
        let exe = std::env::current_exe().expect("current_exe");
        let self_dir = exe.parent().expect("parent");
        // join_paths 로 만들어 구분자(`:`/`;`)를 크로스플랫폼으로 맞춘다.
        let base = std::env::join_paths([
            std::path::PathBuf::from("/usr/bin"),
            std::path::PathBuf::from("/bin"),
        ])
        .expect("join base");

        let result = path_prepending_self_dir(Some(base)).expect("path built");
        let entries: Vec<_> = std::env::split_paths(&result).collect();

        assert_eq!(
            entries.first().map(|p| p.as_path()),
            Some(self_dir),
            "self binary dir must be the first PATH entry"
        );
        // 기존 항목도 보존되어야 한다(덮어쓰기가 아니라 prepend).
        assert!(
            entries
                .iter()
                .any(|p| p == std::path::Path::new("/usr/bin")),
            "existing PATH entries must be preserved"
        );
    }

    /// base 가 없어도(부모에 PATH env 자체가 없는 드문 케이스) self-dir 만으로
    /// 유효한 PATH 를 만든다.
    #[test]
    fn builds_path_from_self_dir_when_base_absent() {
        let result = path_prepending_self_dir(None).expect("path built");
        let entries: Vec<_> = std::env::split_paths(&result).collect();
        assert_eq!(entries.len(), 1, "only the self binary dir");
    }
}
