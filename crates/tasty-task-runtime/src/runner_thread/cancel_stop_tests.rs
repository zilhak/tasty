//! 취소한 task 의 프로세스: 프로세스 묶음을 끝내고 종료를 확인한 뒤에 점유를 반환한다.
//! 러너가 지켜볼 때(tick 이 종결을 흡수), 러너가 없을 때(취소 시 바로), 재시작 뒤(reload)를 본다.

use std::os::unix::process::CommandExt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use tasty_agent::platform::process_start;
use tasty_agent::runner::{DispatchOutcome, PollOutcome, TaskExecutor};
use tasty_agent::task::TaskCreateOpts;
use tasty_agent::{OnFailure, SemaphoreStore, Task, TaskCommand, TaskState, TaskStore};
use tasty_memory::{HOST_OWNER, MemoryStorage, MemoryStore, MemoryValue, PutOpts, Scope};

use crate::runner_host::{HostExecutor, RunnerContext, handle_key};
use crate::{TaskScope, TaskService};

const WAIT: Duration = Duration::from_secs(10);

fn memory(dir: &Path) -> Arc<Mutex<dyn MemoryStorage>> {
    Arc::new(Mutex::new(MemoryStore::open(&dir.join("mem.db")).unwrap()))
}

fn ctx_on(memory: Arc<Mutex<dyn MemoryStorage>>) -> RunnerContext {
    RunnerContext {
        scope_stopping: Arc::new(AtomicBool::new(false)),
        memory,
        agent_seq: Arc::new(AtomicU64::new(0)),
        host_ipc: Arc::new(OnceLock::new()),
        task_waker_hub: Arc::new(crate::task_waker::TaskWakerHub::new()),
        hook_task_waits: Arc::new(crate::hook_wait::HookTaskWaits::new()),
        agent_turns: Default::default(),
        completion: Arc::new(crate::completion::fixture::Resolver::default()),
    }
}

/// 자식이 손자를 만들고 손자의 PID 를 `pid_file` 에 쓴 뒤 기다리는 명령.
fn family_command(pid_file: &Path) -> Vec<String> {
    vec![
        "sh".into(),
        "-c".into(),
        format!("sleep 60 & echo $! > {}; wait", pid_file.display()),
    ]
}

