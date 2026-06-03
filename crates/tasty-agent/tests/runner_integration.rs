//! Runner 통합 테스트 — Real TaskStore + Mock executor 로 ready → running →
//! succeeded → downstream ready 까지 검증.

use std::collections::HashMap;
use std::sync::atomic::AtomicU64;

use tasty_agent::runner::{DispatchHandle, DispatchOutcome, PollOutcome, RunnerLoop, TaskExecutor};
use tasty_agent::task::TaskCreateOpts;
use tasty_agent::{OnFailure, Task, TaskCommand, TaskResult, TaskState, TaskStore};
use tasty_memory::MemoryStore;
use tempfile::TempDir;

fn fresh_store() -> (TempDir, MemoryStore, AtomicU64) {
    let td = tempfile::tempdir().expect("tempdir");
    let mem = MemoryStore::open(&td.path().join("mem.db")).expect("mem");
    (td, mem, AtomicU64::new(0))
}

fn run_cmd() -> TaskCommand {
    TaskCommand::Run {
        command: vec!["true".into()],
        workspace_id: 1,
        cwd: None,
    }
}

/// 시나리오별 poll outcome 큐 — task_id → VecDeque<PollOutcome>.
struct ScriptedExec {
    polls: HashMap<String, std::collections::VecDeque<PollOutcome>>,
    /// dispatch 시점에 핸들을 task_id 와 매핑 (poll 에서 어느 task 의 outcome 인지 알기 위함).
    handle_to_task: HashMap<u32, String>,
    next_pid: u32,
}

impl ScriptedExec {
    fn new() -> Self {
        Self {
            polls: HashMap::new(),
            handle_to_task: HashMap::new(),
            next_pid: 1,
        }
    }
    fn script(&mut self, task_id: &str, outcomes: Vec<PollOutcome>) {
        self.polls
            .insert(task_id.to_string(), outcomes.into_iter().collect());
    }
}

impl TaskExecutor for ScriptedExec {
    fn dispatch(&mut self, task: &Task) -> DispatchOutcome {
        let pid = self.next_pid;
        self.next_pid += 1;
        self.handle_to_task.insert(pid, task.id.clone());
        DispatchOutcome::Started(DispatchHandle::ShellProcess { pid })
    }
    fn poll(&mut self, handle: &DispatchHandle) -> PollOutcome {
        let pid = match handle {
            DispatchHandle::ShellProcess { pid } => *pid,
            DispatchHandle::ImmediateFail(e) => return PollOutcome::Failed(e.clone()),
            _ => return PollOutcome::Active,
        };
        let task_id = match self.handle_to_task.get(&pid) {
            Some(t) => t.clone(),
            None => return PollOutcome::Active,
        };
        self.polls
            .get_mut(&task_id)
            .and_then(|q| q.pop_front())
            .unwrap_or(PollOutcome::Active)
    }
}

