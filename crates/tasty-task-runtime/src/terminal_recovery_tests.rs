//! Commit-after-write failure injection exercises recovery, without replaying an external action.
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::time::Duration;

use tasty_agent::task::{Completion, TaskCreateOpts};
use tasty_agent::{OnFailure, Task, TaskCommand, TaskResult, TaskState, TaskStore};
use tasty_memory::testing::faults::{Faults, FaultyStore};
use tasty_memory::writer::{MemoryAccess, settle};
use tasty_memory::{HOST_OWNER, MemoryStorage};

use crate::event_feed::AgentEvent;
use crate::task_waker::{AwaitOutcome, TerminalSnapshot};
use crate::{TaskScope, TaskService};

struct Fixture {
    faults: Arc<Faults>,
    service: TaskService,
    scope: TaskScope,
    task: Task,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

fn fixture() -> Fixture {
    fixture_with_prefix("tasty.agent.task.")
}

fn fixture_with_prefix(prefix: &str) -> Fixture {
    let faults = Arc::new(
        Faults::default()
            .writing(prefix)
            .listing(tasty_agent::task::terminal::PREFIX),
    );
    let memory: Arc<Mutex<dyn MemoryStorage>> =
        Arc::new(Mutex::new(FaultyStore::new(faults.clone())));
    let service = TaskService::new(
        MemoryAccess::inline(memory),
        Arc::new(OnceLock::new()),
        Arc::new(crate::completion::fixture::Resolver::default()),
    );
    let scope = TaskScope::new(service.runner_registry().clone());
    let ctx = service.runner_context(&scope);
    let task = ctx.with_store(|mem| {
        let mut store = TaskStore::new(mem, HOST_OWNER, scope.agent_seq());
        let task = store
            .create(TaskCreateOpts {
                workspace_id: 1,
                name: "parent".into(),
                command: TaskCommand::Custom {
                    ipc_method: "system.ping".into(),
                    params: serde_json::json!({}),
                    poll: None,
                },
                depends_on: vec![],
                on_failure: OnFailure::Abort,
                metadata: serde_json::json!({}),
                now_ms: now_ms(),
            })
            .unwrap();
        store
            .set_state(1, &task.id, TaskState::Running, 2)
            .unwrap()
            .0
    });
    Fixture {
        faults,
        service,
        scope,
        task,
    }
}

fn completed() -> Completion {
    Completion::succeeded(
        None,
        TaskResult {
            exit_code: Some(0),
            output: None,
            error: None,
        },
    )
}

fn registered_waiter(f: &Fixture) -> mpsc::Receiver<AwaitOutcome> {
    let hub = f.scope.waker_hub().clone();
    let task = f.task.clone();
    let (registered_tx, registered_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let outcome = hub.await_terminal(1, &task.id, Some(0), || {
            registered_tx.send(()).unwrap();
            Some(TerminalSnapshot::of(&task, None))
        });
        // The receiver may have reported a failed assertion and gone away.
        drop(done_tx.send(outcome));
    });
    registered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    done_rx
}

fn receive(f: &Fixture, rx: mpsc::Receiver<AwaitOutcome>) -> TerminalSnapshot {
    match rx.recv_timeout(Duration::from_secs(5)) {
        Ok(AwaitOutcome::Terminal(snapshot)) => snapshot,
        other => {
            // Release our own timeout-zero waiter even when a mutation makes this test fail.
            f.scope
                .waker_hub()
                .fire(1, &f.task.id, TerminalSnapshot::of(&f.task, None));
            panic!("confirmed terminal did not wake waiter: {other:?}");
        }
    }
}

fn events(scope: &TaskScope) -> Vec<(String, String)> {
    scope
        .event_queue()
        .take_pending()
        .0
        .into_iter()
        .filter_map(|e| match e {
            AgentEvent::TaskFinished {
                task_id,
                terminal_id,
                ..
            } => Some((task_id, terminal_id.expect("durable identity"))),
            _ => None,
        })
        .collect()
}

fn fault(f: &Fixture) {
    Faults::arm(&f.faults.units_after_writing, 1);
}
fn consumed(f: &Fixture) {
    assert_eq!(f.faults.units_after_writing.load(Ordering::SeqCst), 0);
}

