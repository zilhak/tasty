//! v2 입력 binding 의 실행 경계 — 그래프 활성화 전 tick, 입력 snapshot, argv·stdin·params 매핑.

use serde_json::{Value, json};
use tasty_agent::TaskState;
use tasty_agent::runner::RunnerLoop;
use tasty_agent::task::contract::FailureStage;
use tasty_agent::task::{TaskGraphSpec, TaskStore};

use super::tests::fresh_ctx;
use super::*;

fn spec(v: Value) -> TaskGraphSpec {
    serde_json::from_value(v).expect("graph spec")
}

fn store_op<R>(ctx: &RunnerContext, f: impl FnOnce(&mut TaskStore) -> R) -> R {
    ctx.with_memory(|mem| {
        let seq = ctx.agent_seq.clone();
        let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
        f(&mut store)
    })
}

fn get(ctx: &RunnerContext, id: &str) -> Task {
    store_op(ctx, |s| s.get(1, &id.to_string()).unwrap().expect("task"))
}

/// 러너 스레드의 한 tick 과 같은 일을 한다. 저장소 snapshot 을 읽고 상태·결과를 기록한다.
fn tick(ctx: &RunnerContext, runner: &mut RunnerLoop<HostExecutor>, now: u64) {
    let snapshot = store_op(ctx, |s| s.list(1).unwrap());
    let set_ctx = ctx.clone();
    let res_ctx = ctx.clone();
    runner.tick(
        1,
        now,
        &snapshot,
        move |ws, id, st, n| store_op(&set_ctx, |s| s.set_state(ws, id, st, n).map(|_| ())),
        move |ws, id, c, n| store_op(&res_ctx, |s| s.complete(ws, id, c, n).map(|_| ())),
    );
}

