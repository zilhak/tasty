//! 후처리 단계의 runner — 본 작업은 한 번만, 후처리는 예약대로, permit 은 마지막 종결까지.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::AtomicU64;

use serde_json::json;
use tasty_memory::MemoryStore;

use super::*;
use crate::task::postprocess::PostprocessOutcome;
use crate::task::{TaskGraphSpec, TaskStore};

type Shared = Rc<RefCell<MemoryStore>>;

struct Exec {
    mem: Shared,
    seq: Rc<AtomicU64>,
    dispatched: Vec<TaskId>,
    main_polls: u32,
    /// (task, run, now) 순서의 후처리 시작.
    starts: Vec<(TaskId, u32)>,
    released: Vec<TaskId>,
    /// 실행 번호별 보고. 없으면 성공(true).
    script: HashMap<u32, PostprocessReport>,
    supports_postprocess: bool,
}

impl TaskExecutor for Exec {
    fn dispatch(&mut self, task: &Task) -> DispatchOutcome {
        self.dispatched.push(task.id.clone());
        DispatchOutcome::Started(DispatchHandle::ShellProcess { pid: 1 })
    }
    fn poll(&mut self, handle: &DispatchHandle) -> PollOutcome {
        match handle {
            DispatchHandle::ShellProcess { .. } => {
                self.main_polls += 1;
                PollOutcome::Done(TaskResult {
                    exit_code: Some(0),
                    output: Some(json!({"stdout": {"text": "draft"}})),
                    error: None,
                })
            }
            DispatchHandle::PostprocessProcess { run, .. } => PollOutcome::Postprocessed(
                self.script.get(run).cloned().unwrap_or(PostprocessReport {
                    run: *run,
                    exit_code: Some(0),
                    stderr: None,
                    stderr_truncated: false,
                    outcome: PostprocessOutcome::Collected {
                        stdout: json!(true),
                    },
                }),
            ),
            other => panic!("unexpected poll of {other:?}"),
        }
    }
    fn release_permit(&mut self, task_id: &TaskId) {
        self.released.push(task_id.clone());
    }
    fn start_postprocess(&mut self, task: &Task, attempt_id: &str, run: u32) -> DispatchHandle {
        if !self.supports_postprocess {
            return DispatchHandle::PostprocessResolved(PostprocessReport::failed(
                run,
                PostprocessCause::Spawn,
                "unsupported",
            ));
        }
        let mut mem = self.mem.borrow_mut();
        let mut store = TaskStore::new(&mut *mem, "_host", &self.seq);
        store
            .begin_postprocess_run(1, &task.id, attempt_id, run, 0)
            .expect("begin");
        self.starts.push((task.id.clone(), run));
        DispatchHandle::PostprocessProcess {
            pid: 100 + run,
            run,
        }
    }
}

fn setup(postprocess: serde_json::Value, supports: bool) -> (tempfile::TempDir, RunnerLoop<Exec>) {
    let td = tempfile::tempdir().expect("tempdir");
    let mem = Rc::new(RefCell::new(
        MemoryStore::open(&td.path().join("mem.db")).expect("mem"),
    ));
    let seq = Rc::new(AtomicU64::new(0));
    {
        let mut m = mem.borrow_mut();
        let mut store = TaskStore::new(&mut *m, "_host", &seq);
        let spec: TaskGraphSpec = serde_json::from_value(json!({
            "contract_version": 2,
            "tasks": [
                {"id": "judge", "command": {"kind": "run", "command": ["true"], "workspace_id": 1},
                 "output_schema": {"type": "boolean"}, "postprocess": postprocess},
                {"id": "after", "command": {"kind": "custom", "ipc_method": "system.ping", "params": {}},
                 "depends_on": ["judge"]}
            ]}))
        .expect("spec");
        store.submit_graph(1, spec, 0).expect("submit");
    }
    let exec = Exec {
        mem,
        seq,
        dispatched: Vec::new(),
        main_polls: 0,
        starts: Vec::new(),
        released: Vec::new(),
        script: HashMap::new(),
        supports_postprocess: supports,
    };
    (td, RunnerLoop::new(exec))
}

fn tick(runner: &mut RunnerLoop<Exec>, now: u64) -> HashMap<TaskId, Task> {
    let mem = runner.executor.mem.clone();
    let seq = runner.executor.seq.clone();
    let snapshot = {
        let mut m = mem.borrow_mut();
        TaskStore::new(&mut *m, "_host", &seq)
            .list(1)
            .expect("list")
    };
    runner.tick(
        1,
        now,
        &snapshot,
        |ws, id, st, n| {
            let mut m = mem.borrow_mut();
            TaskStore::new(&mut *m, "_host", &seq)
                .set_state(ws, id, st, n)
                .map(|_| ())
        },
        |ws, id, c, n| {
            let mut m = mem.borrow_mut();
            TaskStore::new(&mut *m, "_host", &seq)
                .complete(ws, id, c, n)
                .map(|_| ())
        },
    );
    let mut m = mem.borrow_mut();
    TaskStore::new(&mut *m, "_host", &seq)
        .list(1)
        .expect("list")
        .into_iter()
        .map(|t| (t.id.clone(), t))
        .collect()
}

