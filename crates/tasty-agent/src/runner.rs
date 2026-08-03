//! Task runner — Ready task 를 dispatch 하고 Running task 를 poll 하는 순수 루프.
//!
//! 본 모듈은 *IPC / GUI / OS 와 독립적*. 실제 host-side 동작 (shell process,
//! Custom IPC dispatch, 범용 폴링) 은 [`TaskExecutor`] trait 의 host 측
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

use serde::{Deserialize, Serialize};

use crate::task::{Task, TaskId, TaskResult, TaskState};
use crate::{AgentError, Result};

/// dispatch / poll 결과를 묶는 핸들. variant 별 의미:
/// - `PolledDispatch`: 범용 비동기 폴링. dispatch 후 `poll_method` 를 terminal 상태
///   도달까지 반복 호출. `poll_params` 는 dispatch 시점에 완성된 폴링 인자.
/// - `ShellProcess`: pid 만. host executor 가 `Child` 객체를 별 map 으로 보관.
/// - `ReduceImmediate` / `CustomImmediate`: dispatch 시점에 결과가 즉시 결정.
/// - `ImmediateFail`: dispatch 가 실패. transition 표상 Ready → Failed 직접 전이가
///   불허이므로 *먼저 Running 으로 보낸 후* poll 결과로 Failed 로 흡수하기 위한
///   우회 variant.
/// - `AwaitExternal`: poll 은 관여하지 않는(항상 Active) 외부 push 대기. 종결은
///   host 가 밖에서 store 를 직접 전이시켜 이뤄지고, `tick` 0단계(terminal 흡수)
///   가 다음 tick 에 handle 정리를 담당한다.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum DispatchHandle {
    /// 범용 폴링 핸들 — dispatch 응답으로 채워진 `poll_params` 로 `poll_method` 를
    /// terminal 상태(`state_field` 가 `terminal_states` 중 하나) 도달까지 반복 호출.
    PolledDispatch {
        workspace_id: u32,
        poll_method: String,
        poll_params: serde_json::Value,
        state_field: String,
        terminal_states: Vec<String>,
        interval_ms: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        deadline_ms: Option<u64>,
    },
    ShellProcess {
        pid: u32,
    },
    ReduceImmediate(TaskResult),
    CustomImmediate(TaskResult),
    ImmediateFail(String),
    /// `WaitBarrier` task 의 polling handle — poll 마다 barrier_state 재조회.
    BarrierPoll {
        workspace_id: u32,
        name: String,
    },
    /// 외부 push 신호(예: 훅 완료)를 기다리는 핸들. `poll` 은
    /// host executor 구현에서 **항상** [`PollOutcome::Active`] 를 반환해야 한다 —
    /// 이 handle 의 진짜 종결은 poll 이 아니라 host 가 외부에서(예: `HookFired`
    /// 소비 경로) `task_set_result`/`task_set_state` 를 직접 호출해 이뤄진다.
    /// `wait_key` 는 이 크레이트가 의미를 해석하지 않는 host-opaque 식별자(예:
    /// hook_id 의 문자열화) — host 가 자신의 `wait_key → task_id` 매핑에서 이
    /// task 를 다시 찾을 때 쓴다. 다른 영속 handle 과 동일하게
    /// `tasty.agent.handle.<id>` 로 영속되므로 호스트 재시작도 버틴다 — 외부
    /// 완료는 `self.running`(runner in-memory)과 무관하게 store 를 직접
    /// 전이시키므로, 재시작 후에도 handle 을 `PolledDispatch`/`BarrierPoll` 과
    /// 동형으로 **복원**해야 한다(host reload 경로). 복원하지 않으면 0단계
    /// terminal 흡수가 이 task 를 찾지 못해 `release_permit` 이 누락된다 — 그
    /// permit 누수를 막는 것이 이 variant 를 도입한 목적이므로, 재시작
    /// 시나리오에서도 지켜야 한다.
    ///
    /// `deadline_ms` 는 unix epoch ms 절대 시각 — dispatch 시점에 push 전략의
    /// `timeout_ms` 로부터 host 가 계산해 채운다. `hook_id → task_id` 매핑
    /// (host 의 `hook_wait` 모듈)은 비영속이라 재시작하면 사라지지만, 이
    /// `deadline_ms` 는 handle 자체에 실려 함께 영속되므로 재시작 후에도 만료
    /// 판정이 가능하다 — 훅으로 깨어날 수는 없어도 deadline 으로는 마감된다.
    /// `#[serde(default)]` 는 이 필드 도입 이전에 영속된 구 포맷(필드 없음)을
    /// `0`으로 채운다 — 즉 **즉시 만료** 로 취급한다(무한 대기로 잔존하는 것이
    /// 바로 이 variant 가 없애려는 상태이므로, 정보 없는 구 handle 을 무한으로
    /// 보는 선택지는 배제한다).
    AwaitExternal {
        wait_key: String,
        #[serde(default)]
        deadline_ms: u64,
    },
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
    #[allow(clippy::cognitive_complexity)] // complexity-exempt: 한 tick 안의 순차 3단계(외부 terminal 흡수 → Running poll → Ready dispatch)가 self.running·set_state·set_result 클로저를 공유해, 쪼개면 세 함수에 동일 매개변수만 나열되고 흐름 추적이 어려워진다. 중첩은 얕음.
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
        // 0. 외부 terminal 전이 흡수(누수 수정) — `agent.task_cancel` /
        //    `agent.task_set_result` 가 외부에서 store 의 Running 을 어느
        //    terminal 상태로든(Succeeded/Failed/Cancelled/Skipped,
        //    `TaskState::is_terminal()`) 직접 전이시켰을 수 있다. **이전엔
        //    `Cancelled` 만 흡수했다** — `task_set_result` 로 Succeeded/Failed 로
        //    전이된 task 는 이 0단계를 통과해 버려 handle 이 `self.running` 에
        //    영구 잔존하고 `release_permit` 이 결코 호출되지 않았다(semaphore
        //    permit·lease 누수). 이번 tick 시작 시점에 이미 Running 이 아니라면
        //    (즉 이 tick 의 1단계 poll 로 방금 종결된 게 아니라 그 이전에 이미
        //    외부에서 종결됐다면) handle 정리 + permit 해제를 이 자리에서
        //    선제 흡수한다. handle 이 없으면(에초에 dispatch 이력이 없는 task)
        //    `remove` 가 no-op 이라 안전하다.
        for task in tasks {
            if !task.state.is_terminal() {
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

    /// I.A.S6: Deferred 반환 시 task 는 Ready 유지, 다음 tick 재dispatch.
    #[test]
    fn deferred_dispatch_keeps_task_ready_for_retry() {
        struct ToggleExec {
            calls: u32,
        }
        impl TaskExecutor for ToggleExec {
            fn dispatch(&mut self, _t: &Task) -> DispatchOutcome {
                self.calls += 1;
                if self.calls == 1 {
                    DispatchOutcome::Deferred
                } else {
                    DispatchOutcome::Started(DispatchHandle::ShellProcess { pid: 42 })
                }
            }
            fn poll(&mut self, _h: &DispatchHandle) -> PollOutcome {
                PollOutcome::Active
            }
        }
        let mut runner = RunnerLoop::new(ToggleExec { calls: 0 });
        let store = std::cell::RefCell::new(MockStore {
            tasks: vec![mk_task("t-1", &[])],
        });

        // tick 1: Deferred → state 전이 없음, handle 미보관.
        let snap1 = store.borrow().tasks.clone();
        runner.tick(
            1,
            100,
            &snap1,
            |ws, id, st, now| store.borrow_mut().set_state(ws, id, st, now),
            |ws, id, r| store.borrow_mut().set_result(ws, id, r),
        );
        assert_eq!(store.borrow().tasks[0].state, TaskState::Ready);
        assert!(!runner.running.contains_key("t-1"));

        // tick 2: 같은 task 가 다시 dispatch 시도 → Started → Running.
        let snap2 = store.borrow().tasks.clone();
        runner.tick(
            1,
            200,
            &snap2,
            |ws, id, st, now| store.borrow_mut().set_state(ws, id, st, now),
            |ws, id, r| store.borrow_mut().set_result(ws, id, r),
        );
        assert_eq!(store.borrow().tasks[0].state, TaskState::Running);
        assert!(runner.running.contains_key("t-1"));
    }

    /// J.A.S2: DispatchHandle 의 serde round-trip — 영속 후 reload 시 의미 보존 확인.
    #[test]
    fn dispatch_handle_serde_roundtrip_all_variants() {
        let cases = vec![
            DispatchHandle::PolledDispatch {
                workspace_id: 42,
                poll_method: "fake.poll".into(),
                poll_params: serde_json::json!({ "job": "J1" }),
                state_field: "state".into(),
                terminal_states: vec!["done".into()],
                interval_ms: 500,
                deadline_ms: Some(1234),
            },
            DispatchHandle::ShellProcess { pid: 12345 },
            DispatchHandle::ReduceImmediate(TaskResult {
                exit_code: Some(0),
                output: Some(serde_json::json!({"merged": "ok"})),
                error: None,
            }),
            DispatchHandle::CustomImmediate(TaskResult {
                exit_code: Some(0),
                output: None,
                error: None,
            }),
            DispatchHandle::ImmediateFail("boom".into()),
            DispatchHandle::BarrierPoll {
                workspace_id: 1,
                name: "b".into(),
            },
            DispatchHandle::AwaitExternal {
                wait_key: "42".into(),
                deadline_ms: 999_999,
            },
        ];
        for h in cases {
            let v = serde_json::to_value(&h).expect("serialize");
            // tag="kind" → object 에 "kind" 필드 존재.
            assert!(v.get("kind").is_some(), "missing 'kind' for {h:?}");
            let back: DispatchHandle = serde_json::from_value(v).expect("deserialize");
            // Debug equality (DispatchHandle 는 PartialEq 안 받음 → format 비교).
            assert_eq!(format!("{h:?}"), format!("{back:?}"));
        }
    }

    /// 구 포맷(deadline_ms 도입 전 영속된 `AwaitExternal` — `wait_key` 만 있음)의
    /// 하위호환. `#[serde(default)]` 가 누락 필드를 `0`으로 채우고, 이는
    /// (실사용 시각이 항상 0 보다 큰 이상) 항상 만료로 판정된다 — 무한 대기로
    /// 남는 것을 막으려는 의도적 설계.
    #[test]
    fn await_external_old_format_without_deadline_defaults_to_immediately_expired() {
        let old_format = serde_json::json!({
            "kind": "await_external",
            "data": { "wait_key": "hook-1" },
        });
        let handle: DispatchHandle =
            serde_json::from_value(old_format).expect("old format deserialize");
        match handle {
            DispatchHandle::AwaitExternal {
                wait_key,
                deadline_ms,
            } => {
                assert_eq!(wait_key, "hook-1");
                assert_eq!(deadline_ms, 0, "missing field must default to 0 (=expired)");
            }
            other => panic!("expected AwaitExternal, got {other:?}"),
        }
    }

    /// `AwaitExternal` 핸들의 의도된 전체 생애주기 — dispatch 로
    /// 생성된 뒤 여러 tick 동안 poll 이 항상 `Active` 라 Running 이 유지되고
    /// (executor 는 이 핸들에 절대 관여하지 않는다), 외부(hook 완료 등)가 store 를
    /// 직접 Succeeded 로 전이시키면 **다음 tick 의 0단계**가 handle 제거 +
    /// release_permit 을 흡수한다 — 0단계가 `Cancelled` 만 흡수하던 예전 버전이면
    /// 이 시나리오에서 handle 이 영구 잔존해 이 테스트가 실패했을 것이다.
    #[test]
    fn await_external_stays_active_until_externally_resolved_then_absorbed() {
        struct AwaitExec {
            released: std::cell::RefCell<Vec<TaskId>>,
        }
        impl TaskExecutor for AwaitExec {
            fn dispatch(&mut self, _t: &Task) -> DispatchOutcome {
                DispatchOutcome::Started(DispatchHandle::AwaitExternal {
                    wait_key: "hook-99".into(),
                    deadline_ms: u64::MAX,
                })
            }
            fn poll(&mut self, handle: &DispatchHandle) -> PollOutcome {
                // 계약: AwaitExternal 은 poll 이 절대 종결시키지 않는다.
                assert!(matches!(handle, DispatchHandle::AwaitExternal { .. }));
                PollOutcome::Active
            }
            fn release_permit(&mut self, task_id: &TaskId) {
                self.released.borrow_mut().push(task_id.clone());
            }
        }
        let mut runner = RunnerLoop::new(AwaitExec {
            released: std::cell::RefCell::new(Vec::new()),
        });
        let store = std::cell::RefCell::new(MockStore {
            tasks: vec![mk_task("t-1", &[])],
        });

        // tick 1: Ready → Running, AwaitExternal handle 보관.
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
            Some(DispatchHandle::AwaitExternal { .. })
        ));

        // tick 2~3: poll 은 항상 Active — Running 유지, handle 잔존.
        for now in [200, 300] {
            let snap = store.borrow().tasks.clone();
            runner.tick(
                1,
                now,
                &snap,
                |ws, id, st, n| store.borrow_mut().set_state(ws, id, st, n),
                |ws, id, r| store.borrow_mut().set_result(ws, id, r),
            );
            assert_eq!(store.borrow().tasks[0].state, TaskState::Running);
            assert!(runner.running.contains_key("t-1"));
        }
        assert!(runner.executor.released.borrow().is_empty());

        // 외부(hook_id → task_id 매핑 소비 등)가 poll 을 거치지 않고 store 를
        // 직접 Succeeded 로 전이 — `agent.task_set_result` 시나리오와 동형.
        store
            .borrow_mut()
            .set_result(
                1,
                &"t-1".to_string(),
                TaskResult {
                    exit_code: Some(0),
                    output: None,
                    error: None,
                },
            )
            .expect("set_result");
        store
            .borrow_mut()
            .set_state(1, &"t-1".to_string(), TaskState::Succeeded, 400)
            .expect("set_state");

        // tick 4: 0단계가 이 외부 전이를 흡수 — handle 제거 + release_permit.
        let snap4 = store.borrow().tasks.clone();
        runner.tick(
            1,
            400,
            &snap4,
            |ws, id, st, now| store.borrow_mut().set_state(ws, id, st, now),
            |ws, id, r| store.borrow_mut().set_result(ws, id, r),
        );
        assert!(!runner.running.contains_key("t-1"));
        assert_eq!(
            runner.executor.released.borrow().clone(),
            vec!["t-1".to_string()]
        );
    }

    /// I.A.S6: Cancelled task — running map 에서 제거 + release_permit 호출.
    #[test]
    fn cancelled_running_task_is_purged_and_permit_released() {
        struct TrackReleases {
            released: std::cell::RefCell<Vec<TaskId>>,
        }
        impl TaskExecutor for TrackReleases {
            fn dispatch(&mut self, _t: &Task) -> DispatchOutcome {
                DispatchOutcome::Started(DispatchHandle::ShellProcess { pid: 1 })
            }
            fn poll(&mut self, _h: &DispatchHandle) -> PollOutcome {
                PollOutcome::Active
            }
            fn release_permit(&mut self, task_id: &TaskId) {
                self.released.borrow_mut().push(task_id.clone());
            }
        }
        let exec = TrackReleases {
            released: std::cell::RefCell::new(Vec::new()),
        };
        let mut runner = RunnerLoop::new(exec);
        runner
            .running
            .insert("t-1".to_string(), DispatchHandle::ShellProcess { pid: 1 });
        // 외부에서 Cancelled 로 직접 전이된 상태 (agent.task_cancel 시나리오).
        let store = std::cell::RefCell::new(MockStore {
            tasks: vec![Task {
                state: TaskState::Cancelled,
                ..mk_task("t-1", &[])
            }],
        });
        let snap = store.borrow().tasks.clone();
        runner.tick(
            1,
            300,
            &snap,
            |ws, id, st, now| store.borrow_mut().set_state(ws, id, st, now),
            |ws, id, r| store.borrow_mut().set_result(ws, id, r),
        );
        assert!(!runner.running.contains_key("t-1"));
        let released = runner.executor.released.borrow().clone();
        assert_eq!(released, vec!["t-1".to_string()]);
    }

    struct TrackReleases {
        released: std::cell::RefCell<Vec<TaskId>>,
    }
    impl TaskExecutor for TrackReleases {
        fn dispatch(&mut self, _t: &Task) -> DispatchOutcome {
            DispatchOutcome::Started(DispatchHandle::ShellProcess { pid: 1 })
        }
        fn poll(&mut self, _h: &DispatchHandle) -> PollOutcome {
            PollOutcome::Active
        }
        fn release_permit(&mut self, task_id: &TaskId) {
            self.released.borrow_mut().push(task_id.clone());
        }
    }

    /// 누수 회귀 방지 — `agent.task_set_result` 로 외부에서 Running 을
    /// 곧장 Succeeded 로 전이시킨 task(0단계가 예전엔 `Cancelled` 만 흡수해
    /// 이 경우를 놓쳤다)도 handle 제거 + release_permit 이 일어나야 한다.
    #[test]
    fn externally_succeeded_running_task_is_purged_and_permit_released() {
        let exec = TrackReleases {
            released: std::cell::RefCell::new(Vec::new()),
        };
        let mut runner = RunnerLoop::new(exec);
        runner
            .running
            .insert("t-1".to_string(), DispatchHandle::ShellProcess { pid: 1 });
        let store = std::cell::RefCell::new(MockStore {
            tasks: vec![Task {
                state: TaskState::Succeeded,
                ..mk_task("t-1", &[])
            }],
        });
        let snap = store.borrow().tasks.clone();
        runner.tick(
            1,
            300,
            &snap,
            |ws, id, st, now| store.borrow_mut().set_state(ws, id, st, now),
            |ws, id, r| store.borrow_mut().set_result(ws, id, r),
        );
        assert!(!runner.running.contains_key("t-1"));
        assert_eq!(
            runner.executor.released.borrow().clone(),
            vec!["t-1".to_string()]
        );
    }

    /// 위와 동형 — `Failed`. 두 상태 모두 `TaskState::is_terminal()` 로 흡수된다.
    #[test]
    fn externally_failed_running_task_is_purged_and_permit_released() {
        let exec = TrackReleases {
            released: std::cell::RefCell::new(Vec::new()),
        };
        let mut runner = RunnerLoop::new(exec);
        runner
            .running
            .insert("t-1".to_string(), DispatchHandle::ShellProcess { pid: 1 });
        let store = std::cell::RefCell::new(MockStore {
            tasks: vec![Task {
                state: TaskState::Failed {
                    error: "boom".into(),
                },
                ..mk_task("t-1", &[])
            }],
        });
        let snap = store.borrow().tasks.clone();
        runner.tick(
            1,
            300,
            &snap,
            |ws, id, st, now| store.borrow_mut().set_state(ws, id, st, now),
            |ws, id, r| store.borrow_mut().set_result(ws, id, r),
        );
        assert!(!runner.running.contains_key("t-1"));
        assert_eq!(
            runner.executor.released.borrow().clone(),
            vec!["t-1".to_string()]
        );
    }

    /// 0단계는 handle 이 없는(dispatch 이력 없는) terminal task 에는 no-op —
    /// release_permit 을 호출하지 않는다(과호출 방지).
    #[test]
    fn terminal_task_without_handle_does_not_call_release_permit() {
        let exec = TrackReleases {
            released: std::cell::RefCell::new(Vec::new()),
        };
        let mut runner = RunnerLoop::new(exec);
        // self.running 에 아무것도 없음 — 예: 재시작 후 Skipped 로 이미 종결된 task.
        let store = std::cell::RefCell::new(MockStore {
            tasks: vec![Task {
                state: TaskState::Skipped,
                ..mk_task("t-1", &[])
            }],
        });
        let snap = store.borrow().tasks.clone();
        runner.tick(
            1,
            300,
            &snap,
            |ws, id, st, now| store.borrow_mut().set_state(ws, id, st, now),
            |ws, id, r| store.borrow_mut().set_result(ws, id, r),
        );
        assert!(runner.executor.released.borrow().is_empty());
    }
}