#[test]
fn v1_unknown_completion_wakes_timeout_zero_without_changing_the_error_or_replaying() {
    let f = fixture();
    let waiter = registered_waiter(&f);
    fault(&f);
    assert!(
        f.service
            .task_complete(&f.scope, 1, &f.task.id, completed(), 3)
            .is_err()
    );
    consumed(&f);
    assert_eq!(receive(&f, waiter).state, TaskState::Succeeded);
    assert_eq!(events(&f.scope).len(), 1);
    assert!(
        f.service
            .task_complete(&f.scope, 1, &f.task.id, completed(), 4)
            .is_err()
    );
    assert!(events(&f.scope).is_empty());
}

#[test]
fn ipc_transition_request_retains_unknown_response_but_recovers_confirmed_terminal() {
    let f = fixture();
    let waiter = registered_waiter(&f);
    let scope = f.scope.clone();
    let id = f.task.id.clone();
    let (request, recovery) = f.service.unit_request_with_recovery(0, move |tx| {
        tx.task_set_state(&scope, 1, &id, TaskState::Succeeded, 3)
    });
    fault(&f);
    let mut ticket = f.service.memory_access().submit(request).unwrap();
    let response = settle(ticket.try_take().unwrap());
    assert!(response.is_err(), "IPC must retain an unsettled response");
    consumed(&f);
    recovery.schedule(f.service.memory_access());
    assert_eq!(receive(&f, waiter).state, TaskState::Succeeded);
    assert_eq!(events(&f.scope).len(), 1);
    recovery.schedule(f.service.memory_access());
    assert!(events(&f.scope).is_empty());
}

#[test]
fn consumed_hook_mapping_still_recovers_after_commit_without_an_answer() {
    let f = fixture();
    let waiter = registered_waiter(&f);
    f.service.hook_task_waits().register_owned(
        71,
        1,
        f.task.id.clone(),
        u64::MAX,
        crate::hook_wait::HookWaitOwner {
            agent_seq: f.scope.agent_seq().clone(),
            completion: f.scope.waker_hub().clone(),
        },
        None,
    );
    fault(&f);
    f.service.resolve_hook_task_wait(&f.scope, 71, Some(0), 3);
    consumed(&f);
    assert_eq!(receive(&f, waiter).state, TaskState::Succeeded);
    assert_eq!(events(&f.scope).len(), 1);
    f.service.resolve_hook_task_wait(&f.scope, 71, Some(0), 4);
    assert!(events(&f.scope).is_empty());
}

#[test]
fn restart_recovers_a_lost_response_and_already_terminal_downstream_once() {
    let f = fixture();
    let ctx = f.service.runner_context(&f.scope);
    let child = ctx.with_store(|mem| {
        TaskStore::new(mem, HOST_OWNER, f.scope.agent_seq())
            .create(TaskCreateOpts {
                workspace_id: 1,
                name: "downstream must not execute".into(),
                command: TaskCommand::Run {
                    command: vec!["false".into()],
                    workspace_id: 1,
                    cwd: None,
                },
                depends_on: vec![f.task.id.clone()],
                on_failure: OnFailure::Abort,
                metadata: serde_json::json!({}),
                now_ms: now_ms(),
            })
            .unwrap()
    });
    let waiter = registered_waiter(&f);
    fault(&f);
    // Deliberately drop the response before any runtime continuation can reconcile it.
    let seq = f.scope.agent_seq().clone();
    let id = f.task.id.clone();
    let unknown = f.service.memory_access().run_blocking(0, move |mem| {
        TaskStore::new(mem, HOST_OWNER, &seq).set_state(
            1,
            &id,
            TaskState::Failed {
                error: "failed".into(),
            },
            now_ms(),
        )
    });
    assert!(unknown.is_err());
    consumed(&f);
    assert!(events(&f.scope).is_empty());
    f.service.purge_stale_agent_state_on_boot(&f.scope, &[1]);
    assert!(matches!(
        receive(&f, waiter).state,
        TaskState::Failed { .. }
    ));
    let first = events(&f.scope);
    assert_eq!(first.len(), 2);
    assert!(first.iter().any(|(id, _)| id == &child.id));
    assert_ne!(first[0].1, first[1].1);
    let loaded = ctx.with_store(|mem| {
        TaskStore::new(mem, HOST_OWNER, f.scope.agent_seq())
            .get(1, &child.id)
            .unwrap()
            .unwrap()
    });
    assert!(loaded.state.is_terminal());
    f.service.purge_stale_agent_state_on_boot(&f.scope, &[1]);
    assert!(events(&f.scope).is_empty());
}

