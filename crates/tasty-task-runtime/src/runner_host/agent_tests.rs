//! agent task 실행을 가짜 provider 로 결정적으로 시험한다. provider 는 `<p>.state` 에 정해 둔
//! 상태로 답하고 `<p>.tell`·`<p>.spawn` 호출을 기록한다. 턴 보고는 표에 직접 넣는다.

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use tasty_agent::runner::{DispatchHandle, DispatchOutcome, PollOutcome, TaskExecutor};
use tasty_agent::task::agent::{AgentSession, report};
use tasty_agent::task::contract::TaskContract;
use tasty_agent::task::{FailureCode, TaskCreateOpts};
use tasty_agent::{OnFailure, Task, TaskCommand, TaskState, TaskStore};
use tasty_ipc::host_call::HostIpcInjector;
use tasty_ipc::protocol::JsonRpcResponse;
use tasty_ipc::server::IpcCommand;
use tasty_memory::HOST_OWNER;

use super::HostExecutor;
use crate::agent_turns::{TurnEnd, TurnEvent};
use crate::runner_host::RunnerContext;
use crate::runner_host::tests::fresh_ctx;

/// 가짜 provider. 상태를 바꾸며 호출을 기록한다.
struct FakeProvider {
    state: Arc<Mutex<String>>,
    calls: Arc<Mutex<Vec<(String, Value)>>>,
}

impl FakeProvider {
    fn install(ctx: &RunnerContext, initial: &str) -> Self {
        Self::install_with(ctx, initial, Duration::ZERO)
    }

    /// `spawn_delay` 만큼 늦게 spawn 에 답한다.
    fn install_with(ctx: &RunnerContext, initial: &str, spawn_delay: Duration) -> Self {
        let (tx, rx) = mpsc::channel::<IpcCommand>();
        ctx.host_ipc
            .set(HostIpcInjector::new(tx, Arc::new(|| {})))
            .ok()
            .expect("set once");
        let state = Arc::new(Mutex::new(initial.to_string()));
        let calls = Arc::new(Mutex::new(Vec::new()));
        let (s, c) = (state.clone(), calls.clone());
        std::thread::spawn(move || {
            while let Ok(cmd) = rx.recv_timeout(Duration::from_secs(30)) {
                let method = cmd.request.method.clone();
                c.lock()
                    .unwrap()
                    .push((method.clone(), cmd.request.params.clone()));
                let body = if method.ends_with(".state") {
                    json!({ "state": s.lock().unwrap().clone() })
                } else if method.ends_with(".spawn") {
                    std::thread::sleep(spawn_delay);
                    json!({ "child_surface_id": 42 })
                } else {
                    json!({ "ok": true })
                };
                let id = cmd.request.id.clone().unwrap_or(Value::Null);
                if cmd
                    .response_tx
                    .send(JsonRpcResponse::success(id, body))
                    .is_err()
                {
                    break;
                }
            }
        });
        Self { state, calls }
    }

    fn set(&self, s: &str) {
        *self.state.lock().unwrap() = s.to_string();
    }

    fn sent(&self, suffix: &str) -> Vec<Value> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(m, _)| m.ends_with(suffix))
            .map(|(_, p)| p.clone())
            .collect()
    }
}

fn contract(v: Value) -> TaskContract {
    serde_json::from_value(v).expect("contract")
}

/// Ready 인 v2 agent task 를 만든다.
fn create(ctx: &RunnerContext, session: AgentSession, c: TaskContract) -> Task {
    let seq = ctx.agent_seq.clone();
    ctx.with_memory(|mem| {
        TaskStore::new(mem, HOST_OWNER, seq.as_ref())
            .create_typed(
                TaskCreateOpts {
                    workspace_id: 1,
                    name: "review".into(),
                    command: TaskCommand::Agent {
                        provider: "claude".into(),
                        workspace_id: 1,
                        session,
                        instruction: "review".into(),
                        timeout_ms: None,
                    },
                    depends_on: vec![],
                    on_failure: OnFailure::Abort,
                    metadata: Value::Null,
                    now_ms: 0,
                },
                c,
            )
            .expect("create")
    })
}

