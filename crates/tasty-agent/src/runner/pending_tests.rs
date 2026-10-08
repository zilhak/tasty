//! 기록하지 못한 완료 보고 — 다시 poll 하지 않고 같은 보고를 다음 tick 에 낸다.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;

use super::*;
use crate::task::{OnFailure, TaskCommand};

struct OnceDone {
    polls: Cell<u32>,
    released: RefCell<Vec<TaskId>>,
}

impl TaskExecutor for OnceDone {
    fn dispatch(&mut self, _t: &Task) -> DispatchOutcome {
        DispatchOutcome::Started(DispatchHandle::ShellProcess { pid: 1 })
    }
    fn poll(&mut self, _h: &DispatchHandle) -> PollOutcome {
        self.polls.set(self.polls.get() + 1);
        PollOutcome::Done(TaskResult {
            exit_code: Some(self.polls.get() as i32),
            output: None,
            error: None,
        })
    }
    fn release_permit(&mut self, task_id: &TaskId) {
        self.released.borrow_mut().push(task_id.clone());
    }
}

fn running_task() -> Task {
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
        state: TaskState::Running,
        created_at: 0,
        started_at: Some(0),
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

#[test]
fn a_completion_that_was_not_recorded_is_resent_without_polling_again() {
    let mut runner = RunnerLoop::new(OnceDone {
        polls: Cell::new(0),
        released: RefCell::new(Vec::new()),
    });
    runner
        .running
        .insert("t-1".into(), DispatchHandle::ShellProcess { pid: 1 });
    let snap = vec![running_task()];
    let sent: RefCell<Vec<Completion>> = RefCell::new(Vec::new());
    let fail_store = Cell::new(true);

    for now in [10, 20] {
        runner.tick(
            1,
            now,
            &snap,
            |_, _, _, _| Ok(()),
            |_, _, c, _| {
                sent.borrow_mut().push(c);
                if fail_store.get() {
                    Err(AgentError::InvalidArgument("store unavailable".into()))
                } else {
                    Ok(())
                }
            },
        );
        // 기록하지 못한 동안 handle 과 permit 을 쥐고 있다.
        assert!(runner.running.contains_key("t-1"));
        assert!(runner.executor.released.borrow().is_empty());
        assert!(runner.pending.contains_key("t-1"));
    }
    assert_eq!(runner.executor.polls.get(), 1, "결과는 한 번만 거둔다");

    fail_store.set(false);
    runner.tick(
        1,
        30,
        &snap,
        |_, _, _, _| Ok(()),
        |_, _, c, _| {
            sent.borrow_mut().push(c);
            Ok(())
        },
    );
    assert!(!runner.running.contains_key("t-1"));
    assert!(runner.pending.is_empty());
    assert_eq!(*runner.executor.released.borrow(), vec!["t-1".to_string()]);
    let sent = sent.into_inner();
    assert_eq!(sent.len(), 3);
    assert!(sent.iter().all(|c| *c == sent[0]), "같은 보고를 다시 낸다");
}

#[test]
fn a_rejected_completion_is_not_resent() {
    let mut runner = RunnerLoop::new(OnceDone {
        polls: Cell::new(0),
        released: RefCell::new(Vec::new()),
    });
    runner
        .running
        .insert("t-1".into(), DispatchHandle::ShellProcess { pid: 1 });
    let snap = vec![running_task()];
    runner.tick(
        1,
        10,
        &snap,
        |_, _, _, _| Ok(()),
        |_, id, _, _| {
            Err(AgentError::CompletionRejected {
                task_id: id.clone(),
                attempt_id: Some("t-1#1".into()),
                current_attempt_id: Some("t-1#2".into()),
                reason: crate::CompletionRejection::StaleAttempt,
            })
        },
    );
    assert!(runner.pending.is_empty());
    assert!(!runner.running.contains_key("t-1"));
}

struct BigExit;

impl TaskExecutor for BigExit {
    fn dispatch(&mut self, _t: &Task) -> DispatchOutcome {
        DispatchOutcome::Started(DispatchHandle::ShellProcess { pid: 1 })
    }
    fn poll(&mut self, _h: &DispatchHandle) -> PollOutcome {
        PollOutcome::Exited(TaskResult {
            exit_code: Some(3),
            output: Some(serde_json::json!({"stdout": {"text": "x"}})),
            error: Some("Run exited with code 3\n--- stdout ---\nx".into()),
        })
    }
}

/// 레코드가 memory 값 상한을 넘어 기록하지 못한 보고는 다시 내지 않고, 출력을 뺀 같은 회차의
/// 실패로 바꿔 기록한다. 그 뒤 handle 과 permit 을 놓는다.
#[test]
fn a_completion_too_large_to_store_is_settled_as_a_short_failure() {
    let mut runner = RunnerLoop::new(BigExit);
    runner
        .running
        .insert("t-1".into(), DispatchHandle::ShellProcess { pid: 1 });
    let snap = vec![running_task()];
    let sent: RefCell<Vec<Completion>> = RefCell::new(Vec::new());
    runner.tick(
        1,
        10,
        &snap,
        |_, _, _, _| Ok(()),
        |_, _, c, _| {
            let too_large = c.result.output.is_some();
            sent.borrow_mut().push(c);
            if too_large {
                Err(AgentError::Memory(
                    tasty_memory::MemoryError::ValueTooLarge {
                        actual: 1_049_931,
                        max: 1_048_576,
                    },
                ))
            } else {
                Ok(())
            }
        },
    );
    assert!(!runner.running.contains_key("t-1"));
    assert!(runner.pending.is_empty());
    let sent = sent.into_inner();
    assert_eq!(sent.len(), 2);
    let shrunk = &sent[1];
    assert_eq!(shrunk.result.exit_code, Some(3));
    assert!(shrunk.result.output.is_none());
    let CompletionOutcome::Failed { error } = &shrunk.outcome else {
        panic!("expected a failure, got {:?}", shrunk.outcome);
    };
    assert!(
        error.starts_with("Run exited with code 3 (the full result could not be stored: "),
        "{error}"
    );
    assert_eq!(shrunk.result.error.as_ref(), Some(error));

    // 이미 짧은 보고가 같은 오류로 실패하면 바꾸지 않고 다시 낸다.
    let e = AgentError::Memory(tasty_memory::MemoryError::ValueTooLarge { actual: 2, max: 1 });
    assert!(shrink_too_large_completion(&e, shrunk).is_none());
    // 다른 저장 오류는 바꾸지 않는다.
    let other = AgentError::InvalidArgument("store unavailable".into());
    assert!(shrink_too_large_completion(&other, &sent[0]).is_none());
}