/// 첫 후처리가 실패해 재시도 대기에 들어간 상태까지 진행한다(now=30, 재시도는 130 부터).
fn into_retry_wait() -> (tempfile::TempDir, RunnerLoop<Exec>) {
    let (td, mut runner) = setup(
        json!({"command": ["judge"], "timeout_ms": 1000, "retry": {"max_retries": 1, "delay_ms": 100}}),
        true,
    );
    runner.executor.script.insert(
        1,
        PostprocessReport::failed(1, PostprocessCause::InvalidJson, "not json"),
    );
    // tick 1: dispatch, tick 2: 본 작업 완료와 첫 후처리 시작, tick 3: 첫 후처리 실패.
    for now in [10, 20, 30] {
        tick(&mut runner, now);
    }
    (td, runner)
}

#[test]
fn the_first_postprocess_starts_in_the_tick_the_main_work_finishes() {
    let (_td, mut runner) = setup(json!({"command": ["judge"], "timeout_ms": 1000}), true);
    tick(&mut runner, 10);
    let tasks = tick(&mut runner, 20);
    assert_eq!(runner.executor.main_polls, 1);
    assert_eq!(runner.executor.starts, vec![("judge".to_string(), 1)]);
    assert!(matches!(tasks["judge"].state, TaskState::Running));
    assert!(matches!(tasks["after"].state, TaskState::Waiting));
    assert!(runner.executor.released.is_empty());
}

#[test]
fn a_failed_postprocess_waits_before_the_retry_and_keeps_the_permit() {
    let (_td, mut runner) = into_retry_wait();
    assert!(matches!(
        runner.running.get("judge"),
        Some(DispatchHandle::PostprocessPending {
            run: 2,
            not_before_ms: 130
        })
    ));
    let tasks = tick(&mut runner, 120);
    assert!(matches!(tasks["judge"].state, TaskState::Running));
    assert_eq!(runner.executor.starts.len(), 1);
    assert!(runner.executor.released.is_empty());
}

#[test]
fn the_retry_reuses_the_main_result_and_releases_the_permit_once() {
    let (_td, mut runner) = into_retry_wait();
    tick(&mut runner, 130);
    assert_eq!(runner.executor.starts.len(), 2);
    // 본 작업은 다시 실행하지 않는다.
    assert_eq!(runner.executor.main_polls, 1);
    let tasks = tick(&mut runner, 140);
    assert!(matches!(tasks["judge"].state, TaskState::Succeeded));
    assert_eq!(runner.executor.released, vec!["judge".to_string()]);
    let typed = tasks["judge"].typed_result.clone().expect("typed");
    assert_eq!(serde_json::to_value(&typed.output).unwrap(), json!(true));
    let pp = typed.raw.postprocess.expect("raw");
    assert_eq!((pp.run, pp.failed_runs.len()), (2, 1));
}

#[test]
fn the_next_task_is_dispatched_only_after_the_postprocess_finishes() {
    let (_td, mut runner) = into_retry_wait();
    tick(&mut runner, 130);
    assert_eq!(runner.executor.dispatched, vec!["judge".to_string()]);
    tick(&mut runner, 140);
    assert_eq!(runner.executor.dispatched, vec!["judge".to_string()]);
    tick(&mut runner, 150);
    assert_eq!(
        runner.executor.dispatched,
        vec!["judge".to_string(), "after".to_string()]
    );
}

#[test]
fn an_executor_without_postprocess_support_fails_the_task_once() {
    let (_td, mut runner) = setup(json!({"command": ["judge"], "timeout_ms": 1000}), false);
    tick(&mut runner, 10);
    tick(&mut runner, 20);
    let tasks = tick(&mut runner, 30);
    let judge = &tasks["judge"];
    assert!(
        matches!(&judge.state, TaskState::Failed { error } if error.contains("postprocess spawn")),
        "{:?}",
        judge.state
    );
    assert_eq!(runner.executor.released, vec!["judge".to_string()]);
    assert!(matches!(tasks["after"].state, TaskState::Skipped));
}

/// 기록하지 못할 만큼 큰 후처리 보고는 `result_too_large` 실패로 회차를 확정하고 permit 을
/// 한 번 놓는다. 줄이지 않으면 같은 보고를 매 tick 다시 내며 Running 에 머문다.
#[test]
fn a_postprocess_report_too_large_to_record_fails_the_task_and_releases_the_permit() {
    let (_td, mut runner) = setup(
        json!({"command": ["judge"], "timeout_ms": 1000, "retry": {"max_retries": 2, "delay_ms": 0}}),
        true,
    );
    // 사유 하나가 memory 값 상한을 넘는다. 재시도가 남아도 다시 실행하지 않는다.
    runner.executor.script.insert(
        1,
        PostprocessReport::failed(1, PostprocessCause::Spawn, "x".repeat(1100 * 1024)),
    );
    for now in [10, 20, 30] {
        tick(&mut runner, now);
    }
    let tasks = tick(&mut runner, 40);
    let TaskState::Failed { error } = &tasks["judge"].state else {
        panic!("expected failed, got {:?}", tasks["judge"].state);
    };
    assert!(
        error.starts_with("postprocess result_too_large: "),
        "{error}"
    );
    assert_eq!(runner.executor.starts.len(), 1);
    assert_eq!(runner.executor.released, vec!["judge".to_string()]);
    assert!(runner.pending.is_empty());
}
