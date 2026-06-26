//! Runner 통합 테스트 — Real TaskStore + Mock executor 로 ready → running →
//! succeeded → downstream ready 까지 검증.
// 테스트 fixture 의 mock 채널/큐 타입이 깊게 중첩되지만 테스트 한정
// 가독성 문제라 alias 도입 가치가 낮다 — 파일 단위 허용 (TODO 17 결정 B).
#![allow(clippy::type_complexity)]

use std::collections::HashMap;
use std::sync::atomic::AtomicU64;

use tasty_agent::runner::{DispatchHandle, DispatchOutcome, PollOutcome, RunnerLoop, TaskExecutor};
use tasty_agent::task::TaskCreateOpts;
use tasty_agent::{
    BarrierState, BarrierStore, LeaseMode, LeaseStore, OnFailure, SemaphoreStore, Task,
    TaskCommand, TaskId, TaskResult, TaskState, TaskStore,
};
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

// =====================================================================
// Phase I.A.S6 — semaphore-gated dispatch + WaitBarrier 통합 시나리오.
// HostExecutor 가 본 crate 외부에 있으므로, *test-local* SemaphoreAwareExec /
// BarrierAwareExec 가 동일한 규약 (metadata.semaphore 컨벤션, BarrierPoll handle)
// 을 사용해 runner 와 store 의 상호작용을 검증한다.
// =====================================================================

use std::cell::RefCell;
use std::rc::Rc;

/// metadata.semaphore 를 읽어 SemaphoreStore::acquire/release 를 호출하는 executor.
/// 점유 부족 시 Deferred, 점유 성공 후 ShellProcess handle 로 dispatch 시뮬레이션.
/// poll 은 미리 스크립트된 outcome 큐를 따른다.
struct SemaphoreAwareExec {
    mem: Rc<RefCell<MemoryStore>>,
    polls: HashMap<String, std::collections::VecDeque<PollOutcome>>,
    handle_to_task: HashMap<u32, String>,
    held_permits: HashMap<TaskId, (u32, String, String)>,
    next_pid: u32,
}

impl SemaphoreAwareExec {
    fn new(mem: Rc<RefCell<MemoryStore>>) -> Self {
        Self {
            mem,
            polls: HashMap::new(),
            handle_to_task: HashMap::new(),
            held_permits: HashMap::new(),
            next_pid: 1,
        }
    }
    fn script(&mut self, task_id: &str, outcomes: Vec<PollOutcome>) {
        self.polls
            .insert(task_id.to_string(), outcomes.into_iter().collect());
    }
}

impl TaskExecutor for SemaphoreAwareExec {
    fn dispatch(&mut self, task: &Task) -> DispatchOutcome {
        if let Some(meta) = task.metadata.get("semaphore").and_then(|v| v.as_object()) {
            let name = match meta.get("name").and_then(|v| v.as_str()) {
                Some(n) => n.to_string(),
                None => return DispatchOutcome::PermanentFail("missing semaphore.name".into()),
            };
            let holder = meta
                .get("holder")
                .and_then(|v| v.as_str())
                .unwrap_or(task.id.as_str())
                .to_string();
            let mut mem = self.mem.borrow_mut();
            let mut store = SemaphoreStore::new(&mut *mem, "_host");
            let outcome = match store.acquire(task.workspace_id, &name, &holder) {
                Ok(o) => o,
                Err(e) => return DispatchOutcome::PermanentFail(format!("semaphore: {e}")),
            };
            if !outcome.acquired {
                return DispatchOutcome::Deferred;
            }
            self.held_permits
                .insert(task.id.clone(), (task.workspace_id, name, holder));
        }
        let pid = self.next_pid;
        self.next_pid += 1;
        self.handle_to_task.insert(pid, task.id.clone());
        DispatchOutcome::Started(DispatchHandle::ShellProcess { pid })
    }

