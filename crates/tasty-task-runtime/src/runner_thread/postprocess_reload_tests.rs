//! 재시작 복구 — 시작을 기록한 후처리는 다시 실행하지 않고, 예약만 된 실행은 이어서 실행한다.

use serde_json::{Value, json};
use tasty_agent::runner::RunnerLoop;
use tasty_agent::task::postprocess::{PostprocessOutcome, PostprocessReport};
use tasty_agent::task::{Completion, TaskGraphSpec};
use tasty_memory::{MemoryValue, PutOpts};

use super::super::runner_host::{handle_value, postprocess_result_key};
use super::*;
use std::sync::OnceLock;
use std::sync::atomic::AtomicU64;
use tasty_agent::{Task, TaskResult};
use tasty_memory::MemoryStore;

fn fresh_ctx() -> (tempfile::TempDir, RunnerContext) {
    let td = tempfile::tempdir().unwrap();
    let mem = MemoryStore::open(&td.path().join("mem.db")).unwrap();
    let ctx = RunnerContext {
        scope_stopping: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        memory: Arc::new(Mutex::new(mem)),
        agent_seq: Arc::new(AtomicU64::new(0)),
        host_ipc: Arc::new(OnceLock::new()),
        task_waker_hub: Arc::new(crate::task_waker::TaskWakerHub::new()),
        hook_task_waits: Arc::new(crate::hook_wait::HookTaskWaits::new()),
        completion: Arc::new(crate::completion::fixture::Resolver::default()),
    };
    (td, ctx)
}

const JUDGE: &str = "judge";

fn store_op<R>(ctx: &RunnerContext, f: impl FnOnce(&mut TaskStore) -> R) -> R {
    ctx.with_memory(|mem| {
        let seq = ctx.agent_seq.clone();
        let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
        f(&mut store)
    })
}

fn judge(ctx: &RunnerContext) -> Task {
    store_op(ctx, |s| {
        s.get(1, &JUDGE.to_string()).unwrap().expect("task")
    })
}

/// 본 작업까지 끝낸 judge 를 만든다. 후처리는 실행되면 `log` 에 한 줄을 남긴다.
fn after_main(ctx: &RunnerContext, log: &std::path::Path, retry: Value) -> String {
    let script = format!("echo pp >> '{}'; printf true", log.display());
    let mut postprocess = json!({"command": ["sh", "-c", script], "timeout_ms": 10000});
    if !retry.is_null() {
        postprocess["retry"] = retry;
    }
    let spec: TaskGraphSpec = serde_json::from_value(json!({
        "contract_version": 2,
        "tasks": [{"id": JUDGE, "output_schema": {"type": "boolean"}, "postprocess": postprocess,
                   "command": {"kind": "run", "workspace_id": 1, "command": ["true"]}}]
    }))
    .expect("graph");
    let id = JUDGE.to_string();
    let attempt = store_op(ctx, |s| {
        s.submit_graph(1, spec, 0).unwrap();
        s.set_state(1, &id, TaskState::Running, 1).unwrap();
        s.get(1, &id).unwrap().unwrap().attempt.unwrap().id
    });
    let main = TaskResult {
        exit_code: Some(0),
        output: None,
        error: None,
    };
    store_op(ctx, |s| {
        s.complete(
            1,
            &id,
            Completion::succeeded(Some(attempt.clone()), main),
            2,
        )
        .unwrap()
    });
    // 이전 호스트가 저장한 본 작업 handle. 복원은 이 handle 이 아니라 회차의 진행을 따른다.
    let handle = handle_value(
        serde_json::to_value(DispatchHandle::ShellProcess { pid: 0xFFFF_FFFE }).unwrap(),
        Some(&attempt),
    );
    let MemoryValue::Json(handle) = handle else {
        panic!("handle value is JSON");
    };
    put(ctx, &handle_key(&id), handle);
    attempt
}

fn put(ctx: &RunnerContext, key: &str, value: Value) {
    ctx.with_memory(|mem| {
        mem.put(
            HOST_OWNER,
            &Scope::Workspace(1),
            key,
            &MemoryValue::Json(value),
            &PutOpts::default(),
        )
        .unwrap()
    });
}