#[test]
fn an_old_round_obligation_never_finishes_a_new_running_round_waiter() {
    let mut f = fixture();
    let ctx = f.service.runner_context(&f.scope);
    ctx.with_store(|mem| {
        let mut store = TaskStore::new(mem, HOST_OWNER, f.scope.agent_seq());
        store
            .set_state(
                1,
                &f.task.id,
                TaskState::Failed {
                    error: "old".into(),
                },
                3,
            )
            .unwrap();
        store.retry(1, &f.task.id, false, 4).unwrap();
        f.task = store
            .set_state(1, &f.task.id, TaskState::Running, 5)
            .unwrap()
            .0;
    });
    let waiter = registered_waiter(&f);
    super::terminal_recovery::reconcile(&ctx, 1);
    assert_eq!(events(&f.scope).len(), 1);
    assert!(matches!(waiter.try_recv(), Err(mpsc::TryRecvError::Empty)));
    f.service
        .task_complete(&f.scope, 1, &f.task.id, completed(), 6)
        .unwrap();
    assert_eq!(receive(&f, waiter).state, TaskState::Succeeded);
    assert_eq!(events(&f.scope).len(), 1);
}

#[test]
fn a_restart_completion_commit_can_lose_its_response_without_losing_notification() {
    let f = fixture();
    let ctx = f.service.runner_context(&f.scope);
    ctx.with_store(|mem| {
        let scope = tasty_memory::Scope::Workspace(1);
        let handle = serde_json::json!({"kind":"shell_process", "data":{"pid":0xFFFF_FFFEu32}});
        mem.put(
            HOST_OWNER,
            &scope,
            &crate::runner_host::handle_key(&f.task.id),
            &tasty_memory::MemoryValue::Json(handle),
            &Default::default(),
        )
        .unwrap();
        let result = serde_json::json!({"kind":"failed", "error":"stored before restart"});
        mem.put(
            HOST_OWNER,
            &scope,
            &crate::runner_host::run_result_key(&f.task.id),
            &tasty_memory::MemoryValue::Json(result),
            &Default::default(),
        )
        .unwrap();
    });
    let waiter = registered_waiter(&f);
    fault(&f);
    f.service.purge_stale_agent_state_on_boot(&f.scope, &[1]);
    consumed(&f);
    assert!(matches!(
        receive(&f, waiter).state,
        TaskState::Failed { .. }
    ));
    assert_eq!(events(&f.scope).len(), 1);
    f.service.purge_stale_agent_state_on_boot(&f.scope, &[1]);
    assert!(events(&f.scope).is_empty());
}

#[test]
fn an_unacknowledged_obligation_is_deduplicated_and_survives_until_a_confirmed_ack() {
    let f = fixture();
    let ctx = f.service.runner_context(&f.scope);
    ctx.with_store(|mem| {
        TaskStore::new(mem, HOST_OWNER, f.scope.agent_seq())
            .set_state(1, &f.task.id, TaskState::Succeeded, 3)
            .unwrap()
    });
    // Read barriers do not open units. The next unit is the acknowledgement, not the task write.
    Faults::arm(&f.faults.units_before_body, 1);
    super::terminal_recovery::reconcile(&ctx, 1);
    assert_eq!(events(&f.scope).len(), 1);
    assert_eq!(
        ctx.with_store(|mem| tasty_agent::task::terminal::pending(mem, 1).unwrap().len()),
        1
    );
    super::terminal_recovery::reconcile(&ctx, 1);
    assert!(events(&f.scope).is_empty());
    assert_eq!(
        ctx.with_store(|mem| tasty_agent::task::terminal::pending(mem, 1).unwrap().len()),
        0
    );
}

