//! 훅 대기는 건 회차에만 보고한다 — 늦게 온 옛 회차의 훅이 다음 회차를 끝내지 않는다.
//! 재시작은 완료 쓰기 뒤 끊긴 하류 반영을 마무리한다.

use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, OnceLock};

use tasty_agent::task::{Completion, TaskGraphSpec};
use tasty_agent::{TaskState, TaskStore};
use tasty_memory::{HOST_OWNER, MemoryStore};

use super::{expire_overdue_hook_waits, purge_and_reload_on_restart};
use crate::hook_wait::HookWaitOwner;
use crate::runner_host::RunnerContext;

fn ctx() -> (tempfile::TempDir, RunnerContext) {
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

fn with_store<R>(ctx: &RunnerContext, f: impl FnOnce(&mut TaskStore) -> R) -> R {
    ctx.with_memory(|mem| f(&mut TaskStore::new(mem, HOST_OWNER, ctx.agent_seq.as_ref())))
}

fn state_of(ctx: &RunnerContext, id: &str) -> TaskState {
    with_store(ctx, |s| s.get(1, &id.into()).unwrap().unwrap().state)
}

fn state(ctx: &RunnerContext) -> TaskState {
    state_of(ctx, "p")
}

fn register(ctx: &RunnerContext, hook_id: u64, attempt: &str) {
    ctx.hook_task_waits.register_owned(
        hook_id,
        1,
        "p".into(),
        2000,
        HookWaitOwner {
            agent_seq: ctx.agent_seq.clone(),
            completion: ctx.task_waker_hub.clone(),
        },
        Some(attempt.into()),
    );
}

#[test]
fn an_expired_wait_from_an_earlier_attempt_leaves_the_current_run_alone() {
    let (_td, ctx) = ctx();
    with_store(&ctx, |s| {
        let spec: TaskGraphSpec = serde_json::from_value(serde_json::json!({
            "contract_version": 2,
            "tasks": [{"id": "p",
                "command": {"kind": "custom", "ipc_method": "system.ping", "params": {}},
                "output_schema": {"type": "object", "fields": {}}}]
        }))
        .unwrap();
        s.submit_graph(1, spec, 0).unwrap();
        let p = "p".to_string();
        s.set_state(1, &p, TaskState::Running, 1).unwrap();
        s.complete(1, &p, Completion::failed(None, "boom".into()), 2)
            .unwrap();
        s.retry(1, &p, false, 3).unwrap();
        s.set_state(1, &p, TaskState::Running, 4).unwrap();
    });
    register(&ctx, 1, "p#1");
    expire_overdue_hook_waits(&ctx, 5000);
    assert_eq!(
        state(&ctx),
        TaskState::Running,
        "p#1 의 만료가 p#2 를 끝냈다"
    );
    assert!(ctx.hook_task_waits.is_empty());

    register(&ctx, 2, "p#2");
    expire_overdue_hook_waits(&ctx, 5000);
    assert!(matches!(state(&ctx), TaskState::Failed { .. }));
}

#[test]
fn a_restart_releases_consumers_left_waiting_after_the_completion_write() {
    let (_td, ctx) = ctx();
    with_store(&ctx, |s| {
        let spec: TaskGraphSpec = serde_json::from_value(serde_json::json!({
            "contract_version": 2,
            "tasks": [
                {"id": "p",
                 "command": {"kind": "custom", "ipc_method": "system.ping", "params": {}},
                 "output_schema": {"type": "object", "fields": {}}},
                {"id": "c", "depends_on": ["p"],
                 "command": {"kind": "custom", "ipc_method": "system.ping", "params": {}}}]
        }))
        .unwrap();
        s.submit_graph(1, spec, 0).unwrap();
        // 완료 레코드는 썼지만 하류 반영 전에 멈춘 상태를 그대로 심는다.
        let mut p = s.get(1, &"p".into()).unwrap().unwrap();
        p.state = TaskState::Succeeded;
        p.finished_at = Some(1);
        s.put(&p).unwrap();
    });
    assert_eq!(state_of(&ctx, "c"), TaskState::Waiting);
    purge_and_reload_on_restart(&ctx, 1);
    assert_eq!(state_of(&ctx, "c"), TaskState::Ready);
}