/// runner 처럼 dispatch 뒤 Running 으로 만든다.
fn dispatch(exec: &mut HostExecutor, ctx: &RunnerContext, task: &Task) -> DispatchHandle {
    let DispatchOutcome::Started(h) = exec.dispatch(task) else {
        panic!("dispatch did not start");
    };
    let seq = ctx.agent_seq.clone();
    ctx.with_memory(|mem| {
        TaskStore::new(mem, HOST_OWNER, seq.as_ref())
            .set_state(1, &task.id, TaskState::Running, 1)
            .expect("running")
    });
    h
}

fn v2() -> TaskContract {
    contract(json!({ "contract_version": 2 }))
}

fn existing() -> AgentSession {
    AgentSession::Existing { surface_id: 7 }
}

#[test]
fn a_busy_session_gets_no_instruction_until_it_is_idle() {
    let (_td, ctx) = fresh_ctx();
    let fake = FakeProvider::install(&ctx, "active");
    let mut exec = HostExecutor::new(ctx.clone());
    let task = create(&ctx, existing(), v2());
    let h = dispatch(&mut exec, &ctx, &task);
    assert!(matches!(exec.poll(&h), PollOutcome::Active));
    assert!(
        fake.sent(".tell").is_empty(),
        "사용자의 턴에 끼어들지 않는다"
    );
    fake.set("idle");
    assert!(matches!(exec.poll(&h), PollOutcome::Active));
    assert_eq!(fake.sent(".tell").len(), 1);
    assert_eq!(fake.sent(".tell")[0]["surface_id"], 7);
    assert!(matches!(exec.poll(&h), PollOutcome::Active));
    assert_eq!(fake.sent(".tell").len(), 1, "한 번만 보낸다");
}

#[test]
fn an_idle_session_without_a_turn_end_report_does_not_succeed() {
    let (_td, ctx) = fresh_ctx();
    let _fake = FakeProvider::install(&ctx, "idle");
    let mut exec = HostExecutor::new(ctx.clone());
    let task = create(&ctx, existing(), v2());
    let h = dispatch(&mut exec, &ctx, &task);
    for _ in 0..3 {
        assert!(matches!(exec.poll(&h), PollOutcome::Active));
    }
    // 이전 턴의 종료 보고(시작 보고 전)는 이 회차에 적용하지 않는다.
    ctx.agent_turns.report(
        7,
        "claude",
        TurnEvent::Ended(TurnEnd::Answer(Some("old".into()))),
    );
    assert!(matches!(exec.poll(&h), PollOutcome::Active));
    ctx.agent_turns.report(7, "claude", TurnEvent::Started);
    ctx.agent_turns.report(
        7,
        "claude",
        TurnEvent::Ended(TurnEnd::Answer(Some("new".into()))),
    );
    let PollOutcome::Done(r) = exec.poll(&h) else {
        panic!("expected done");
    };
    assert_eq!(r.output.unwrap()[report::FINAL_ANSWER], "new");
}

#[test]
fn needs_input_is_recorded_as_awaiting_input_and_does_not_finish() {
    let (_td, ctx) = fresh_ctx();
    let fake = FakeProvider::install(&ctx, "idle");
    let mut exec = HostExecutor::new(ctx.clone());
    let task = create(&ctx, existing(), v2());
    let h = dispatch(&mut exec, &ctx, &task);
    assert!(matches!(exec.poll(&h), PollOutcome::Active));
    ctx.agent_turns.report(7, "claude", TurnEvent::Started);
    fake.set("needs_input");
    assert!(matches!(exec.poll(&h), PollOutcome::Active));
    let seq = ctx.agent_seq.clone();
    let stored = ctx.with_memory(|mem| {
        TaskStore::new(mem, HOST_OWNER, seq.as_ref())
            .get(1, &task.id)
            .unwrap()
            .unwrap()
    });
    let link = stored.attempt.unwrap().agent.expect("link");
    assert_eq!(link.surface_id, 7);
    assert!(link.awaiting_input_since.is_some());
    fake.set("exited");
    let PollOutcome::Failed(e) = exec.poll(&h) else {
        panic!("expected failure");
    };
    assert_eq!(
        FailureCode::parse_message(&e),
        Some(FailureCode::AgentExited)
    );
}

