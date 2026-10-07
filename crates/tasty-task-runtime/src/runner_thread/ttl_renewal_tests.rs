//! 러너가 지켜보는 task 의 lease·semaphore TTL 은 task 가 살아 있는 동안 갱신된다. 갱신하지 않으면
//! TTL 이 지난 뒤 다른 holder 가 같은 자원을 가져간다.

use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use tasty_agent::runner::{DispatchOutcome, TaskExecutor};
use tasty_agent::task::TaskCreateOpts;
use tasty_agent::{
    LeaseMode, LeaseStore, OnFailure, SemaphoreStore, TaskCommand, TaskState, TaskStore,
};
use tasty_memory::{HOST_OWNER, MemoryStore, MemoryValue, PutOpts, Scope};

use crate::runner_host::{HostExecutor, RunnerContext, handle_key};

const TTL_MS: u64 = 300;
/// 빈 Run cwd 는 lease 자원으로 채워지므로(`${lease.resource}`) 있는 디렉터리를 자원으로 쓴다.
const DB: &str = "/";

fn ctx() -> (tempfile::TempDir, RunnerContext) {
    let td = tempfile::tempdir().unwrap();
    let mem = MemoryStore::open(&td.path().join("mem.db")).unwrap();
    let ctx = RunnerContext {
        scope_stopping: Arc::new(AtomicBool::new(false)),
        memory: Arc::new(Mutex::new(mem)),
        agent_seq: Arc::new(AtomicU64::new(0)),
        host_ipc: Arc::new(OnceLock::new()),
        task_waker_hub: Arc::new(crate::task_waker::TaskWakerHub::new()),
        hook_task_waits: Arc::new(crate::hook_wait::HookTaskWaits::new()),
        agent_turns: Default::default(),
        completion: Arc::new(crate::completion::fixture::Resolver::default()),
    };
    (td, ctx)
}

fn sleeper(ctx: &RunnerContext) -> tasty_agent::Task {
    sleeper_with(
        ctx,
        serde_json::json!({"lease": {"resource": DB, "ttl_ms": TTL_MS}}),
    )
}

