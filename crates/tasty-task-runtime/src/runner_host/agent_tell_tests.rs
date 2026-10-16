//! 기존 세션에 보내는 `<provider>.tell` 이 응답 대기 상한(5초)을 넘길 때의 경계.
//!
//! 모의 IPC 는 `.state` 에 idle 로 바로 답하고 나머지 명령은 시험에 넘긴다. 시험은 명령을 꺼내
//! (실행 시작) 늦게 답하거나, 꺼내지 않아 큐 대기 중 만료되게 한다.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tasty_agent::runner::{DispatchHandle, PollOutcome, TaskExecutor};
use tasty_agent::task::FailureCode;
use tasty_agent::task::agent::report;
use tasty_ipc::dispatch::DispatchStats;
use tasty_ipc::host_call::HostIpcInjector;
use tasty_ipc::protocol::JsonRpcResponse;
use tasty_ipc::server::{Claim, IpcCommand};

use super::{create, dispatch, existing, ours, v2};
use crate::agent_turns::{TurnEnd, TurnEvent};
use crate::runner_host::late_answer::DISPATCH_OUTCOME_UNKNOWN;
use crate::runner_host::tests::fresh_ctx;
use crate::runner_host::{HostExecutor, RunnerContext, load_dispatch_handle};

fn claim(cmd: &IpcCommand) -> Claim {
    cmd.claim(&std::sync::Arc::new(DispatchStats::default()))
}

fn reply(cmd: IpcCommand, result: Value) {
    cmd.response_tx
        .send(JsonRpcResponse::success(
            cmd.request.id.clone().unwrap_or(Value::Null),
            result,
        ))
        .expect("send resp");
}

/// `.state` 는 idle 로 답하고 나머지 명령은 돌려준 통로로 넘긴다.
fn install(ctx: &RunnerContext) -> mpsc::Receiver<IpcCommand> {
    let (tx, rx) = mpsc::channel::<IpcCommand>();
    ctx.host_ipc
        .set(HostIpcInjector::new(tx, std::sync::Arc::new(|| {})))
        .ok()
        .expect("set once");
    let (fwd_tx, fwd_rx) = mpsc::channel::<IpcCommand>();
    std::thread::spawn(move || {
        while let Ok(cmd) = rx.recv() {
            if cmd.request.method.ends_with(".state") {
                assert_eq!(claim(&cmd), Claim::Run);
                reply(cmd, json!({ "state": "idle" }));
            } else if fwd_tx.send(cmd).is_err() {
                break;
            }
        }
    });
    fwd_rx
}

fn stored(ctx: &RunnerContext, task_id: &str) -> Option<DispatchHandle> {
    load_dispatch_handle(ctx, 1, task_id)
}

/// 지연된 handler 가 결국 지시를 전달했다. 다시 보내지 않고 늦은 응답을 받아 그 턴을 이어 간다.
#[test]
fn a_tell_that_answers_past_the_bound_is_not_sent_again() {
    let (_td, ctx) = fresh_ctx();
    let fwd = install(&ctx);
    let (go_tx, go_rx) = mpsc::channel::<()>();
    let worker = std::thread::spawn(move || {
        let tell = fwd.recv().expect("tell");
        assert_eq!(tell.request.method, "claude.tell");
        assert_eq!(claim(&tell), Claim::Run);
        go_rx.recv().expect("answer late");
        reply(tell, json!({ "ok": true }));
        fwd
    });
    let mut exec = HostExecutor::new(ctx.clone());
    let task = create(&ctx, existing(), v2());
    let h = dispatch(&mut exec, &ctx, &task);
    match exec.poll(&h) {
        PollOutcome::Active => {}
        other => panic!("a tell that started past the bound must not end the task, got {other:?}"),
    }
    assert_eq!(ctx.agent_turns.holder(7), Some(task.id.clone()));
    assert!(
        matches!(
            stored(&ctx, &task.id),
            Some(DispatchHandle::AwaitDispatchAnswer { .. })
        ),
        "a restart while waiting must see an unanswered request"
    );
    assert!(matches!(exec.poll(&h), PollOutcome::Active));
    go_tx.send(()).expect("go");
    let fwd = worker.join().unwrap();
    let until = Instant::now() + Duration::from_secs(5);
    while !matches!(
        stored(&ctx, &task.id),
        Some(DispatchHandle::AgentTurn { .. })
    ) {
        assert!(Instant::now() < until, "the late answer was never taken");
        assert!(matches!(exec.poll(&h), PollOutcome::Active));
        std::thread::sleep(Duration::from_millis(5));
    }
    ctx.agent_turns.report(7, "claude", ours(&ctx, 7));
    ctx.agent_turns.report(
        7,
        "claude",
        TurnEvent::Ended(TurnEnd::Answer(Some("mine".into()))),
    );
    let PollOutcome::Done(r) = exec.poll(&h) else {
        panic!("expected done");
    };
    assert_eq!(r.output.unwrap()[report::FINAL_ANSWER], "mine");
    assert!(fwd.try_recv().is_err(), "the tell was sent exactly once");
}