fn read_pid(pid_file: &Path) -> u32 {
    let deadline = Instant::now() + WAIT;
    loop {
        if let Ok(s) = std::fs::read_to_string(pid_file)
            && let Ok(pid) = s.trim().parse()
        {
            return pid;
        }
        assert!(Instant::now() < deadline, "grandchild pid was not written");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// 끝났거나 회수만 남았다(좀비).
fn gone(pid: u32) -> bool {
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(stat) => stat
            .rsplit_once(')')
            .is_some_and(|(_, rest)| rest.trim_start().starts_with('Z')),
        Err(_) => !tasty_agent::platform::process_alive::is_alive(pid),
    }
}

fn eventually_gone(pid: u32) -> bool {
    let deadline = Instant::now() + WAIT;
    while Instant::now() < deadline {
        if gone(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

/// 이전 호스트가 띄운 Run 처럼 새 프로세스 그룹으로 실행한다. 회수는 별도 스레드가 한다(재시작
/// 뒤에는 init 이, 같은 호스트에서는 watcher 가 회수한다).
fn spawn_family(pid_file: &Path) -> (u32, u64, u32) {
    let argv = family_command(pid_file);
    let mut cmd = std::process::Command::new(&argv[0]);
    cmd.args(&argv[1..]).process_group(0);
    let mut child = cmd.spawn().unwrap();
    let pid = child.id();
    let started_at = process_start::start_time(pid).unwrap();
    std::thread::spawn(move || child.wait());
    (pid, started_at, read_pid(pid_file))
}

fn run_task(mem: &Arc<Mutex<dyn MemoryStorage>>, seq: &AtomicU64, pid_file: &Path) -> Task {
    let mut guard = mem.lock().unwrap();
    let mut store = TaskStore::new(&mut *guard, HOST_OWNER, seq);
    let t = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "family".into(),
            command: TaskCommand::Run {
                command: family_command(pid_file),
                workspace_id: 1,
                cwd: None,
            },
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: serde_json::json!({"semaphore": {"name": "gpu"}}),
            now_ms: 1000,
        })
        .unwrap();
    SemaphoreStore::new(&mut *guard, HOST_OWNER)
        .create(1, "gpu", 1, 1000)
        .unwrap();
    t
}

fn hold(mem: &Arc<Mutex<dyn MemoryStorage>>, seq: &AtomicU64, id: &str, pid: u32, t: u64) {
    let mut guard = mem.lock().unwrap();
    TaskStore::new(&mut *guard, HOST_OWNER, seq)
        .set_state(1, &id.to_string(), TaskState::Running, 1100)
        .unwrap();
    assert!(
        SemaphoreStore::new(&mut *guard, HOST_OWNER)
            .acquire(1, "gpu", id, None, 1100)
            .unwrap()
            .acquired
    );
    guard
        .put(
            HOST_OWNER,
            &Scope::Workspace(1),
            &handle_key(id),
            &MemoryValue::Json(serde_json::json!({
                "kind": "shell_process", "data": {"pid": pid}, "started_at": t})),
            &PutOpts::default(),
        )
        .unwrap();
}

fn gpu_holders(mem: &Arc<Mutex<dyn MemoryStorage>>) -> Vec<String> {
    let mut guard = mem.lock().unwrap();
    SemaphoreStore::new(&mut *guard, HOST_OWNER)
        .get(1, "gpu")
        .unwrap()
        .unwrap()
        .holders
        .into_iter()
        .map(|h| h.id)
        .collect()
}

fn handle_left(mem: &Arc<Mutex<dyn MemoryStorage>>, id: &str) -> bool {
    let guard = mem.lock().unwrap();
    guard
        .get(&Scope::Workspace(1), &handle_key(id))
        .unwrap()
        .is_some()
}

/// 러너가 지켜보는 Run 을 취소하면 tick 이 종결을 흡수해(release_permit) 묶음을 끝낸다. 종료를
/// 확인하기 전에는 permit 을 쥐고 있고, 확인한 다음 maintain 에서 반환한다.
#[test]
fn a_watched_run_is_killed_with_its_children_before_its_permit_returns() {
    let td = tempfile::tempdir().unwrap();
    let mem = memory(td.path());
    let ctx = ctx_on(mem.clone());
    let pid_file = td.path().join("grandchild.pid");
    let task = run_task(&mem, &ctx.agent_seq, &pid_file);
    let mut exec = HostExecutor::new(ctx.clone());
    let DispatchOutcome::Started(handle) = exec.dispatch(&task) else {
        panic!("dispatch");
    };
    let grandchild = read_pid(&pid_file);
    let tasty_agent::runner::DispatchHandle::ShellProcess { pid } = handle else {
        panic!("{handle:?}");
    };
    assert!(matches!(exec.poll(&handle), PollOutcome::Active));
    let stored = ctx.with_memory(|mem| crate::runner_host::stored_process(mem, 1, &task.id));
    assert_eq!(stored.map(|p| p.pid), Some(pid));
    assert!(stored.unwrap().started_at.is_some(), "시작 시각을 저장한다");

    exec.release_permit(&task.id);
    assert_eq!(
        gpu_holders(&mem),
        vec![task.id.clone()],
        "종료 확인 전에 반환했다"
    );
    let deadline = Instant::now() + WAIT;
    while !gpu_holders(&mem).is_empty() && Instant::now() < deadline {
        exec.maintain();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        gpu_holders(&mem).is_empty(),
        "종료를 확인했는데 반환하지 않았다"
    );
    assert!(gone(pid), "Run 이 남았다");
    assert!(eventually_gone(grandchild), "Run 이 만든 프로세스가 남았다");
    assert!(!handle_left(&mem, &task.id));
}

/// 러너가 없을 때 취소하면 취소가 바로 묶음을 끝내고, 종료를 확인한 뒤 반환한다.
#[test]
fn cancelling_with_no_runner_kills_the_run_then_returns_its_permit() {
    let td = tempfile::tempdir().unwrap();
    let mem = memory(td.path());
    let svc = TaskService::new(
        mem.clone(),
        Arc::new(OnceLock::new()),
        Arc::new(crate::completion::fixture::Resolver::default()),
    );
    let scope = TaskScope::new(svc.runner_registry().clone());
    let pid_file = td.path().join("grandchild.pid");
    let task = run_task(&mem, scope.agent_seq(), &pid_file);
    let (pid, started_at, grandchild) = spawn_family(&pid_file);
    hold(&mem, scope.agent_seq(), &task.id, pid, started_at);

    svc.task_cancel(&scope, 1, &task.id, 2000).unwrap();
    assert!(gone(pid), "취소한 Run 이 남았다");
    assert!(eventually_gone(grandchild), "Run 이 만든 프로세스가 남았다");
    assert!(
        gpu_holders(&mem).is_empty(),
        "취소한 Run 의 permit 이 남았다"
    );
    assert!(!handle_left(&mem, &task.id));
}

/// 재시작 뒤 러너를 켜기 전에(부팅 정리 전에) 취소한 Run 은 reload 가 끝내고 반환한다.
#[test]
fn a_run_cancelled_before_the_restart_cleanup_is_killed_by_it() {
    let td = tempfile::tempdir().unwrap();
    let mem = memory(td.path());
    let ctx = ctx_on(mem.clone());
    let pid_file = td.path().join("grandchild.pid");
    let task = run_task(&mem, &ctx.agent_seq, &pid_file);
    let (pid, started_at, grandchild) = spawn_family(&pid_file);
    hold(&mem, &ctx.agent_seq, &task.id, pid, started_at);
    {
        let mut guard = mem.lock().unwrap();
        TaskStore::new(&mut *guard, HOST_OWNER, ctx.agent_seq.as_ref())
            .cancel(1, &task.id, 2000)
            .unwrap();
    }
    assert_eq!(gpu_holders(&mem), vec![task.id.clone()]);

    super::purge_stale_agent_state_on_boot(&ctx, &[1]);
    assert!(gone(pid), "취소한 Run 이 남았다");
    assert!(eventually_gone(grandchild), "Run 이 만든 프로세스가 남았다");
    assert!(
        gpu_holders(&mem).is_empty(),
        "취소한 Run 의 permit 이 남았다"
    );
    assert!(!handle_left(&mem, &task.id));
}

/// 저장한 PID 를 다른 프로세스가 쓰고 있으면(시작 시각이 다르다) 같은 Run 이 아니다. 끝내지 않고,
/// Run 은 이미 끝난 것으로 본다.
#[test]
fn a_reused_pid_is_neither_killed_nor_taken_for_the_run() {
    let td = tempfile::tempdir().unwrap();
    let mem = memory(td.path());
    let ctx = ctx_on(mem.clone());
    let pid_file = td.path().join("unused.pid");
    let me = std::process::id();
    let reused = process_start::start_time(me).unwrap() + 1;

    // Running 이면 결과 불명으로 끝낸다(재시작 뒤 같은 프로세스가 아니다).
    let running = run_task(&mem, &ctx.agent_seq, &pid_file);
    hold(&mem, &ctx.agent_seq, &running.id, me, reused);
    assert!(super::purge_and_reload_on_restart(&ctx, 1).is_empty());
    let state = ctx.with_memory(|mem| {
        TaskStore::new(mem, HOST_OWNER, ctx.agent_seq.as_ref())
            .get(1, &running.id)
            .unwrap()
            .unwrap()
            .state
    });
    assert!(matches!(state, TaskState::Unknown { .. }), "{state:?}");
    assert!(gpu_holders(&mem).is_empty());

    // 취소한 task 의 handle 이 가리키는 다른 프로세스는 끝내지 않는다(이 시험 프로세스가 산다).
    let other = {
        let mut guard = mem.lock().unwrap();
        let mut store = TaskStore::new(&mut *guard, HOST_OWNER, ctx.agent_seq.as_ref());
        let t = store
            .create(TaskCreateOpts {
                workspace_id: 1,
                name: "other".into(),
                command: TaskCommand::Run {
                    command: vec!["true".into()],
                    workspace_id: 1,
                    cwd: None,
                },
                depends_on: vec![],
                on_failure: OnFailure::Abort,
                metadata: serde_json::json!({"semaphore": {"name": "gpu"}}),
                now_ms: 3000,
            })
            .unwrap();
        t.id
    };
    hold(&mem, &ctx.agent_seq, &other, me, reused);
    {
        let mut guard = mem.lock().unwrap();
        TaskStore::new(&mut *guard, HOST_OWNER, ctx.agent_seq.as_ref())
            .cancel(1, &other, 4000)
            .unwrap();
    }
    let ended = ctx.with_memory(|mem| {
        TaskStore::new(mem, HOST_OWNER, ctx.agent_seq.as_ref())
            .get(1, &other)
            .unwrap()
            .unwrap()
    });
    assert!(super::settle_ended_task(&ctx, 1, &ended));
    assert!(gpu_holders(&mem).is_empty());
}
