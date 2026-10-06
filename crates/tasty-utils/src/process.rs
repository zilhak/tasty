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
/// 넘기면 안 되는 환경변수의 접두사. `CMUX_` 는 자식에서 cmux CLI 가 동작하지 않게 하고,
/// `CLAUDE_CODE_` 는 Claude Code 세션 표지(`CLAUDE_CODE_CHILD_SESSION` 등)와 세션 비밀
/// (`CLAUDE_CODE_MESSAGING_TOKEN` 등)이다. 표지가 남으면 자식에서 띄운 Claude 가
/// 자신을 자식 세션으로 판단해 transcript 를 쓰지 않는다.
pub const STRIPPED_ENV_PREFIXES: &[&str] = &["CMUX_", "CLAUDE_CODE_"];

/// 접두사로 묶이지 않는 Claude Code 세션 환경변수.
pub const STRIPPED_ENV_NAMES: &[&str] = &[
    "CLAUDECODE",
    "CLAUDE_PID",
    "CLAUDE_EFFORT",
    "CLAUDE_PLUGIN_DATA",
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

/// 제거·보존 판정 시험이 공유하는 키 목록. 터미널 셸과 훅 실행 경로의 시험도 같은 목록을 쓴다.
#[doc(hidden)]
pub const STRIPPED_KEYS_FOR_TEST: &[&str] = &[
    "CLAUDECODE",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDE_CODE_SESSION_ATTENDED",
    "CLAUDE_PID",
    "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDE_CODE_MESSAGING_SOCKET",
    "CLAUDE_CODE_MESSAGING_TOKEN",
    "CLAUDE_CODE_EXECPATH",
    "CLAUDE_EFFORT",
    "CLAUDE_PLUGIN_DATA",
    "CMUX_SOCKET_PATH",
];

/// 지우지 않아야 하는 키.
#[doc(hidden)]
pub const KEPT_KEYS_FOR_TEST: &[&str] = &[
    "TASTY_SURFACE_ID",
    "TASTY_PARENT_HOME",
    "TERM",
    "CLAUDE_CONFIG_DIR",
    "ANTHROPIC_API_KEY",
    "MY_CLAUDECODE",
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Claude Code 세션 키와 CMUX_* 는 고르고 TASTY_*·일반 키는 남긴다.
    #[test]
    fn claude_session_and_cmux_keys_are_stripped_but_others_are_kept() {
        let stripped = STRIPPED_KEYS_FOR_TEST;
        let kept = KEPT_KEYS_FOR_TEST;
        let all = stripped.iter().chain(kept).map(OsString::from);
        let picked = env_keys_to_strip(all);
        let expected: Vec<OsString> = stripped.iter().map(OsString::from).collect();
        assert_eq!(picked, expected);
    }

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
