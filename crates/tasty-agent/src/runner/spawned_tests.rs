//! 시작했지만 Running 전이를 확정하지 못한 회차 — 같은 회차를 다시 시작하지 않고 다음 tick 에 이어 간다.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;

use super::*;
use crate::task::{OnFailure, TaskCommand};

#[derive(Default)]
struct Counting {
    dispatches: Cell<u32>,
    /// poll 한 handle 의 pid.
    polled: RefCell<Vec<u32>>,
    released: RefCell<Vec<TaskId>>,
    done: Cell<bool>,
}

impl TaskExecutor for Counting {
    fn dispatch(&mut self, _t: &Task) -> DispatchOutcome {
        self.dispatches.set(self.dispatches.get() + 1);
        DispatchOutcome::Started(DispatchHandle::ShellProcess {
            pid: 100 + self.dispatches.get(),
        })
    }
    fn poll(&mut self, h: &DispatchHandle) -> PollOutcome {
        if let DispatchHandle::ShellProcess { pid } = h {
            self.polled.borrow_mut().push(*pid);
        }
        if self.done.get() {
            PollOutcome::Done(TaskResult {
                exit_code: Some(0),
                output: None,
                error: None,
            })
        } else {
            PollOutcome::Active
        }
    }
    fn release_permit(&mut self, task_id: &TaskId) {
        self.released.borrow_mut().push(task_id.clone());
    }
}

fn task(state: TaskState) -> Task {
    Task {
        id: "t-1".into(),
        workspace_id: 1,
        name: "t-1".into(),
        command: TaskCommand::Run {
            command: vec!["true".into()],
            workspace_id: 1,
            cwd: None as Option<PathBuf>,
        },
        depends_on: vec![],
        state,
        created_at: 0,
        started_at: None,
        finished_at: None,
        result: None,
        on_failure: OnFailure::default(),
        metadata: serde_json::Value::Null,
        reserved_for_fallback: false,
        contract: None,
        typed_result: None,
        graph_id: None,
        input_snapshot: None,
        accepted: None,
        report_token: None,
        attempt: None,
        route: None,
        skip: None,
    }
}

fn unanswered() -> AgentError {
    AgentError::InvalidArgument("running transition not answered".into())
}

/// `states` 의 snapshot 으로 한 tick 을 돈다. Running 전이는 `running_ok` 일 때만 성공한다.
fn tick(runner: &mut RunnerLoop<Counting>, now: u64, state: TaskState, running_ok: bool) {
    runner.tick(
        1,
        now,
        &[task(state)],
        |_, _, st, _| {
            if matches!(st, TaskState::Running) && !running_ok {
                Err(unanswered())
            } else {
                Ok(())
            }
        },
        |_, _, _, _| Ok(()),
    );
}

/// 첫 dispatch 가 시작한 handle 의 pid.
const SPAWNED: u32 = 101;

#[test]
fn a_started_attempt_whose_running_transition_failed_is_not_started_again() {
    let mut runner = RunnerLoop::new(Counting::default());
    tick(&mut runner, 10, TaskState::Ready, false);
    tick(&mut runner, 20, TaskState::Ready, false);
    assert_eq!(
        runner.executor.dispatches.get(),
        1,
        "같은 회차를 다시 시작했다"
    );
    assert!(
        runner.executor.released.borrow().is_empty(),
        "실행 중인 회차의 점유를 놓았다"
    );

    tick(&mut runner, 30, TaskState::Ready, true);
    tick(&mut runner, 40, TaskState::Running, true);
    assert_eq!(runner.executor.dispatches.get(), 1);
    assert_eq!(
        *runner.executor.polled.borrow(),
        vec![SPAWNED],
        "시작한 그 handle 을 지켜본다"
    );

    runner.executor.done.set(true);
    tick(&mut runner, 50, TaskState::Running, true);
    assert_eq!(*runner.executor.released.borrow(), vec!["t-1".to_string()]);
    assert!(runner.running.is_empty());
}