#[test]
fn unknown_schema_is_preserved_and_no_notification_is_invented() {
    let f = fixture();
    let ctx = f.service.runner_context(&f.scope);
    let key = format!("{}foreign", tasty_agent::task::terminal::PREFIX);
    ctx.with_store(|mem| {
        mem.put(
            HOST_OWNER,
            &tasty_memory::Scope::Workspace(1),
            &key,
            &tasty_memory::MemoryValue::Json(serde_json::json!({"schema":99})),
            &Default::default(),
        )
        .unwrap()
    });
    super::terminal_recovery::reconcile(&ctx, 1);
    assert!(events(&f.scope).is_empty());
    assert!(
        ctx.with_store(|mem| mem.get(&tasty_memory::Scope::Workspace(1), &key).unwrap())
            .is_some()
    );
}

#[test]
fn a_transient_confirmation_read_failure_is_retried_without_repeating_the_mutation() {
    let f = fixture();
    let waiter = registered_waiter(&f);
    fault(&f);
    Faults::arm(&f.faults.prefix_lists, 1);
    assert!(
        f.service
            .task_complete(&f.scope, 1, &f.task.id, completed(), 3)
            .is_err()
    );
    consumed(&f);
    assert_eq!(f.faults.prefix_lists.load(Ordering::SeqCst), 0);
    assert_eq!(receive(&f, waiter).state, TaskState::Succeeded);
    assert_eq!(events(&f.scope).len(), 1);
}

#[test]
fn file_backed_terminal_obligation_survives_close_and_is_acknowledged_after_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("owned-memory.db");
    let mut task = fixture().task;
    task.state = TaskState::Succeeded;
    let identity = {
        let mut mem = tasty_memory::MemoryStore::open(&path).unwrap();
        TaskStore::new(&mut mem, HOST_OWNER, &std::sync::atomic::AtomicU64::new(0))
            .put(&task)
            .unwrap();
        tasty_agent::task::terminal::pending(&mem, 1)
            .unwrap()
            .pop()
            .unwrap()
            .id
    };
    let mem = tasty_memory::MemoryStore::open(&path).unwrap();
    let service = TaskService::new(
        MemoryAccess::inline(Arc::new(Mutex::new(mem))),
        Arc::new(OnceLock::new()),
        Arc::new(crate::completion::fixture::Resolver::default()),
    );
    let scope = TaskScope::new(service.runner_registry().clone());
    service.purge_stale_agent_state_on_boot(&scope, &[1]);
    assert_eq!(events(&scope), vec![(task.id, identity)]);
    drop(service);
    drop(scope);
    let mem = tasty_memory::MemoryStore::open(&path).unwrap();
    assert!(
        tasty_agent::task::terminal::pending(&mem, 1)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn a_committed_ack_with_a_lost_response_does_not_reissue_a_terminal_event() {
    let f = fixture_with_prefix(tasty_agent::task::terminal::PREFIX);
    let ctx = f.service.runner_context(&f.scope);
    ctx.with_store(|mem| {
        TaskStore::new(mem, HOST_OWNER, f.scope.agent_seq())
            .set_state(1, &f.task.id, TaskState::Succeeded, 3)
            .unwrap()
    });
    fault(&f);
    super::terminal_recovery::reconcile(&ctx, 1);
    consumed(&f);
    assert_eq!(events(&f.scope).len(), 1);
    assert!(
        ctx.with_store(|mem| tasty_agent::task::terminal::pending(mem, 1).unwrap())
            .is_empty()
    );
    super::terminal_recovery::reconcile(&ctx, 1);
    assert!(events(&f.scope).is_empty());
}

#[test]
fn failed_confirmation_reads_leave_the_obligation_pending_without_fabricating_success() {
    let f = fixture();
    let ctx = f.service.runner_context(&f.scope);
    let waiter = registered_waiter(&f);
    fault(&f);
    Faults::arm(&f.faults.prefix_lists, 3);
    assert!(
        f.service
            .task_complete(&f.scope, 1, &f.task.id, completed(), 3)
            .is_err()
    );
    consumed(&f);
    assert_eq!(f.faults.prefix_lists.load(Ordering::SeqCst), 0);
    assert!(events(&f.scope).is_empty());
    assert!(matches!(waiter.try_recv(), Err(mpsc::TryRecvError::Empty)));
    assert_eq!(
        ctx.with_store(|mem| tasty_agent::task::terminal::pending(mem, 1).unwrap().len()),
        1
    );
    super::terminal_recovery::reconcile(&ctx, 1);
    assert_eq!(receive(&f, waiter).state, TaskState::Succeeded);
    assert_eq!(events(&f.scope).len(), 1);
}
