//! Task runner — Ready task 를 dispatch 하고 Running task 를 poll 하는 순수 루프.
//!
//! 본 모듈은 *IPC / GUI / OS 와 독립적*. 실제 host-side 동작 (claude.spawn,
//! shell process, Custom IPC dispatch) 은 [`TaskExecutor`] trait 의 host 측
//! 구현에 위임한다 — 테스트는 mock executor 로 검증.
//!
//! 한 번의 `tick()` 호출이 다음을 수행:
//! 1. Running task 에 대해 `executor.poll(handle)` — Done / Failed 면 set_result
//!    + set_state(Succeeded/Failed) 로 종결.
//! 2. Ready task 에 대해 `executor.dispatch(task)` — handle 보관 + set_state(Running).
//!
//! 호출자는 polling interval (500ms 권장) 마다 tick 을 호출. 호스트 재시작 시
//! handle 은 유실 (R3 의 in-memory 정책) — 재시작 후 Running 잔여 task 는
//! Unknown 으로 처리되어 사용자 retry 가 필요.

use std::collections::HashMap;

use crate::task::{Task, TaskId, TaskResult, TaskState};
use crate::{AgentError, Result};

/// dispatch / poll 결과를 묶는 핸들. variant 별 의미:
/// - `ClaudeChild`: parent surface 와 child_index 보관. poll 은 `claude.wait` 결과로.
/// - `ShellProcess`: pid 만. host executor 가 `Child` 객체를 별 map 으로 보관.
/// - `ReduceImmediate` / `CustomImmediate`: dispatch 시점에 결과가 즉시 결정.
/// - `ImmediateFail`: dispatch 가 실패. transition 표상 Ready → Failed 직접 전이가
///   불허이므로 *먼저 Running 으로 보낸 후* poll 결과로 Failed 로 흡수하기 위한
///   우회 variant.
#[derive(Debug, Clone)]
pub enum DispatchHandle {
    ClaudeChild {
        parent_sid: u32,
        child_index: u32,
        workspace_id: u32,
    },
    ShellProcess {
        pid: u32,
    },
    ReduceImmediate(TaskResult),
    CustomImmediate(TaskResult),
    ImmediateFail(String),
}

/// poll 결과.
#[derive(Debug, Clone)]
pub enum PollOutcome {
    Active,
    Done(TaskResult),
    Failed(String),
}

/// dispatch 의 3-way 결과.
///
/// - `Started(h)` — dispatch 성공, Ready → Running 전이 + handle 보관.
/// - `Deferred` — 이번 tick 의 dispatch 불가 (예: semaphore permit 미점유).
///   state 전이 X, 다음 tick 재시도.
/// - `PermanentFail(e)` — 즉시 실패. 기존 `Err(String)` 흡수 — `ImmediateFail` handle
///   로 wrapping 되어 다음 tick poll 에서 Failed 로 흡수된다.
#[derive(Debug)]
pub enum DispatchOutcome {
    Started(DispatchHandle),
    Deferred,
    PermanentFail(String),
}

/// host 가 구현하는 task 실행기. dispatch 는 *비차단*, poll 은 *비차단 1tick*.
pub trait TaskExecutor {
    /// task 의 command 를 분석해 실제 실행을 시작.
    fn dispatch(&mut self, task: &Task) -> DispatchOutcome;
    /// 핸들의 현재 상태를 1 tick 확인.
    fn poll(&mut self, handle: &DispatchHandle) -> PollOutcome;
    /// task 가 종결(Succeeded/Failed/Cancelled) 됐을 때 permit 등을 해제.
    /// 기본 구현은 no-op — semaphore 통합이 없는 executor 는 override 불필요.
    fn release_permit(&mut self, _task_id: &TaskId) {}
}

/// `TaskExecutor` 를 들고 task list 를 진행시키는 루프. workspace 1개당 1 instance.
pub struct RunnerLoop<E: TaskExecutor> {
    pub executor: E,
    pub running: HashMap<TaskId, DispatchHandle>,
}