/// 큐에서 기다리다 만료된 지시는 실행되지 않았다. 묶음을 풀고 다음 poll 에서 다시 보낸다.
#[test]
fn a_tell_that_expired_in_the_queue_is_sent_again() {
    let (_td, ctx) = fresh_ctx();
    let fwd = install(&ctx);
    let mut exec = HostExecutor::new(ctx.clone());
    let task = create(&ctx, existing(), v2());
    let h = dispatch(&mut exec, &ctx, &task);
    match exec.poll(&h) {
        PollOutcome::Active => {}
        other => panic!("a tell that never ran must not end the task, got {other:?}"),
    }
    let stale = fwd.try_recv().expect("the expired tell was queued");
    assert_eq!(
        claim(&stale),
        Claim::Withdrawn,
        "the expired tell never runs"
    );
    assert_eq!(ctx.agent_turns.holder(7), None, "the binding is released");
    let worker = std::thread::spawn(move || {
        let tell = fwd.recv().expect("second tell");
        assert_eq!(claim(&tell), Claim::Run);
        reply(tell, json!({ "ok": true }));
        fwd
    });
    assert!(matches!(exec.poll(&h), PollOutcome::Active));
    let fwd = worker.join().unwrap();
    assert_eq!(ctx.agent_turns.holder(7), Some(task.id.clone()));
    ctx.agent_turns.report(7, "claude", ours(&ctx, 7));
    ctx.agent_turns.report(
        7,
        "claude",
        TurnEvent::Ended(TurnEnd::Answer(Some("done".into()))),
    );
    let PollOutcome::Done(r) = exec.poll(&h) else {
        panic!("expected done");
    };
    assert_eq!(r.output.unwrap()[report::FINAL_ANSWER], "done");
    assert!(fwd.try_recv().is_err(), "no third tell");
}

/// 시작한 지시의 응답 통로가 답 없이 닫혔다. 전달됐는지 알 수 없어 결과 불명이다.
#[test]
fn a_tell_whose_channel_closes_after_it_started_is_an_unknown_outcome() {
    let (_td, ctx) = fresh_ctx();
    let fwd = install(&ctx);
    let (go_tx, go_rx) = mpsc::channel::<()>();
    let worker = std::thread::spawn(move || {
        let tell = fwd.recv().expect("tell");
        assert_eq!(claim(&tell), Claim::Run);
        go_rx.recv().expect("drop late");
        drop(tell);
    });
    let mut exec = HostExecutor::new(ctx.clone());
    let task = create(&ctx, existing(), v2());
    let h = dispatch(&mut exec, &ctx, &task);
    assert!(matches!(exec.poll(&h), PollOutcome::Active));
    go_tx.send(()).expect("go");
    worker.join().unwrap();
    match exec.poll(&h) {
        PollOutcome::Lost(reason) => {
            assert!(reason.starts_with(DISPATCH_OUTCOME_UNKNOWN), "{reason}");
            assert!(reason.contains("claude.tell"), "{reason}");
        }
        other => panic!("expected an unknown outcome, got {other:?}"),
    }
}

/// 늦게 온 응답이 오류면 지시는 전달되지 않았다. 회차를 실패로 끝내고 묶음을 푼다.
#[test]
fn a_tell_that_fails_late_fails_the_attempt() {
    let (_td, ctx) = fresh_ctx();
    let fwd = install(&ctx);
    let (go_tx, go_rx) = mpsc::channel::<()>();
    let worker = std::thread::spawn(move || {
        let tell = fwd.recv().expect("tell");
        assert_eq!(claim(&tell), Claim::Run);
        go_rx.recv().expect("answer late");
        let id = tell.request.id.clone().unwrap_or(Value::Null);
        tell.response_tx
            .send(JsonRpcResponse::error(id, -32000, "no such session"))
            .expect("send resp");
    });
    let mut exec = HostExecutor::new(ctx.clone());
    let task = create(&ctx, existing(), v2());
    let h = dispatch(&mut exec, &ctx, &task);
    assert!(matches!(exec.poll(&h), PollOutcome::Active));
    go_tx.send(()).expect("go");
    worker.join().unwrap();
    let PollOutcome::Failed(e) = exec.poll(&h) else {
        panic!("expected failure");
    };
    assert_eq!(
        FailureCode::parse_message(&e),
        Some(FailureCode::AgentUnavailable)
    );
    assert_eq!(ctx.agent_turns.holder(7), None);
}