#[test]
fn two_task_chain_propagates_to_downstream() {
    let (_td, mut mem, seq) = fresh_store();

    // ── 1. 2-task DAG 생성: t-a → t-b ──
    let (a_id, b_id) = {
        let mut store = TaskStore::new(&mut mem, "_host", &seq);
        let a = store
            .create(TaskCreateOpts {
                workspace_id: 1,
                name: "a".into(),
                command: run_cmd(),
                depends_on: vec![],
                on_failure: OnFailure::Abort,
                metadata: serde_json::Value::Null,
                now_ms: 1000,
            })
            .unwrap();
        let b = store
            .create(TaskCreateOpts {
                workspace_id: 1,
                name: "b".into(),
                command: run_cmd(),
                depends_on: vec![a.id.clone()],
                on_failure: OnFailure::Abort,
                metadata: serde_json::Value::Null,
                now_ms: 1001,
            })
            .unwrap();
        assert_eq!(a.state, TaskState::Ready);
        assert_eq!(b.state, TaskState::Waiting);
        (a.id, b.id)
    };

    let mut exec = ScriptedExec::new();
    // t-a: dispatch → 1 tick active → Done. t-b: dispatch → Done 즉시.
    exec.script(
        &a_id,
        vec![
            PollOutcome::Active,
            PollOutcome::Done(TaskResult {
                exit_code: Some(0),
                output: None,
                error: None,
            }),
        ],
    );
    exec.script(
        &b_id,
        vec![PollOutcome::Done(TaskResult {
            exit_code: Some(0),
            output: None,
            error: None,
        })],
    );

    let mut runner = RunnerLoop::new(exec);

    // ── 2. tick 1: t-a dispatch → Running. t-b 는 여전히 Waiting. ──
    tick_with_store(&mut runner, &mut mem, &seq, 2000);
    {
        let store = TaskStore::new(&mut mem, "_host", &seq);
        let tasks = store.list(1).unwrap();
        let a = tasks.iter().find(|t| t.id == a_id).unwrap();
        let b = tasks.iter().find(|t| t.id == b_id).unwrap();
        assert_eq!(a.state, TaskState::Running);
        assert_eq!(b.state, TaskState::Waiting);
    }

    // ── 3. tick 2: t-a poll active. 상태 변화 없음. ──
    tick_with_store(&mut runner, &mut mem, &seq, 2500);
    {
        let store = TaskStore::new(&mut mem, "_host", &seq);
        let tasks = store.list(1).unwrap();
        assert_eq!(
            tasks.iter().find(|t| t.id == a_id).unwrap().state,
            TaskState::Running
        );
    }

    // ── 4. tick 3: t-a poll Done → Succeeded → cascade downstream b → Ready. ──
    tick_with_store(&mut runner, &mut mem, &seq, 3000);
    {
        let store = TaskStore::new(&mut mem, "_host", &seq);
        let tasks = store.list(1).unwrap();
        let a = tasks.iter().find(|t| t.id == a_id).unwrap();
        let b = tasks.iter().find(|t| t.id == b_id).unwrap();
        assert_eq!(a.state, TaskState::Succeeded);
        assert_eq!(b.state, TaskState::Ready);
    }

    // ── 5. tick 4: t-b dispatch → Running. ──
    tick_with_store(&mut runner, &mut mem, &seq, 3500);
    {
        let store = TaskStore::new(&mut mem, "_host", &seq);
        let tasks = store.list(1).unwrap();
        assert_eq!(
            tasks.iter().find(|t| t.id == b_id).unwrap().state,
            TaskState::Running
        );
    }

    // ── 6. tick 5: t-b poll Done → Succeeded. ──
    tick_with_store(&mut runner, &mut mem, &seq, 4000);
    {
        let store = TaskStore::new(&mut mem, "_host", &seq);
        let tasks = store.list(1).unwrap();
        assert_eq!(
            tasks.iter().find(|t| t.id == b_id).unwrap().state,
            TaskState::Succeeded
        );
    }
}

/// 한 tick — 실제 runner_thread 의 로직을 단순화한 helper. tick 클로저 두 개가
/// 동시 mutate 못 하므로 `RefCell<Vec<...>>` 로 staging 후 일괄 반영.
fn tick_with_store<E: TaskExecutor>(
    runner: &mut RunnerLoop<E>,
    mem: &mut MemoryStore,
    seq: &AtomicU64,
    now_ms: u64,
) {
    use std::cell::RefCell;
    let snapshot = {
        let store = TaskStore::new(mem, "_host", seq);
        store.list(1).unwrap_or_default()
    };
    let staged: RefCell<Vec<(String, Option<TaskResult>, Option<TaskState>)>> =
        RefCell::new(Vec::new());
    runner.tick(
        1,
        now_ms,
        &snapshot,
        |_ws, id, st, _now| {
            let mut s = staged.borrow_mut();
            match s.iter_mut().find(|(i, _, _)| i == id) {
                Some((_, _, slot)) => *slot = Some(st),
                None => s.push((id.clone(), None, Some(st))),
            }
            Ok(())
        },
        |_ws, id, r| {
            let mut s = staged.borrow_mut();
            match s.iter_mut().find(|(i, _, _)| i == id) {
                Some((_, slot, _)) => *slot = Some(r),
                None => s.push((id.clone(), Some(r), None)),
            }
            Ok(())
        },
    );
    let mut store = TaskStore::new(mem, "_host", seq);
    for (id, r_opt, st_opt) in staged.into_inner() {
        if let Some(r) = r_opt {
            let _ = store.set_result(1, &id, r); // test helper — 시나리오 외 에러는 무시
        }
        if let Some(st) = st_opt {
            let _ = store.set_state(1, &id, st, now_ms); // test helper — 시나리오 외 에러는 무시
        }
    }
}