fn sleeper_with(ctx: &RunnerContext, metadata: serde_json::Value) -> tasty_agent::Task {
    ctx.with_memory(|mem| {
        TaskStore::new(mem, HOST_OWNER, ctx.agent_seq.as_ref())
            .create(TaskCreateOpts {
                workspace_id: 1,
                name: "sleeper".into(),
                command: TaskCommand::Run {
                    command: vec!["sleep".into(), "30".into()],
                    workspace_id: 1,
                    cwd: None,
                },
                depends_on: vec![],
                on_failure: OnFailure::Abort,
                metadata,
                now_ms: 1000,
            })
            .unwrap()
    })
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

fn other_gets_db(ctx: &RunnerContext) -> bool {
    ctx.with_memory(|mem| {
        LeaseStore::new(mem, HOST_OWNER)
            .acquire(1, DB, "other", None, LeaseMode::Block, now())
            .unwrap()
            .acquired
    })
}

fn db_holder(ctx: &RunnerContext) -> Option<String> {
    ctx.with_memory(|mem| {
        LeaseStore::new(mem, HOST_OWNER)
            .get(1, DB)
            .unwrap()
            .map(|l| l.holder)
    })
}

/// 러너가 tick 마다 부르는 maintain 만으로 TTL 의 몇 배가 지나도 lease 가 유지된다.
fn keep_ticking(exec: &mut HostExecutor, for_: Duration) {
    let end = Instant::now() + for_;
    while Instant::now() < end {
        exec.maintain();
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn a_running_task_keeps_its_ttl_lease_while_the_runner_watches_it() {
    let (_td, ctx) = ctx();
    let task = sleeper(&ctx);
    let mut exec = HostExecutor::new(ctx.clone());
    let outcome = exec.dispatch(&task);
    let DispatchOutcome::Started(_) = outcome else {
        panic!("dispatch: {outcome:?}");
    };
    keep_ticking(&mut exec, Duration::from_millis(TTL_MS * 4));
    assert!(
        !other_gets_db(&ctx),
        "살아 있는 task 의 lease 를 TTL 뒤 다른 holder 가 가져갔다"
    );
    assert_eq!(db_holder(&ctx), Some(task.id.clone()));
    let acquired_at = ctx.with_memory(|mem| {
        LeaseStore::new(mem, HOST_OWNER)
            .get(1, DB)
            .unwrap()
            .unwrap()
            .acquired_at
    });
    assert!(acquired_at < now() - TTL_MS, "갱신이 획득 시각을 바꿨다");

    // 끝나면(취소) 프로세스를 끝내고 반환한다. 그 뒤에는 갱신하지 않는다.
    exec.release_permit(&task.id);
    let deadline = Instant::now() + Duration::from_secs(10);
    while db_holder(&ctx).is_some() && Instant::now() < deadline {
        exec.maintain();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(db_holder(&ctx), None);
}

/// 재시작 뒤 넘겨받은 Run 의 lease 도 갱신한다. 내려가 있던 동안 이미 만료 시각이 지났어도
/// 아무도 가져가지 않았으면 다시 늦춘다.
#[test]
fn a_restored_run_keeps_its_ttl_lease_after_a_restart() {
    let (_td, ctx) = ctx();
    let task = sleeper(&ctx);
    let id = task.id.clone();
    // 이전 호스트가 쥔 lease. 만료 시각은 이미 지났다.
    ctx.with_memory(|mem| {
        TaskStore::new(mem, HOST_OWNER, ctx.agent_seq.as_ref())
            .set_state(1, &id, TaskState::Running, 1100)
            .unwrap();
        assert!(
            LeaseStore::new(mem, HOST_OWNER)
                .acquire(
                    1,
                    DB,
                    &id,
                    Some(TTL_MS),
                    LeaseMode::Fail,
                    now() - 10 * TTL_MS
                )
                .unwrap()
                .acquired
        );
        mem.put(
            HOST_OWNER,
            &Scope::Workspace(1),
            &handle_key(&id),
            &MemoryValue::Json(serde_json::json!({
                "kind": "shell_process", "data": {"pid": std::process::id()}})),
            &PutOpts::default(),
        )
        .unwrap();
    });
    let mut exec = HostExecutor::new(ctx.clone());
    let restored = super::purge_and_reload_on_restart(&ctx, 1);
    assert_eq!(restored.len(), 1);
    let task = super::load_task(&ctx, 1, &id).unwrap();
    exec.adopt_restored_run(1, &task);
    keep_ticking(&mut exec, Duration::from_millis(TTL_MS * 3));
    assert!(
        !other_gets_db(&ctx),
        "복원한 Run 의 lease 를 다른 holder 가 가져갔다"
    );
    assert_eq!(db_holder(&ctx), Some(id));
}

fn gpu_task(ctx: &RunnerContext) -> tasty_agent::Task {
    ctx.with_memory(|mem| {
        SemaphoreStore::new(mem, HOST_OWNER)
            .create(1, "gpu", 1, 1000)
            .unwrap()
    });
    sleeper_with(
        ctx,
        serde_json::json!({"semaphore": {"name": "gpu", "ttl_ms": TTL_MS}}),
    )
}

fn other_gets_gpu(ctx: &RunnerContext) -> bool {
    ctx.with_memory(|mem| {
        SemaphoreStore::new(mem, HOST_OWNER)
            .acquire(1, "gpu", "other", None, now())
            .unwrap()
            .acquired
    })
}

fn gpu_holder(ctx: &RunnerContext) -> Vec<(String, Option<u64>)> {
    ctx.with_memory(|mem| {
        SemaphoreStore::new(mem, HOST_OWNER)
            .get(1, "gpu")
            .unwrap()
            .unwrap()
            .holders
            .into_iter()
            .map(|h| (h.id, h.acquired_at))
            .collect()
    })
}

#[test]
fn a_running_task_keeps_its_ttl_permit_while_the_runner_watches_it() {
    let (_td, ctx) = ctx();
    let task = gpu_task(&ctx);
    let mut exec = HostExecutor::new(ctx.clone());
    let outcome = exec.dispatch(&task);
    let DispatchOutcome::Started(_) = outcome else {
        panic!("dispatch: {outcome:?}");
    };
    keep_ticking(&mut exec, Duration::from_millis(TTL_MS * 4));
    assert!(
        !other_gets_gpu(&ctx),
        "살아 있는 task 의 permit 을 TTL 뒤 다른 holder 가 가져갔다"
    );
    let holders = gpu_holder(&ctx);
    assert_eq!(holders.len(), 1);
    assert_eq!(holders[0].0, task.id);
    assert!(
        holders[0].1.unwrap() < now() - TTL_MS,
        "갱신이 획득 시각을 바꿨다"
    );

    exec.release_permit(&task.id);
    let deadline = Instant::now() + Duration::from_secs(10);
    while !gpu_holder(&ctx).is_empty() && Instant::now() < deadline {
        exec.maintain();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(gpu_holder(&ctx).is_empty());
}

#[test]
fn a_restored_run_keeps_its_ttl_permit_after_a_restart() {
    let (_td, ctx) = ctx();
    let task = gpu_task(&ctx);
    let id = task.id.clone();
    ctx.with_memory(|mem| {
        TaskStore::new(mem, HOST_OWNER, ctx.agent_seq.as_ref())
            .set_state(1, &id, TaskState::Running, 1100)
            .unwrap();
        assert!(
            SemaphoreStore::new(mem, HOST_OWNER)
                .acquire(1, "gpu", &id, Some(TTL_MS), now() - 10 * TTL_MS)
                .unwrap()
                .acquired
        );
        mem.put(
            HOST_OWNER,
            &Scope::Workspace(1),
            &handle_key(&id),
            &MemoryValue::Json(serde_json::json!({
                "kind": "shell_process", "data": {"pid": std::process::id()}})),
            &PutOpts::default(),
        )
        .unwrap();
    });
    let mut exec = HostExecutor::new(ctx.clone());
    assert_eq!(super::purge_and_reload_on_restart(&ctx, 1).len(), 1);
    let task = super::load_task(&ctx, 1, &id).unwrap();
    exec.adopt_restored_run(1, &task);
    keep_ticking(&mut exec, Duration::from_millis(TTL_MS * 3));
    assert!(
        !other_gets_gpu(&ctx),
        "복원한 Run 의 permit 을 다른 holder 가 가져갔다"
    );
    assert_eq!(gpu_holder(&ctx)[0].0, id);
}