fn tick_until_terminal(ctx: &RunnerContext, runner: &mut RunnerLoop<HostExecutor>, id: &str) {
    for n in 0..200 {
        tick(ctx, runner, 10 + n);
        if get(ctx, id).state.is_terminal() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("{id} did not finish: {:?}", get(ctx, id).state);
}

fn finish(ctx: &RunnerContext, id: &str, output: Value) {
    store_op(ctx, |s| {
        let id = id.to_string();
        s.set_state(1, &id, TaskState::Running, 1).unwrap();
        s.set_result(
            1,
            &id,
            TaskResult {
                exit_code: None,
                output: Some(output),
                error: None,
            },
        )
        .unwrap();
        s.set_state(1, &id, TaskState::Succeeded, 2).unwrap();
    });
}

#[cfg(unix)]
#[test]
fn a_tick_between_staging_and_activation_dispatches_nothing() {
    let (_td, ctx) = fresh_ctx();
    let mut runner = RunnerLoop::new(HostExecutor::new(ctx.clone()));
    let graph = json!({"contract_version": 2, "tasks": [
        {"id": "first", "command": {"kind": "run", "command": ["true"], "workspace_id": 1}},
        {"id": "second", "command": {"kind": "run", "command": ["true"], "workspace_id": 1},
         "depends_on": ["first"]}
    ]});
    let plan = store_op(&ctx, |s| {
        let plan = s.plan_graph(1, spec(graph), 0).unwrap();
        s.stage_graph(&plan).unwrap();
        plan
    });
    // 저장은 끝났지만 활성화 전이다. 실제 tick 이 돌아도 아무것도 실행되지 않는다.
    for n in 0..3 {
        tick(&ctx, &mut runner, n);
    }
    assert_eq!(get(&ctx, "first").state, TaskState::Waiting);
    assert_eq!(get(&ctx, "first").started_at, None);

    store_op(&ctx, |s| s.activate_graph(&plan, 5).unwrap());
    tick_until_terminal(&ctx, &mut runner, "second");
    assert_eq!(get(&ctx, "first").state, TaskState::Succeeded);
    assert_eq!(get(&ctx, "second").state, TaskState::Succeeded);
}

#[cfg(unix)]
#[test]
fn run_inputs_reach_argv_and_stdin_as_values_without_reinterpretation() {
    let (_td, ctx) = fresh_ctx();
    let mut runner = RunnerLoop::new(HostExecutor::new(ctx.clone()));
    let tricky = "${task.p.output} $(echo injected) ${lease.resource}";
    let graph = json!({"contract_version": 2, "tasks": [
        {"id": "p", "command": {"kind": "custom", "ipc_method": "system.ping"},
         "output_schema": {"type": "object", "fields": {"msg": {"type": "string"}}}},
        {"id": "c",
         "command": {"kind": "run", "workspace_id": 1,
             "command": ["sh", "-c", "printf '%s|' \"$@\"; cat", "sh"]},
         "input_schema": {"type": "object", "fields": {
             "text": {"type": "string"}, "n": {"type": "int64"}}},
         "bindings": {"text": {"from_task": "p", "pointer": "/msg"},
                      "n": {"literal": "9007199254740993"}},
         "input_mapping": {"args": ["/text", "/n"], "stdin": true}}
    ]});
    store_op(&ctx, |s| s.submit_graph(1, spec(graph), 0).unwrap());
    finish(&ctx, "p", json!({"msg": tricky}));
    tick_until_terminal(&ctx, &mut runner, "c");

    let c = get(&ctx, "c");
    assert_eq!(c.state, TaskState::Succeeded, "{:?}", c.result);
    let stdout = c
        .typed_result
        .as_ref()
        .unwrap()
        .raw
        .execution
        .as_ref()
        .unwrap()["stdout"]["text"]
        .as_str()
        .unwrap()
        .to_string();
    let wire = format!(r#"{{"n":"9007199254740993","text":{}}}"#, json!(tricky));
    assert_eq!(stdout, format!("{tricky}|9007199254740993|{wire}"));

    // 원본 command 는 그대로이고 해석한 실행값은 snapshot 에 따로 있다.
    let TaskCommand::Run { command, .. } = &c.command else {
        panic!("run");
    };
    assert_eq!(command.len(), 4);
    let snap = c.input_snapshot.expect("snapshot");
    assert_eq!(
        snap.execution.args,
        vec![tricky.to_string(), "9007199254740993".into()]
    );
    assert!(snap.execution.stdin);
    assert_eq!(snap.sources[0].from_task, "p");
}

#[test]
fn custom_params_receive_typed_values_at_declared_pointers() {
    let (_td, ctx) = fresh_ctx();
    let mut exec = HostExecutor::new(ctx.clone());
    let graph = json!({"contract_version": 2, "tasks": [
        {"id": "c", "command": {"kind": "custom", "ipc_method": "system.ping",
             "params": {"keep": "$(not run)", "nested": {}}},
         "input_schema": {"type": "object", "fields": {
             "n": {"type": "int64"}, "rows": {"type": "list", "items": {"type": "string"}}}},
         "bindings": {"n": {"literal": 9007199254740993_i64},
                      "rows": {"literal": ["a", "$(b)"]}},
         "input_mapping": {"params": {"/surface": "/n", "/nested/rows": "/rows"}}}
    ]});
    store_op(&ctx, |s| s.submit_graph(1, spec(graph), 0).unwrap());
    let c = get(&ctx, "c");
    // injector 가 없어 IPC 는 실패하지만 입력 해석과 저장은 그 전에 끝난다.
    match exec.dispatch(&c) {
        DispatchOutcome::PermanentFail(e) => assert!(e.contains(INJECTOR_UNINIT_MSG), "{e}"),
        other => panic!("expected an injector failure, got {other:?}"),
    }
    let snap = get(&ctx, "c").input_snapshot.expect("snapshot");
    assert_eq!(
        snap.execution.params,
        Some(
            json!({"keep": "$(not run)", "nested": {"rows": ["a", "$(b)"]},
                    "surface": 9007199254740993_i64})
        )
    );
}

#[test]
fn an_unresolvable_input_fails_at_the_input_stage_without_running() {
    let (_td, ctx) = fresh_ctx();
    let mut runner = RunnerLoop::new(HostExecutor::new(ctx.clone()));
    let graph = json!({"contract_version": 2, "tasks": [
        {"id": "p", "command": {"kind": "custom", "ipc_method": "system.ping"}},
        {"id": "c",
         "command": {"kind": "run", "workspace_id": 1, "command": ["touch", "should-not-exist"]},
         "input_schema": {"type": "object", "fields": {"n": {"type": "int64"}}},
         "bindings": {"n": {"from_task": "p", "pointer": "/count", "convert": "assert"}},
         "input_mapping": {"args": ["/n"]}}
    ]});
    store_op(&ctx, |s| s.submit_graph(1, spec(graph), 0).unwrap());
    // json 출력의 assert 는 실행 시 검증한다. 값이 없으면 실행하지 않는다.
    finish(&ctx, "p", json!({"other": 1}));
    tick_until_terminal(&ctx, &mut runner, "c");
    let c = get(&ctx, "c");
    assert!(matches!(c.state, TaskState::Failed { .. }), "{:?}", c.state);
    let err = c.typed_result.unwrap().error.unwrap();
    assert_eq!(err.stage, FailureStage::Input);
    assert_eq!(err.location.as_deref(), Some("/bindings/n"));
    assert!(err.message.contains("/count"), "{}", err.message);
    assert!(c.input_snapshot.unwrap().failure.is_some());
}
