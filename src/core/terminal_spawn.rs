//! 셸 PTY 생성. model은 생성 정보만 보관하고 실제 `Terminal`과 waker는 호스트가 만든다.

use tasty_terminal::{Pty, Terminal, TerminalConfig, Waker};

use crate::model::DeferredSpawn;

/// 새 셸 터미널의 실행 옵션.
pub(crate) struct ShellSpawnOpts<'a> {
    pub cols: usize,
    pub rows: usize,
    pub shell: Option<&'a str>,
    pub shell_args: &'a [&'a str],
    pub waker: Waker,
    pub working_dir: Option<&'a std::path::Path>,
    /// 자식 셸에 추가로 심을 환경변수(docs/features/terminal-output/index.md#명령-인덱싱-osc-133,
    /// `ShellConfig::envs_ref` 참고).
    pub extra_env: &'a [(&'a str, &'a str)],
}

/// 셸 PTY를 만든다. 호출자는 반환된 Terminal을 store에 넣은 뒤 트리에 marker를 붙인다.
/// 그래야 layout이 store에 없는 터미널을 가리키는 순간이 생기지 않는다.
pub(crate) fn spawn_shell_terminal(
    surface_id: u32,
    spawn: ShellSpawnOpts<'_>,
) -> anyhow::Result<(Terminal, Pty)> {
    tasty_terminal::spawn_terminal(
        TerminalConfig {
            cols: spawn.cols,
            rows: spawn.rows,
            shell: spawn.shell,
            args: spawn.shell_args,
            surface_id,
            working_dir: spawn.working_dir,
            initial_input: None,
            extra_env: spawn.extra_env,
        },
        spawn.waker,
    )
}

/// 지연 placeholder의 생성 정보로 PTY를 만든다. waker는 spawn 시점에 호출자가 공급한다.
pub(crate) fn spawn_deferred_terminal(
    surface_id: u32,
    spawn: &DeferredSpawn,
    waker: Waker,
) -> anyhow::Result<(Terminal, Pty)> {
    let shell_args: Vec<&str> = spawn.shell_args.iter().map(|s| s.as_str()).collect();
    let extra_env: Vec<(&str, &str)> = spawn
        .extra_env
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let initial = deferred_initial_input(spawn);
    tasty_terminal::spawn_terminal(
        TerminalConfig {
            cols: spawn.cols,
            rows: spawn.rows,
            shell: spawn.shell.as_deref(),
            args: &shell_args,
            surface_id,
            working_dir: spawn.working_dir.as_deref(),
            initial_input: initial.as_deref(),
            extra_env: &extra_env,
        },
        waker,
    )
}

/// PTY master의 첫 입력으로 넣을 복원 명령. `Terminal::new`가 writer thread를 띄우기 전에
/// 동기로 쓰므로 셸이 stdin을 처음 읽는 순간 이 명령이 들어간다(예: `claude -r <uuid>\r`).
fn deferred_initial_input(spawn: &DeferredSpawn) -> Option<String> {
    spawn.restore_command.as_deref().map(|c| format!("{c}\r"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn noop_waker() -> Waker {
        Arc::new(|| {})
    }

    fn deferred(shell: Option<&str>, restore_command: Option<&str>) -> DeferredSpawn {
        DeferredSpawn {
            shell: shell.map(str::to_string),
            shell_args: Vec::new(),
            extra_env: Vec::new(),
            cols: 80,
            rows: 24,
            working_dir: None,
            restore_command: restore_command.map(str::to_string),
            scrollback_persist_id: None,
        }
    }

    #[test]
    fn spawn_shell_terminal_with_a_missing_shell_returns_err_not_panic() {
        // 잘못된 셸 경로는 복구 가능한 오류로 반환해야 한다.
        let bogus = "/nonexistent/definitely/not/a/real/shell-xyzzy";
        let result = spawn_shell_terminal(
            1,
            ShellSpawnOpts {
                cols: 80,
                rows: 24,
                shell: Some(bogus),
                shell_args: &[],
                waker: noop_waker(),
                working_dir: None,
                extra_env: &[],
            },
        );
        assert!(
            result.is_err(),
            "a missing shell path must return Err, not Ok or panic"
        );
    }

    #[test]
    fn spawn_deferred_terminal_with_a_missing_shell_returns_err() {
        let spawn = deferred(Some("/nonexistent/tasty_no_such_shell"), None);
        assert!(spawn_deferred_terminal(1, &spawn, noop_waker()).is_err());
    }

    #[test]
    fn restore_command_is_preloaded_with_a_trailing_cr() {
        assert_eq!(
            deferred_initial_input(&deferred(None, Some("claude -r abc"))).as_deref(),
            Some("claude -r abc\r")
        );
        assert_eq!(deferred_initial_input(&deferred(None, None)), None);
    }
}