impl<E: TaskExecutor> RunnerLoop<E> {
    pub fn new(executor: E) -> Self {
        Self {
            executor,
            running: HashMap::new(),
        }
    }

    /// 한 tick. state 전이는 `set_state`/`set_result` 클로저로 호출자가 위임.
    ///
    /// `tasks`: 현재 workspace 의 task snapshot (보통 직전 `task_list` 결과).
    /// `set_state`: `(workspace_id, &task_id, new_state, now_ms)` → `Result<(), AgentError>`.
    /// `set_result`: `(workspace_id, &task_id, result)` → `Result<(), AgentError>`.
    ///
    /// 에러는 tracing::warn 으로 흡수하고 다음 task 로 진행 (runner thread 가 죽지
    /// 않게 — 사용자가 cancel/retry 로 정리 가능).
    pub fn tick<FS, FR>(
        &mut self,
        workspace_id: u32,
        now_ms: u64,
        tasks: &[Task],
        mut set_state: FS,
        mut set_result: FR,
    ) where
        FS: FnMut(u32, &TaskId, TaskState, u64) -> Result<()>,
        FR: FnMut(u32, &TaskId, TaskResult) -> Result<()>,
    {
        // 0. Cancelled 흡수 — `agent.task_cancel` 이 외부에서 store 의 Running →
        //    Cancelled 로 직접 전이시켰을 수 있다. handle 정리 + permit 해제.
        for task in tasks {
            if !matches!(task.state, TaskState::Cancelled) {
                continue;
            }
            if self.running.remove(&task.id).is_some() {
                self.executor.release_permit(&task.id);
            }
        }

        // 1. Running 처리.
        for task in tasks {
            if !matches!(task.state, TaskState::Running) {
                continue;
            }
            let handle = match self.running.get(&task.id) {
                Some(h) => h.clone(),
                None => {
                    // handle 유실 — 호스트 재시작 후 Running 잔여 task 의 경우.
                    // R3 정책: 그대로 둠. 사용자가 retry 로 정리.
                    continue;
                }
            };
            match self.executor.poll(&handle) {
                PollOutcome::Active => {}
                PollOutcome::Done(result) => {
                    log_err(set_result(workspace_id, &task.id, result), &task.id);
                    log_err(
                        set_state(workspace_id, &task.id, TaskState::Succeeded, now_ms),
                        &task.id,
                    );
                    self.running.remove(&task.id);
                    self.executor.release_permit(&task.id);
                }
                PollOutcome::Failed(err) => {
                    log_err(
                        set_result(
                            workspace_id,
                            &task.id,
                            TaskResult {
                                exit_code: None,
                                output: None,
                                error: Some(err.clone()),
                            },
                        ),
                        &task.id,
                    );
                    log_err(
                        set_state(
                            workspace_id,
                            &task.id,
                            TaskState::Failed { error: err },
                            now_ms,
                        ),
                        &task.id,
                    );
                    self.running.remove(&task.id);
                    self.executor.release_permit(&task.id);
                }
            }
        }

        // 2. Ready 처리 — dispatch + Ready → Running 전이.
        //    Deferred 는 state 전이 없이 다음 tick 으로 미룬다 (semaphore 미점유 등).
        //    PermanentFail 은 ImmediateFail 핸들로 wrapping → 다음 tick poll 에서 Failed 흡수.
        for task in tasks {
            if !matches!(task.state, TaskState::Ready) {
                continue;
            }
            // 이미 dispatch 한 적이 있는지 (예: 같은 tick 안에 이전 루프) — 방지.
            if self.running.contains_key(&task.id) {
                continue;
            }
            let handle = match self.executor.dispatch(task) {
                DispatchOutcome::Started(h) => h,
                DispatchOutcome::Deferred => continue,
                DispatchOutcome::PermanentFail(e) => DispatchHandle::ImmediateFail(e),
            };
            // Ready → Running 전이 후에만 handle 보관 (전이 실패 시 보관 X).
            match set_state(workspace_id, &task.id, TaskState::Running, now_ms) {
                Ok(()) => {
                    self.running.insert(task.id.clone(), handle);
                }
                Err(e) => {
                    tracing::warn!("set_state(Running) for {} failed: {e}", task.id);
                }
            }
        }
    }
}