    fn poll(&mut self, handle: &DispatchHandle) -> PollOutcome {
        let pid = match handle {
            DispatchHandle::ShellProcess { pid } => *pid,
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

    fn release_permit(&mut self, task_id: &TaskId) {
        if let Some((ws, name, holder)) = self.held_permits.remove(task_id) {
            let mut mem = self.mem.borrow_mut();
            let mut store = SemaphoreStore::new(&mut *mem, "_host");
            let _ = store.release(ws, &name, &holder); // idempotent
        }
    }
}

/// SemaphoreAwareExec 용 한 tick helper — Rc<RefCell<MemoryStore>> 기반.
fn tick_semaphore_exec(
    runner: &mut RunnerLoop<SemaphoreAwareExec>,
    mem: &Rc<RefCell<MemoryStore>>,
    seq: &AtomicU64,
    now_ms: u64,
) {
    let snapshot = {
        let mut m = mem.borrow_mut();
        TaskStore::new(&mut *m, "_host", seq)
            .list(1)
            .unwrap_or_default()
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
    let mut m = mem.borrow_mut();
    let mut store = TaskStore::new(&mut *m, "_host", seq);
    for (id, r_opt, st_opt) in staged.into_inner() {
        if let Some(r) = r_opt {
            let _ = store.set_result(1, &id, r); // test helper — set 외 에러는 무시
        }
        if let Some(st) = st_opt {
            let _ = store.set_state(1, &id, st, now_ms); // test helper — set 외 에러는 무시
        }
    }
}

#[test]
fn semaphore_gated_dispatch_serializes_two_tasks() {
    let td = tempfile::tempdir().expect("tempdir");
    let mem = MemoryStore::open(&td.path().join("mem.db")).expect("mem");
    let mem = Rc::new(RefCell::new(mem));
    let seq = AtomicU64::new(0);

    // semaphore permits=1.
    {
        let mut m = mem.borrow_mut();
        let mut s = SemaphoreStore::new(&mut *m, "_host");
        s.create(1, "g", 1, 1000).unwrap();
    }

    // 2 task: t-1, t-2 모두 semaphore=g, holder=task.id.
    let (id1, id2) = {
        let mut m = mem.borrow_mut();
        let mut store = TaskStore::new(&mut *m, "_host", &seq);
        let meta1 = serde_json::json!({ "semaphore": { "name": "g" } });
        let meta2 = serde_json::json!({ "semaphore": { "name": "g" } });
        let t1 = store
            .create(TaskCreateOpts {
                workspace_id: 1,
                name: "t1".into(),
                command: run_cmd(),
                depends_on: vec![],
                on_failure: OnFailure::Abort,
                metadata: meta1,
                now_ms: 1100,
            })
            .unwrap();
        let t2 = store
            .create(TaskCreateOpts {
                workspace_id: 1,
                name: "t2".into(),
                command: run_cmd(),
                depends_on: vec![],
                on_failure: OnFailure::Abort,
                metadata: meta2,
                now_ms: 1101,
            })
            .unwrap();
        (t1.id, t2.id)
    };

    // holder 컨벤션을 task.id 로 set — TaskCreateOpts 에 미리 못 박으므로 update.
    {
        let mut m = mem.borrow_mut();
        let mut store = TaskStore::new(&mut *m, "_host", &seq);
        for tid in [&id1, &id2] {
            let mut t = store.get(1, tid).unwrap().unwrap();
            t.metadata = serde_json::json!({ "semaphore": { "name": "g", "holder": tid } });
            store.put(&t).unwrap();
        }
    }

    let mut exec = SemaphoreAwareExec::new(mem.clone());
    // t-1 의 poll: 1 tick Active, 그다음 Done.
    exec.script(
        &id1,
        vec![PollOutcome::Done(TaskResult {
            exit_code: Some(0),
            output: None,
            error: None,
        })],
    );
    exec.script(
        &id2,
        vec![PollOutcome::Done(TaskResult {
            exit_code: Some(0),
            output: None,
            error: None,
        })],
    );

    let mut runner = RunnerLoop::new(exec);

    // tick 1: t-1 acquire 성공 → Running. t-2 는 acquire 실패 → Ready 유지 (Deferred).
    tick_semaphore_exec(&mut runner, &mem, &seq, 2000);
    {
        let mut m = mem.borrow_mut();
        let store = TaskStore::new(&mut *m, "_host", &seq);
        let t1 = store.get(1, &id1).unwrap().unwrap();
        let t2 = store.get(1, &id2).unwrap().unwrap();
        assert_eq!(t1.state, TaskState::Running, "t1 acquired permit");
        assert_eq!(t2.state, TaskState::Ready, "t2 deferred");
    }

    // tick 2: t-1 poll → Done → Succeeded → release_permit (Running arm). 같은 tick 의
    // Ready arm 에서 t-2 가 새로 release 된 permit 을 acquire → Started → Running.
    tick_semaphore_exec(&mut runner, &mem, &seq, 2500);
    {
        let mut m = mem.borrow_mut();
        let store = TaskStore::new(&mut *m, "_host", &seq);
        let t1 = store.get(1, &id1).unwrap().unwrap();
        let t2 = store.get(1, &id2).unwrap().unwrap();
        assert_eq!(t1.state, TaskState::Succeeded);
        assert_eq!(t2.state, TaskState::Running, "t2 acquired after release");
    }

    // tick 3: t-2 poll → Done → Succeeded.
    tick_semaphore_exec(&mut runner, &mem, &seq, 3000);
    {
        let mut m = mem.borrow_mut();
        let store = TaskStore::new(&mut *m, "_host", &seq);
        let t2 = store.get(1, &id2).unwrap().unwrap();
        assert_eq!(t2.state, TaskState::Succeeded);
    }
}

/// BarrierPoll handle 을 처리하는 test-local executor — WaitBarrier task 검증.
struct BarrierAwareExec {
    mem: Rc<RefCell<MemoryStore>>,
}

impl TaskExecutor for BarrierAwareExec {
    fn dispatch(&mut self, task: &Task) -> DispatchOutcome {
        match &task.command {
            TaskCommand::WaitBarrier { name } => {
                DispatchOutcome::Started(DispatchHandle::BarrierPoll {
                    workspace_id: task.workspace_id,
                    name: name.clone(),
                })
            }
            _ => DispatchOutcome::PermanentFail("unsupported in test".into()),
        }
    }
    fn poll(&mut self, handle: &DispatchHandle) -> PollOutcome {
        match handle {
            DispatchHandle::BarrierPoll { workspace_id, name } => {
                let mut m = self.mem.borrow_mut();
                let mut store = BarrierStore::new(&mut *m, "_host");
                match store.state(*workspace_id, name, 9_999_000) {
                    Ok(b) => match b.state {
                        BarrierState::Open => PollOutcome::Active,
                        BarrierState::Closed => PollOutcome::Done(TaskResult {
                            exit_code: Some(0),
                            output: None,
                            error: None,
                        }),
                        BarrierState::TimedOut => {
                            PollOutcome::Failed(format!("barrier '{name}' timed out"))
                        }
                    },
                    Err(e) => PollOutcome::Failed(format!("barrier: {e}")),
                }
            }
            _ => PollOutcome::Active,
        }
    }
}

fn tick_barrier_exec(
    runner: &mut RunnerLoop<BarrierAwareExec>,
    mem: &Rc<RefCell<MemoryStore>>,
    seq: &AtomicU64,
    now_ms: u64,
) {
    let snapshot = {
        let mut m = mem.borrow_mut();
        TaskStore::new(&mut *m, "_host", seq)
            .list(1)
            .unwrap_or_default()
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
    let mut m = mem.borrow_mut();
    let mut store = TaskStore::new(&mut *m, "_host", seq);
    for (id, r_opt, st_opt) in staged.into_inner() {
        if let Some(r) = r_opt {
            let _ = store.set_result(1, &id, r); // test helper — set 외 에러는 무시
        }
        if let Some(st) = st_opt {
            let _ = store.set_state(1, &id, st, now_ms); // test helper — set 외 에러는 무시
        }
    }
}

#[test]
fn wait_barrier_task_succeeds_after_signals() {
    let td = tempfile::tempdir().expect("tempdir");
    let mem = MemoryStore::open(&td.path().join("mem.db")).expect("mem");
    let mem = Rc::new(RefCell::new(mem));
    let seq = AtomicU64::new(0);

    // barrier(b, count_required=2) 생성.
    {
        let mut m = mem.borrow_mut();
        let mut s = BarrierStore::new(&mut *m, "_host");
        s.create(1, "b", 2, None, 1000).unwrap();
    }

    let wb_id = {
        let mut m = mem.borrow_mut();
        let mut store = TaskStore::new(&mut *m, "_host", &seq);
        store
            .create(TaskCreateOpts {
                workspace_id: 1,
                name: "wb".into(),
                command: TaskCommand::WaitBarrier { name: "b".into() },
                depends_on: vec![],
                on_failure: OnFailure::Abort,
                metadata: serde_json::Value::Null,
                now_ms: 1100,
            })
            .unwrap()
            .id
    };

    let exec = BarrierAwareExec { mem: mem.clone() };
    let mut runner = RunnerLoop::new(exec);

    // tick 1: dispatch → BarrierPoll handle, state Open → Running.
    tick_barrier_exec(&mut runner, &mem, &seq, 2000);
    {
        let mut m = mem.borrow_mut();
        let store = TaskStore::new(&mut *m, "_host", &seq);
        assert_eq!(
            store.get(1, &wb_id).unwrap().unwrap().state,
            TaskState::Running
        );
    }

    // tick 2: 아직 signal 안 됨 → Open → Active → Running 유지.
    tick_barrier_exec(&mut runner, &mem, &seq, 2500);
    {
        let mut m = mem.borrow_mut();
        let store = TaskStore::new(&mut *m, "_host", &seq);
        assert_eq!(
            store.get(1, &wb_id).unwrap().unwrap().state,
            TaskState::Running
        );
    }

    // signal 2회 — barrier 가 Closed 로.
    {
        let mut m = mem.borrow_mut();
        let mut s = BarrierStore::new(&mut *m, "_host");
        s.signal(1, "b", 2600).unwrap();
        s.signal(1, "b", 2601).unwrap();
    }

    // tick 3: poll → Closed → Done → Succeeded.
    tick_barrier_exec(&mut runner, &mem, &seq, 3000);
    {
        let mut m = mem.borrow_mut();
        let store = TaskStore::new(&mut *m, "_host", &seq);
        assert_eq!(
            store.get(1, &wb_id).unwrap().unwrap().state,
            TaskState::Succeeded
        );
    }
}

// =====================================================================
// Phase J.A.S1 — lease-gated dispatch 통합 시나리오.
// SemaphoreAwareExec 의 lease 변형. dispatch 시 metadata.lease 의
// resource/holder 로 LeaseStore::acquire 호출. block 모드 + 점유 충돌 시 Deferred.
// =====================================================================

struct LeaseAwareExec {
    mem: Rc<RefCell<MemoryStore>>,
    polls: HashMap<String, std::collections::VecDeque<PollOutcome>>,
    handle_to_task: HashMap<u32, String>,
    held_leases: HashMap<TaskId, (u32, String, String)>,
    next_pid: u32,
}

impl LeaseAwareExec {
    fn new(mem: Rc<RefCell<MemoryStore>>) -> Self {
        Self {
            mem,
            polls: HashMap::new(),
            handle_to_task: HashMap::new(),
            held_leases: HashMap::new(),
            next_pid: 1,
        }
    }
    fn script(&mut self, task_id: &str, outcomes: Vec<PollOutcome>) {
        self.polls
            .insert(task_id.to_string(), outcomes.into_iter().collect());
    }
}

impl TaskExecutor for LeaseAwareExec {
    fn dispatch(&mut self, task: &Task) -> DispatchOutcome {
        if let Some(meta) = task.metadata.get("lease").and_then(|v| v.as_object()) {
            let resource = match meta.get("resource").and_then(|v| v.as_str()) {
                Some(r) => r.to_string(),
                None => return DispatchOutcome::PermanentFail("missing lease.resource".into()),
            };
            let holder = meta
                .get("holder")
                .and_then(|v| v.as_str())
                .unwrap_or(task.id.as_str())
                .to_string();
            let mut mem = self.mem.borrow_mut();
            let mut store = LeaseStore::new(&mut *mem, "_host");
            let outcome = match store.acquire(
                task.workspace_id,
                &resource,
                &holder,
                None,
                LeaseMode::Block,
                0,
            ) {
                Ok(o) => o,
                Err(e) => return DispatchOutcome::PermanentFail(format!("lease: {e}")),
            };
            if !outcome.acquired {
                return DispatchOutcome::Deferred;
            }
            self.held_leases
                .insert(task.id.clone(), (task.workspace_id, resource, holder));
        }
        let pid = self.next_pid;
        self.next_pid += 1;
        self.handle_to_task.insert(pid, task.id.clone());
        DispatchOutcome::Started(DispatchHandle::ShellProcess { pid })
    }

    fn poll(&mut self, handle: &DispatchHandle) -> PollOutcome {
        let pid = match handle {
            DispatchHandle::ShellProcess { pid } => *pid,
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

    fn release_permit(&mut self, task_id: &TaskId) {
        if let Some((ws, resource, holder)) = self.held_leases.remove(task_id) {
            let mut mem = self.mem.borrow_mut();
            let mut store = LeaseStore::new(&mut *mem, "_host");
            let _ = store.release(ws, &resource, &holder); // idempotent
        }
    }
}

fn tick_lease_exec(
    runner: &mut RunnerLoop<LeaseAwareExec>,
    mem: &Rc<RefCell<MemoryStore>>,
    seq: &AtomicU64,
    now_ms: u64,
) {
    let snapshot = {
        let mut m = mem.borrow_mut();
        TaskStore::new(&mut *m, "_host", seq)
            .list(1)
            .unwrap_or_default()
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
    let mut m = mem.borrow_mut();
    let mut store = TaskStore::new(&mut *m, "_host", seq);
    for (id, r_opt, st_opt) in staged.into_inner() {
        if let Some(r) = r_opt {
            let _ = store.set_result(1, &id, r); // test helper — set 외 에러는 무시
        }
        if let Some(st) = st_opt {
            let _ = store.set_state(1, &id, st, now_ms); // test helper — set 외 에러는 무시
        }
    }
}

#[test]
fn lease_gated_dispatch_serializes_two_tasks() {
    let td = tempfile::tempdir().expect("tempdir");
    let mem = MemoryStore::open(&td.path().join("mem.db")).expect("mem");
    let mem = Rc::new(RefCell::new(mem));
    let seq = AtomicU64::new(0);

    // 2 task: t-1, t-2 모두 lease=file:/shared, holder=task.id.
    let (id1, id2) = {
        let mut m = mem.borrow_mut();
        let mut store = TaskStore::new(&mut *m, "_host", &seq);
        let meta1 = serde_json::json!({ "lease": { "resource": "file:/shared" } });
        let meta2 = serde_json::json!({ "lease": { "resource": "file:/shared" } });
        let t1 = store
            .create(TaskCreateOpts {
                workspace_id: 1,
                name: "t1".into(),
                command: run_cmd(),
                depends_on: vec![],
                on_failure: OnFailure::Abort,
                metadata: meta1,
                now_ms: 1100,
            })
            .unwrap();
        let t2 = store
            .create(TaskCreateOpts {
                workspace_id: 1,
                name: "t2".into(),
                command: run_cmd(),
                depends_on: vec![],
                on_failure: OnFailure::Abort,
                metadata: meta2,
                now_ms: 1101,
            })
            .unwrap();
        (t1.id, t2.id)
    };

    // holder 컨벤션을 task.id 로 set.
    {
        let mut m = mem.borrow_mut();
        let mut store = TaskStore::new(&mut *m, "_host", &seq);
        for tid in [&id1, &id2] {
            let mut t = store.get(1, tid).unwrap().unwrap();
            t.metadata = serde_json::json!({
                "lease": { "resource": "file:/shared", "holder": tid }
            });
            store.put(&t).unwrap();
        }
    }

    let mut exec = LeaseAwareExec::new(mem.clone());
    exec.script(
        &id1,
        vec![PollOutcome::Done(TaskResult {
            exit_code: Some(0),
            output: None,
            error: None,
        })],
    );
    exec.script(
        &id2,
        vec![PollOutcome::Done(TaskResult {
            exit_code: Some(0),
            output: None,
            error: None,
        })],
    );

    let mut runner = RunnerLoop::new(exec);

    // tick 1: t-1 acquire 성공 → Running. t-2 는 Block 모드 → acquired=false → Deferred.
    tick_lease_exec(&mut runner, &mem, &seq, 2000);
    {
        let mut m = mem.borrow_mut();
        let store = TaskStore::new(&mut *m, "_host", &seq);
        let t1 = store.get(1, &id1).unwrap().unwrap();
        let t2 = store.get(1, &id2).unwrap().unwrap();
        assert_eq!(t1.state, TaskState::Running, "t1 acquired lease");
        assert_eq!(t2.state, TaskState::Ready, "t2 deferred");
    }

    // tick 2: t-1 poll → Done → Succeeded → release_lease. 같은 tick 의 Ready arm 에서
    // t-2 가 새로 release 된 lease 를 acquire → Running.
    tick_lease_exec(&mut runner, &mem, &seq, 2500);
    {
        let mut m = mem.borrow_mut();
        let store = TaskStore::new(&mut *m, "_host", &seq);
        let t1 = store.get(1, &id1).unwrap().unwrap();
        let t2 = store.get(1, &id2).unwrap().unwrap();
        assert_eq!(t1.state, TaskState::Succeeded);
        assert_eq!(t2.state, TaskState::Running, "t2 acquired after release");
    }

    // tick 3: t-2 poll → Done → Succeeded.
    tick_lease_exec(&mut runner, &mem, &seq, 3000);
    {
        let mut m = mem.borrow_mut();
        let store = TaskStore::new(&mut *m, "_host", &seq);
        let t2 = store.get(1, &id2).unwrap().unwrap();
        assert_eq!(t2.state, TaskState::Succeeded);
    }
}

// =====================================================================
// Phase J.A.S6 — DispatchHandle 영속 round-trip 통합 시나리오.
// runner_host / runner_thread 는 src/ (host adapter) 에 있으므로 여기서는
// pure tasty-agent 측에서 DispatchHandle::serde 의 forward/backward-compat 와
// 모든 variant 의 영속/복원 의미를 검증한다.
// =====================================================================

#[test]
fn dispatch_handle_persistence_round_trip_all_variants() {
    use tasty_agent::DispatchHandle;
    let variants = vec![
        DispatchHandle::PolledDispatch {
            workspace_id: 1,
            poll_method: "fake.poll".into(),
            poll_params: serde_json::json!({ "surface_id": 42, "child_index": 3 }),
            state_field: "state".into(),
            terminal_states: vec!["idle".into(), "needs_input".into(), "exited".into()],
            interval_ms: 500,
            deadline_ms: None,
        },
        DispatchHandle::ShellProcess { pid: 99999 },
        DispatchHandle::ReduceImmediate(TaskResult {
            exit_code: Some(0),
            output: Some(serde_json::json!({"x": 1})),
            error: None,
        }),
        DispatchHandle::CustomImmediate(TaskResult {
            exit_code: Some(0),
            output: None,
            error: None,
        }),
        DispatchHandle::ImmediateFail("dispatch error".into()),
        DispatchHandle::BarrierPoll {
            workspace_id: 1,
            name: "b".into(),
        },
    ];
    for h in variants {
        let json = serde_json::to_string(&h).expect("serialize");
        let back: DispatchHandle = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(
            format!("{h:?}"),
            format!("{back:?}"),
            "round-trip mismatch for {h:?}"
        );
    }
}