#[test]
fn a_second_task_on_the_same_session_waits_for_the_first() {
    let (_td, ctx) = fresh_ctx();
    let fake = FakeProvider::install(&ctx, "idle");
    let mut exec = HostExecutor::new(ctx.clone());
    let first = create(&ctx, existing(), v2());
    let second = create(&ctx, existing(), v2());
    let h1 = dispatch(&mut exec, &ctx, &first);
    let h2 = dispatch(&mut exec, &ctx, &second);
    exec.poll(&h1);
    exec.poll(&h2);
    assert_eq!(fake.sent(".tell").len(), 1, "두 번째 지시는 보내지 않는다");
    assert_eq!(ctx.agent_turns.holder(7), Some(first.id.clone()));
    exec.release_permit(&first.id);
    exec.poll(&h2);
    assert_eq!(fake.sent(".tell").len(), 2);
    assert_eq!(ctx.agent_turns.holder(7), Some(second.id.clone()));
}

#[test]
fn a_structured_output_needs_a_submission_and_the_new_session_is_told_how() {
    let (_td, ctx) = fresh_ctx();
    let fake = FakeProvider::install(&ctx, "active");
    let mut exec = HostExecutor::new(ctx.clone());
    let c = contract(json!({
        "contract_version": 2,
        "output_schema": { "type": "enum", "values": ["approve", "revise"] }
    }));
    let task = create(
        &ctx,
        AgentSession::New {
            parent_surface: 3,
            cwd: None,
        },
        c,
    );
    let h = dispatch(&mut exec, &ctx, &task);
    let spawned = fake.sent(".spawn");
    assert_eq!(spawned.len(), 1);
    assert_eq!(spawned[0]["surface"], 3);
    let prompt = spawned[0]["prompt"].as_str().unwrap();
    assert!(prompt.contains("tasty agent task-submit"), "{prompt}");
    assert!(prompt.contains(&format!("{}#1", task.id)), "{prompt}");
    // 새 세션의 첫 턴은 spawn 의 지시다. 시작 보고 없이도 종료를 받는다.
    ctx.agent_turns.report(
        42,
        "claude",
        TurnEvent::Ended(TurnEnd::Answer(Some("I think revise".into()))),
    );
    // 제출이 없으므로 보고에는 답만 있다. 결과 확정이 result_missing 으로 끝낸다.
    let PollOutcome::Done(r) = exec.poll(&h) else {
        panic!("expected a report without a submission");
    };
    let out = r.output.unwrap();
    assert!(out.get(report::SUBMITTED).is_none());
    assert_eq!(out[report::FINAL_ANSWER], "I think revise");
}

/// spawn 은 일반 호출의 응답 대기(5초)보다 늦게 답할 수 있다. 그래도 회차에 묶는다.
#[test]
fn a_slow_spawn_still_binds_the_new_session() {
    let (_td, ctx) = fresh_ctx();
    let _fake = FakeProvider::install_with(&ctx, "active", Duration::from_millis(5500));
    let mut exec = HostExecutor::new(ctx.clone());
    let task = create(
        &ctx,
        AgentSession::New {
            parent_surface: 3,
            cwd: None,
        },
        v2(),
    );
    dispatch(&mut exec, &ctx, &task);
    assert_eq!(ctx.agent_turns.holder(42), Some(task.id.clone()));
}

#[test]
fn a_restored_handle_without_a_binding_cannot_be_attributed() {
    let (_td, ctx) = fresh_ctx();
    let _fake = FakeProvider::install(&ctx, "idle");
    let mut exec = HostExecutor::new(ctx.clone());
    let h = DispatchHandle::AgentTurn {
        workspace_id: 1,
        task_id: "t".into(),
        attempt_id: "t#1".into(),
        provider: "claude".into(),
        surface_id: 42,
        needs_submission: false,
        deadline_ms: None,
        pending_instruction: None,
    };
    let PollOutcome::Failed(e) = exec.poll(&h) else {
        panic!("expected failure");
    };
    assert_eq!(
        FailureCode::parse_message(&e),
        Some(FailureCode::AgentUnavailable)
    );
}

#[test]
fn an_unsupported_provider_is_refused_at_dispatch() {
    let (_td, ctx) = fresh_ctx();
    let mut exec = HostExecutor::new(ctx.clone());
    let mut task = create(&ctx, existing(), v2());
    if let TaskCommand::Agent { provider, .. } = &mut task.command {
        *provider = "other".into();
    }
    let DispatchOutcome::PermanentFail(e) = exec.dispatch(&task) else {
        panic!("expected refusal");
    };
    assert_eq!(
        FailureCode::parse_message(&e),
        Some(FailureCode::AgentUnavailable)
    );
}