fn log_err<T>(r: Result<T>, task_id: &str) {
    if let Err(e) = r {
        tracing::warn!("runner: {task_id} state op failed: {e}");
    }
}

// ImmediateFail / Immediate handle 의 poll 결과는 host 측 executor 가 매핑하지만,
// pure 로직 테스트 편의를 위해 본 모듈은 trait 만 노출하고 매핑은 위임한다.
// 대신 ImmediateFail 가 어떻게 처리돼야 하는지는 host executor 의 의무로 — 본
// trait 의 `poll` 이 ImmediateFail handle 을 받으면 PollOutcome::Failed 를 반환
// 해야 한다 (host 측 구현 명세).
#[allow(dead_code)]
fn _agent_error_link(_e: AgentError) {}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::task::{Task, TaskCommand};

    /// Mock executor: dispatch 카운터 + poll 결과 시나리오 큐.
    struct MockExec {
        dispatched: Vec<TaskId>,
        /// task_id -> 미리 정해진 poll outcome 시퀀스.
        polls: HashMap<TaskId, std::collections::VecDeque<PollOutcome>>,
    }

    impl MockExec {
        fn new() -> Self {
            Self {
                dispatched: Vec::new(),
                polls: HashMap::new(),
            }
        }
        fn enq_poll(&mut self, task_id: &str, o: PollOutcome) {
            self.polls
                .entry(task_id.to_string())
                .or_default()
                .push_back(o);
        }
    }

    impl TaskExecutor for MockExec {
        fn dispatch(&mut self, task: &Task) -> DispatchOutcome {
            self.dispatched.push(task.id.clone());
            // 단순 ShellProcess 핸들로 가짜.
            DispatchOutcome::Started(DispatchHandle::ShellProcess { pid: 0 })
        }
        fn poll(&mut self, handle: &DispatchHandle) -> PollOutcome {
            if let DispatchHandle::ImmediateFail(e) = handle {
                return PollOutcome::Failed(e.clone());
            }
            // ShellProcess 만 사용 — task id 와 핸들 매핑은 호출자가 별도 관리해야
            // 하지만 본 테스트는 큐가 1개 task 만 다룬다 가정 → 임의 first.
            for (_id, q) in self.polls.iter_mut() {
                if let Some(o) = q.pop_front() {
                    return o;
                }
            }
            PollOutcome::Active
        }
    }

    fn mk_task(id: &str, deps: &[&str]) -> Task {
        Task {
            id: id.to_string(),
            workspace_id: 1,
            name: id.to_string(),
            command: TaskCommand::Run {
                command: vec!["true".into()],
                workspace_id: 1,
                cwd: None as Option<PathBuf>,
            },
            depends_on: deps.iter().map(|s| s.to_string()).collect(),
            state: TaskState::Ready,
            created_at: 0,
            started_at: None,
            finished_at: None,
            result: None,
            on_failure: Default::default(),
            metadata: Default::default(),
        }
    }

    /// 단순 mock store — Vec<Task> 기반. set_state/set_result 클로저가 본 store 를 mutate.
    struct MockStore {
        tasks: Vec<Task>,
    }
    impl MockStore {
        fn set_state(
            &mut self,
            _ws: u32,
            id: &TaskId,
            new_state: TaskState,
            _now: u64,
        ) -> Result<()> {
            for t in self.tasks.iter_mut() {
                if &t.id == id {
                    t.state = new_state;
                    return Ok(());
                }
            }
            Err(AgentError::TaskNotFound(id.clone()))
        }
        fn set_result(&mut self, _ws: u32, id: &TaskId, r: TaskResult) -> Result<()> {
            for t in self.tasks.iter_mut() {
                if &t.id == id {
                    t.result = Some(r);
                    return Ok(());
                }
            }
            Err(AgentError::TaskNotFound(id.clone()))
        }
    }

    #[test]
    fn ready_task_dispatches_and_running() {
        let mut runner = RunnerLoop::new(MockExec::new());
        let store = std::cell::RefCell::new(MockStore {
            tasks: vec![mk_task("t-1", &[])],
        });

        let snapshot = store.borrow().tasks.clone();
        runner.tick(
            1,
            100,
            &snapshot,
            |ws, id, st, now| store.borrow_mut().set_state(ws, id, st, now),
            |ws, id, r| store.borrow_mut().set_result(ws, id, r),
        );

        let tasks = &store.borrow().tasks;
        assert_eq!(tasks[0].state, TaskState::Running);
        assert!(runner.running.contains_key("t-1"));
    }

    #[test]
    fn running_task_done_transitions_to_succeeded() {
        let mut exec = MockExec::new();
        exec.enq_poll(
            "t-1",
            PollOutcome::Done(TaskResult {
                exit_code: Some(0),
                output: None,
                error: None,
            }),
        );
        let mut runner = RunnerLoop::new(exec);
        // 직접 running map 에 핸들 박고 task state 도 Running 으로 설정.
        runner
            .running
            .insert("t-1".to_string(), DispatchHandle::ShellProcess { pid: 0 });
        let store = std::cell::RefCell::new(MockStore {
            tasks: vec![Task {
                state: TaskState::Running,
                ..mk_task("t-1", &[])
            }],
        });

        let snapshot = store.borrow().tasks.clone();
        runner.tick(
            1,
            200,
            &snapshot,
            |ws, id, st, now| store.borrow_mut().set_state(ws, id, st, now),
            |ws, id, r| store.borrow_mut().set_result(ws, id, r),
        );

        let tasks = &store.borrow().tasks;
        assert_eq!(tasks[0].state, TaskState::Succeeded);
        assert!(tasks[0].result.is_some());
        assert!(!runner.running.contains_key("t-1"));
    }

    #[test]
    fn immediate_fail_dispatch_is_absorbed_next_tick() {
        struct FailExec;
        impl TaskExecutor for FailExec {
            fn dispatch(&mut self, _t: &Task) -> DispatchOutcome {
                DispatchOutcome::PermanentFail("dispatch error".to_string())
            }
            fn poll(&mut self, h: &DispatchHandle) -> PollOutcome {
                match h {
                    DispatchHandle::ImmediateFail(e) => PollOutcome::Failed(e.clone()),
                    _ => PollOutcome::Active,
                }
            }
        }
        let mut runner = RunnerLoop::new(FailExec);
        let store = std::cell::RefCell::new(MockStore {
            tasks: vec![mk_task("t-1", &[])],
        });

        // tick 1: dispatch Err → ImmediateFail handle + Ready→Running.
        let snap1 = store.borrow().tasks.clone();
        runner.tick(
            1,
            100,
            &snap1,
            |ws, id, st, now| store.borrow_mut().set_state(ws, id, st, now),
            |ws, id, r| store.borrow_mut().set_result(ws, id, r),
        );
        assert_eq!(store.borrow().tasks[0].state, TaskState::Running);
        assert!(matches!(
            runner.running.get("t-1"),
            Some(DispatchHandle::ImmediateFail(_))
        ));

        // tick 2: poll → Failed → Running→Failed 전이.
        let snap2 = store.borrow().tasks.clone();
        runner.tick(
            1,
            200,
            &snap2,
            |ws, id, st, now| store.borrow_mut().set_state(ws, id, st, now),
            |ws, id, r| store.borrow_mut().set_result(ws, id, r),
        );
        assert!(matches!(
            store.borrow().tasks[0].state,
            TaskState::Failed { .. }
        ));
        assert!(store.borrow().tasks[0].result.is_some());
        assert!(!runner.running.contains_key("t-1"));
    }
}