fn begin(ctx: &RunnerContext, attempt: &str, run: u32) {
    store_op(ctx, |s| {
        s.begin_postprocess_run(1, &JUDGE.to_string(), attempt, run, 3)
            .unwrap()
    });
}

/// 재시작한 runner 로 judge 가 끝날 때까지 돌린다.
fn restart_and_finish(ctx: &RunnerContext) -> Task {
    let mut runner = RunnerLoop::new(HostExecutor::new(ctx.clone()));
    for (task_id, handle) in reload_persistent_handles(ctx, 1) {
        runner.running.insert(task_id, handle);
    }
    for _ in 0..500 {
        let snapshot = store_op(ctx, |s| s.list(1).unwrap());
        let (a, b) = (ctx.clone(), ctx.clone());
        runner.tick(
            1,
            now_ms(),
            &snapshot,
            move |ws, id, st, n| store_op(&a, |s| s.set_state(ws, id, st, n).map(|_| ())),
            move |ws, id, c, n| store_op(&b, |s| s.complete(ws, id, c, n).map(|_| ())),
        );
        let t = judge(ctx);
        if t.state.is_terminal() {
            return t;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("judge did not finish: {:?}", judge(ctx).state);
}

fn runs(log: &std::path::Path) -> usize {
    std::fs::read_to_string(log)
        .map(|s| s.lines().count())
        .unwrap_or(0)
}

#[test]
fn a_started_run_without_a_recorded_result_is_not_run_again() {
    let (td, ctx) = fresh_ctx();
    let log = td.path().join("pp.log");
    // 재시도가 남아 있어도 결과 불명은 다시 실행하지 않는다.
    let attempt = after_main(&ctx, &log, json!({"max_retries": 3}));
    begin(&ctx, &attempt, 1);
    let t = restart_and_finish(&ctx);
    assert!(
        matches!(&t.state, TaskState::Failed { error } if error.contains("outcome_unknown")),
        "{:?}",
        t.state
    );
    assert_eq!(runs(&log), 0);
}

#[test]
fn a_started_run_with_a_recorded_result_finishes_with_that_result() {
    let (td, ctx) = fresh_ctx();
    let log = td.path().join("pp.log");
    let attempt = after_main(&ctx, &log, Value::Null);
    begin(&ctx, &attempt, 1);
    let report = PostprocessReport {
        run: 1,
        exit_code: Some(0),
        stderr: None,
        stderr_truncated: false,
        outcome: PostprocessOutcome::Collected {
            stdout: json!(false),
        },
    };
    put(
        &ctx,
        &postprocess_result_key(JUDGE),
        json!({"attempt_id": attempt, "report": report}),
    );
    let t = restart_and_finish(&ctx);
    assert_eq!(t.state, TaskState::Succeeded, "{:?}", t.result);
    let typed = t.typed_result.unwrap();
    assert_eq!(serde_json::to_value(&typed.output).unwrap(), json!(false));
    assert_eq!(runs(&log), 0);
}

#[test]
fn a_result_from_another_attempt_is_not_used() {
    let (td, ctx) = fresh_ctx();
    let log = td.path().join("pp.log");
    let attempt = after_main(&ctx, &log, Value::Null);
    begin(&ctx, &attempt, 1);
    let report = PostprocessReport {
        run: 1,
        exit_code: Some(0),
        stderr: None,
        stderr_truncated: false,
        outcome: PostprocessOutcome::Collected {
            stdout: json!(true),
        },
    };
    put(
        &ctx,
        &postprocess_result_key(JUDGE),
        json!({"attempt_id": format!("{JUDGE}#99"), "report": report}),
    );
    let t = restart_and_finish(&ctx);
    assert!(matches!(&t.state, TaskState::Failed { error } if error.contains("outcome_unknown")));
}

#[test]
fn a_scheduled_run_that_never_started_runs_once_after_the_restart() {
    let (td, ctx) = fresh_ctx();
    let log = td.path().join("pp.log");
    after_main(&ctx, &log, Value::Null);
    let t = restart_and_finish(&ctx);
    assert_eq!(t.state, TaskState::Succeeded, "{:?}", t.result);
    assert_eq!(runs(&log), 1);
}
