//! 호스트가 응답 대기 상한(5초) 안에 답하지 못할 때의 실행·poll 경계.
//!
//! 모의 IPC 는 명령을 꺼내(실행 시작) 상한을 넘긴 뒤 답하거나, 꺼내지 않아 큐 대기 중 만료되게 한다.

use std::collections::HashMap;
use std::sync::mpsc;
use std::time::Duration;

use serde_json::{Value, json};
use tasty_agent::runner::{DispatchHandle, DispatchOutcome, PollOutcome, TaskExecutor};
use tasty_agent::{OnFailure, PollSpec, PollSpecRef, Task, TaskCommand, TaskState};
use tasty_ipc::dispatch::DispatchStats;
use tasty_ipc::host_call::HostIpcInjector;
use tasty_ipc::protocol::JsonRpcResponse;
use tasty_ipc::server::{Claim, IpcCommand};

use super::super::tests::fresh_ctx;
use super::super::{HOST_DISPATCH_TIMEOUT, HostExecutor, RunnerContext};
use super::DISPATCH_OUTCOME_UNKNOWN;

/// 상한을 확실히 넘기는 지연.
fn past_bound() -> Duration {
    HOST_DISPATCH_TIMEOUT + Duration::from_millis(500)
}

fn install(ctx: &RunnerContext) -> mpsc::Receiver<IpcCommand> {
    let (tx, rx) = mpsc::channel::<IpcCommand>();
    ctx.host_ipc
        .set(HostIpcInjector::new(tx, std::sync::Arc::new(|| {})))
        .ok()
        .expect("set once");
    rx
}

fn reply(cmd: IpcCommand, result: Value) {
    cmd.response_tx
        .send(JsonRpcResponse::success(
            cmd.request.id.clone().unwrap_or(Value::Null),
            result,
        ))
        .expect("send resp");
}

fn claim(cmd: &IpcCommand) -> Claim {
    cmd.claim(&std::sync::Arc::new(DispatchStats::default()))
}

fn polled(deadline_ms: Option<u64>) -> DispatchHandle {
    DispatchHandle::PolledDispatch {
        workspace_id: 1,
        poll_method: "fake.state".to_string(),
        poll_params: json!({ "surface_id": 9 }),
        state_field: "state".to_string(),
        terminal_states: vec!["idle".to_string()],
        failure_states: vec!["exited".to_string()],
        interval_ms: 1,
        deadline_ms,
    }
}

/// `fake.spawn` 을 보내고 응답의 `surface_id` 로 `fake.state` 를 poll 하는 Custom task.
fn spawn_task() -> Task {
    let mut map_from_response = HashMap::new();
    map_from_response.insert("surface_id".to_string(), "surface_id".to_string());
    Task {
        id: "t-spawn".to_string(),
        workspace_id: 1,
        name: "spawn".into(),
        command: TaskCommand::Custom {
            ipc_method: "fake.spawn".into(),
            params: json!({ "cwd": "/w" }),
            poll: Some(Box::new(PollSpecRef::Inline(PollSpec {
                poll_method: "fake.state".into(),
                map_from_response,
                map_from_request: HashMap::new(),
                state_field: "state".into(),
                terminal_states: vec!["idle".into()],
                failure_states: vec!["exited".into()],
                interval_ms: 1,
                timeout_ms: None,
            }))),
        },
        state: TaskState::Ready,
        depends_on: vec![],
        on_failure: OnFailure::Abort,
        metadata: Value::Null,
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
        result: None,
        created_at: 0,
        started_at: None,
        finished_at: None,
    }
}

#[test]
fn a_state_poll_that_started_but_answered_past_the_bound_asks_again() {
    let (_td, ctx) = fresh_ctx();
    let rx = install(&ctx);
    let worker = std::thread::spawn(move || {
        let slow = rx.recv().expect("first poll");
        assert_eq!(claim(&slow), Claim::Run);
        std::thread::sleep(past_bound());
        drop(slow);
        let next = rx.recv().expect("second poll");
        assert_eq!(claim(&next), Claim::Run);
        reply(next, json!({ "state": "idle" }));
    });
    let mut exec = HostExecutor::new(ctx);
    let handle = polled(None);
    match exec.poll(&handle) {
        PollOutcome::Active => {}
        other => panic!("a slow state query must not end the task, got {other:?}"),
    }
    match exec.poll(&handle) {
        PollOutcome::Done(r) => assert_eq!(r.output, Some(json!({ "state": "idle" }))),
        other => panic!("expected Done on the next poll, got {other:?}"),
    }
    worker.join().unwrap();
}

#[test]
fn a_state_poll_that_expired_in_the_queue_asks_again() {
    let (_td, ctx) = fresh_ctx();
    let rx = install(&ctx);
    let mut exec = HostExecutor::new(ctx);
    let handle = polled(None);
    // 아무도 꺼내지 않아 큐에서 상한이 지난다.
    match exec.poll(&handle) {
        PollOutcome::Active => {}
        other => panic!("a queued expiry must not end the task, got {other:?}"),
    }
    let stale = rx.recv().expect("the expired poll was queued");
    assert_eq!(claim(&stale), Claim::Withdrawn);
}

