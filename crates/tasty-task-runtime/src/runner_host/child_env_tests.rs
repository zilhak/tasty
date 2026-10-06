//! Run task 자식의 환경. 바깥 신원 변수를 둔 프로세스에서 실제 Run 을 실행해 확인한다.
//! 시험 프로세스의 환경을 바꾸지 않도록 테스트 바이너리를 helper 로 다시 실행한다.

use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::json;
use tasty_agent::TaskState;
use tasty_agent::runner::RunnerLoop;
use tasty_agent::task::{TaskGraphSpec, TaskStore};

use super::super::tests::fresh_ctx;
use super::super::*;

const DIR_ENV: &str = "TASTY_RUN_ENV_TEST_DIR";

fn store_op<R>(ctx: &RunnerContext, f: impl FnOnce(&mut TaskStore) -> R) -> R {
    ctx.with_memory(|mem| {
        let seq = ctx.agent_seq.clone();
        let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
        f(&mut store)
    })
}

/// helper: 이 프로세스의 환경을 상속하는 Run task 로 `env` 를 파일에 쓴다. 환경변수가 없으면
/// 아무것도 하지 않는다.
#[test]
fn run_task_env_helper() {
    let Some(dir) = std::env::var_os(DIR_ENV) else {
        return;
    };
    let out = Path::new(&dir).join("env.txt");
    let (_td, ctx) = fresh_ctx();
    let spec: TaskGraphSpec = serde_json::from_value(json!({
        "contract_version": 2,
        "tasks": [{"id": "dump", "command": {"kind": "run", "workspace_id": 1,
                   "command": ["sh", "-c", format!("env > '{}'", out.display())]}}]
    }))
    .expect("graph");
    store_op(&ctx, |s| s.submit_graph(1, spec, 0).unwrap());
    let mut runner = RunnerLoop::new(HostExecutor::new(ctx.clone()));
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let snapshot = store_op(&ctx, |s| s.list(1).unwrap());
        let (a, b) = (ctx.clone(), ctx.clone());
        runner.tick(
            1,
            now_ms(),
            &snapshot,
            move |ws, id, st, n| store_op(&a, |s| s.set_state(ws, id, st, n).map(|_| ())),
            move |ws, id, c, n| store_op(&b, |s| s.complete(ws, id, c, n).map(|_| ())),
        );
        let task = store_op(&ctx, |s| s.get(1, &"dump".to_string()).unwrap().unwrap());
        if task.state.is_terminal() {
            assert_eq!(task.state, TaskState::Succeeded, "{:?}", task.result);
            return;
        }
        assert!(Instant::now() < deadline, "Run task did not finish");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_run_task_does_not_inherit_the_outer_session_identities() {
    let td = tempfile::tempdir().expect("tempdir");
    let status = std::process::Command::new(std::env::current_exe().expect("exe"))
        .args([
            "--exact",
            "runner_host::child_env::tests::run_task_env_helper",
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
            "{gone} reached the Run task:\n{env}"
        );
    }
    for kept in ["TASTY_LOCALE=ko", "ANTHROPIC_API_KEY=user-key"] {
        assert!(env.lines().any(|l| l == kept), "{kept} missing:\n{env}");
    }
}