#[test]
fn a_running_transition_that_committed_without_an_answer_is_polled_and_released() {
    let mut runner = RunnerLoop::new(Counting::default());
    // 전이는 확정됐지만 답을 받지 못했다. 다음 snapshot 은 Running 이다.
    tick(&mut runner, 10, TaskState::Ready, false);
    tick(&mut runner, 20, TaskState::Running, true);
    assert_eq!(*runner.executor.polled.borrow(), vec![SPAWNED]);

    runner.executor.done.set(true);
    tick(&mut runner, 30, TaskState::Running, true);
    assert_eq!(runner.executor.dispatches.get(), 1);
    assert_eq!(*runner.executor.released.borrow(), vec!["t-1".to_string()]);
}

#[test]
fn cancelling_a_started_attempt_before_running_is_confirmed_returns_its_permit_once() {
    let mut runner = RunnerLoop::new(Counting::default());
    tick(&mut runner, 10, TaskState::Ready, false);
    tick(&mut runner, 20, TaskState::Cancelled, true);
    tick(&mut runner, 30, TaskState::Cancelled, true);
    assert_eq!(*runner.executor.released.borrow(), vec!["t-1".to_string()]);
    assert_eq!(runner.executor.dispatches.get(), 1);
    assert!(runner.executor.polled.borrow().is_empty());
}

#[test]
fn an_attempt_whose_task_is_no_longer_runnable_is_stopped_and_may_start_afresh() {
    let mut runner = RunnerLoop::new(Counting::default());
    tick(&mut runner, 10, TaskState::Ready, false);
    tick(&mut runner, 20, TaskState::Waiting, true);
    assert_eq!(*runner.executor.released.borrow(), vec!["t-1".to_string()]);
    // 점유를 돌려준 뒤 다시 Ready 가 되면 새로 시작한다.
    tick(&mut runner, 30, TaskState::Ready, true);
    assert_eq!(runner.executor.dispatches.get(), 2);
}

#[test]
fn a_started_attempt_missing_from_the_snapshot_is_kept() {
    let mut runner = RunnerLoop::new(Counting::default());
    tick(&mut runner, 10, TaskState::Ready, false);
    // 목록을 읽지 못한 tick 은 빈 snapshot 이다. 없는 task 를 끝난 것으로 보지 않는다.
    runner.tick(1, 20, &[], |_, _, _, _| Ok(()), |_, _, _, _| Ok(()));
    assert!(runner.executor.released.borrow().is_empty());
    tick(&mut runner, 30, TaskState::Ready, true);
    tick(&mut runner, 40, TaskState::Running, true);
    assert_eq!(runner.executor.dispatches.get(), 1);
    assert_eq!(*runner.executor.polled.borrow(), vec![SPAWNED]);
}

fn typed(state: TaskState, attempt: Option<u32>) -> Task {
    let mut t = task(state);
    t.contract =
        Some(serde_json::from_value(serde_json::json!({"contract_version": 2})).expect("contract"));
    t.attempt = attempt.map(|number| crate::task::attempt::TaskAttempt {
        id: crate::task::attempt::attempt_id(&t.id, number),
        number,
        started_at: 0,
        completion: None,
        postprocess: None,
        agent: None,
    });
    t
}

/// v2 task 는 회차로 같은 실행인지 가린다. 다른 회차의 Running 에는 옛 실행을 붙이지 않는다.
#[test]
fn a_typed_attempt_is_resumed_only_for_the_attempt_it_started() {
    let mut runner = RunnerLoop::new(Counting::default());
    let fail = |_: u32, _: &TaskId, _: TaskState, _: u64| Err(unanswered());
    let ok = |_: u32, _: &TaskId, _: TaskState, _: u64| Ok(());
    let done = |_: u32, _: &TaskId, _: Completion, _: u64| Ok(());
    // 회차 1 을 시작했다. Running 이 확정됐다면 attempt 는 t-1#1 이다.
    runner.tick(1, 10, &[typed(TaskState::Ready, None)], fail, done);
    runner.tick(1, 20, &[typed(TaskState::Running, Some(1))], ok, done);
    assert_eq!(*runner.executor.polled.borrow(), vec![SPAWNED]);

    let mut other = RunnerLoop::new(Counting::default());
    other.tick(1, 10, &[typed(TaskState::Ready, None)], fail, done);
    other.tick(1, 20, &[typed(TaskState::Running, Some(2))], ok, done);
    assert!(other.executor.polled.borrow().is_empty());
    assert_eq!(*other.executor.released.borrow(), vec!["t-1".to_string()]);
}