#[test]
fn a_slow_state_poll_past_the_poll_deadline_is_a_poll_timeout() {
    let (_td, ctx) = fresh_ctx();
    let _rx = install(&ctx);
    let mut exec = HostExecutor::new(ctx);
    match exec.poll(&polled(Some(1))) {
        PollOutcome::Failed(e) => {
            assert!(e.contains("poll timeout"), "{e}");
            assert!(e.contains("host_dispatch timeout"), "{e}");
        }
        other => panic!("expected the poll deadline to end it, got {other:?}"),
    }
}

#[test]
fn a_spawn_that_answers_past_the_bound_is_followed_without_sending_it_again() {
    let (_td, ctx) = fresh_ctx();
    let rx = install(&ctx);
    let (answer_tx, answer_rx) = mpsc::channel::<()>();
    let worker = std::thread::spawn(move || {
        let spawn = rx.recv().expect("spawn");
        assert_eq!(spawn.request.method, "fake.spawn");
        assert_eq!(claim(&spawn), Claim::Run);
        answer_rx.recv().expect("answer late");
        reply(spawn, json!({ "surface_id": 9 }));
        let state = rx.recv().expect("state poll");
        assert_eq!(state.request.method, "fake.state");
        assert_eq!(state.request.params.get("surface_id"), Some(&json!(9)));
        assert_eq!(claim(&state), Claim::Run);
        reply(state, json!({ "state": "idle" }));
        // 늦은 응답을 받은 뒤 spawn 이 다시 오지 않았다.
        rx
    });
    let mut exec = HostExecutor::new(ctx);
    let task = spawn_task();
    let handle = match exec.dispatch(&task) {
        DispatchOutcome::Started(h) => h,
        other => panic!("a started spawn past the bound must not fail the task, got {other:?}"),
    };
    assert!(
        matches!(handle, DispatchHandle::AwaitDispatchAnswer { .. }),
        "{handle:?}"
    );
    assert!(matches!(exec.poll(&handle), PollOutcome::Active));
    answer_tx.send(()).expect("go");
    let outcome = loop {
        match exec.poll(&handle) {
            PollOutcome::Active => std::thread::sleep(Duration::from_millis(5)),
            other => break other,
        }
    };
    match outcome {
        PollOutcome::Done(r) => assert_eq!(r.output, Some(json!({ "state": "idle" }))),
        other => panic!("expected the spawn-wait poll to finish, got {other:?}"),
    }
    let rx = worker.join().unwrap();
    assert!(rx.try_recv().is_err(), "the spawn was sent exactly once");
}

#[test]
fn a_spawn_that_expired_in_the_queue_is_retried_on_a_later_tick() {
    let (_td, ctx) = fresh_ctx();
    let rx = install(&ctx);
    let mut exec = HostExecutor::new(ctx);
    let task = spawn_task();
    match exec.dispatch(&task) {
        DispatchOutcome::Deferred => {}
        other => panic!("a spawn that never ran must stay Ready for a retry, got {other:?}"),
    }
    let stale = rx.recv().expect("the expired spawn was queued");
    assert_eq!(
        claim(&stale),
        Claim::Withdrawn,
        "the expired spawn never runs"
    );
}

#[test]
fn a_spawn_whose_channel_closes_after_it_started_is_an_unknown_outcome() {
    let (_td, ctx) = fresh_ctx();
    let rx = install(&ctx);
    let (drop_tx, drop_rx) = mpsc::channel::<()>();
    let worker = std::thread::spawn(move || {
        let spawn = rx.recv().expect("spawn");
        assert_eq!(claim(&spawn), Claim::Run);
        drop_rx.recv().expect("drop late");
        drop(spawn);
    });
    let mut exec = HostExecutor::new(ctx);
    let handle = match exec.dispatch(&spawn_task()) {
        DispatchOutcome::Started(h) => h,
        other => panic!("expected Started, got {other:?}"),
    };
    drop_tx.send(()).expect("go");
    worker.join().unwrap();
    match exec.poll(&handle) {
        PollOutcome::Lost(reason) => {
            assert!(reason.starts_with(DISPATCH_OUTCOME_UNKNOWN), "{reason}")
        }
        other => panic!("expected an unknown outcome, got {other:?}"),
    }
}

#[test]
fn an_awaited_answer_this_executor_does_not_hold_is_an_unknown_outcome() {
    let (_td, ctx) = fresh_ctx();
    let mut exec = HostExecutor::new(ctx);
    let handle = DispatchHandle::AwaitDispatchAnswer {
        workspace_id: 1,
        task_id: "t-gone".to_string(),
        ipc_method: "fake.spawn".to_string(),
    };
    match exec.poll(&handle) {
        PollOutcome::Lost(reason) => {
            assert!(reason.starts_with(DISPATCH_OUTCOME_UNKNOWN), "{reason}");
            assert!(reason.contains("fake.spawn"), "{reason}");
        }
        other => panic!("expected an unknown outcome, got {other:?}"),
    }
}
