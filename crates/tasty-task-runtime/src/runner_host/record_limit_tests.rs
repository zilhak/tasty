//! 결과 전 레코드 상한 — 상한을 넘는 정의·치환·입력은 실행하지 않고 실패로 끝난다.

use serde_json::{Value, json};
use tasty_agent::OnFailure;
use tasty_agent::TaskState;
use tasty_agent::runner::RunnerLoop;
use tasty_agent::task::contract::FailureStage;
use tasty_agent::task::record_limit::MAX_RECORD_BEFORE_RESULT_BYTES;
use tasty_agent::task::{TaskCreateOpts, TaskGraphSpec, TaskStore};

use super::tests::fresh_ctx;
use super::*;

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

/// 러너 tick 을 task 가 끝날 때까지 돌린다. 상한 검사는 실행 전에 끝나 프로세스를 띄우지 않는다.
fn run_to_end(ctx: &RunnerContext, id: &str) -> Task {
    let mut runner = RunnerLoop::new(HostExecutor::new(ctx.clone()));
    for n in 0..20 {
        let snapshot = store_op(ctx, |s| s.list(1).unwrap());
        let set_ctx = ctx.clone();
        let res_ctx = ctx.clone();
        runner.tick(
            1,
            10 + n,
            &snapshot,
            move |ws, id, st, n| store_op(&set_ctx, |s| s.set_state(ws, id, st, n).map(|_| ())),
            move |ws, id, c, n| store_op(&res_ctx, |s| s.complete(ws, id, c, n).map(|_| ())),
        );
        let t = get(ctx, id);
        if t.state.is_terminal() {
            return t;
        }
    }
    panic!("{id} did not finish: {:?}", get(ctx, id).state);
}

fn failure_message(t: &Task) -> &str {
    match &t.state {
        TaskState::Failed { error } => error,
        other => panic!("expected a failure, got {other:?}"),
    }
}

fn v1_run(
    name: &str,
    command: Vec<String>,
    depends_on: Vec<String>,
    metadata: Value,
) -> TaskCreateOpts {
    TaskCreateOpts {
        workspace_id: 1,
        name: name.into(),
        command: TaskCommand::Run {
            command,
            workspace_id: 1,
            cwd: None,
        },
        depends_on,
        on_failure: OnFailure::Abort,
        metadata,
        now_ms: 0,
    }
}

/// 상한이 생기기 전에 저장된 큰 정의는 실행하지 않고 실패로 끝낸다.
#[test]
fn a_definition_stored_over_the_limit_fails_without_running() {
    let (_td, ctx) = fresh_ctx();
    let id = store_op(&ctx, |s| {
        let mut t = s
            .create(v1_run("big", vec!["true".into()], vec![], Value::Null))
            .unwrap();
        // 저장소의 생성 검사를 거치지 않은 옛 레코드를 흉내 낸다.
        t.metadata = json!({"x": "m".repeat(MAX_RECORD_BEFORE_RESULT_BYTES)});
        s.put(&t).unwrap();
        t.id
    });
    let t = run_to_end(&ctx, &id);
    let error = failure_message(&t);
    assert!(
        error.contains("the definition makes the task record")
            && error.contains(&format!(
                "over the {MAX_RECORD_BEFORE_RESULT_BYTES} byte limit"
            )),
        "{error}"
    );
}

/// v1 출력 치환이 레코드를 상한 너머로 키우면 실행하지 않고 실패로 끝낸다. 치환한 command 는
/// 저장하지 않는다.
#[test]
fn a_substituted_command_over_the_limit_fails_without_running() {
    let (_td, ctx) = fresh_ctx();
    let (p, c) = store_op(&ctx, |s| {
        let p = s
            .create(v1_run("p", vec!["true".into()], vec![], Value::Null))
            .unwrap()
            .id;
        let placeholder = format!("${{task.{p}.output}}");
        let c = s
            .create(v1_run(
                "c",
                vec!["echo".into(), placeholder],
                vec![p.clone()],
                Value::Null,
            ))
            .unwrap()
            .id;
        s.set_state(1, &p, TaskState::Running, 1).unwrap();
        s.set_result(
            1,
            &p,
            TaskResult {
                exit_code: Some(0),
                output: Some(json!("o".repeat(800 * 1024))),
                error: None,
            },
        )
        .unwrap();
        s.set_state(1, &p, TaskState::Succeeded, 2).unwrap();
        (p, c)
    });
    let t = run_to_end(&ctx, &c);
    let error = failure_message(&t);
    assert!(
        error.starts_with("task output substitution: ")
            && error.contains("the command with substituted values makes the task record"),
        "{error}"
    );
    let TaskCommand::Run { command, .. } = &t.command else {
        panic!("run");
    };
    assert_eq!(command[1], format!("${{task.{p}.output}}"));
}

/// v2 입력이 레코드를 상한 너머로 키우면 실행하지 않고 입력 단계 실패로 끝낸다.
#[test]
fn a_resolved_input_over_the_limit_fails_at_the_input_stage() {
    let (_td, ctx) = fresh_ctx();
    let graph = json!({"contract_version": 2, "tasks": [
        {"id": "p", "command": {"kind": "custom", "ipc_method": "system.ping"},
         "output_schema": {"type": "object", "fields": {"msg": {"type": "string"}}}},
        {"id": "c", "metadata": {"pad": "m".repeat(300 * 1024)},
         "command": {"kind": "run", "workspace_id": 1, "command": ["true"]},
         "input_schema": {"type": "object", "fields": {"text": {"type": "string"}}},
         "bindings": {"text": {"from_task": "p", "pointer": "/msg"}},
         "input_mapping": {"args": ["/text"]}}
    ]});
    store_op(&ctx, |s| {
        let spec: TaskGraphSpec = serde_json::from_value(graph).unwrap();
        s.submit_graph(1, spec, 0).unwrap();
        let p = "p".to_string();
        s.set_state(1, &p, TaskState::Running, 1).unwrap();
        s.set_result(
            1,
            &p,
            TaskResult {
                exit_code: None,
                output: Some(json!({"msg": "x".repeat(250 * 1024)})),
                error: None,
            },
        )
        .unwrap();
        s.set_state(1, &p, TaskState::Succeeded, 2).unwrap();
    });
    let t = run_to_end(&ctx, "c");
    let typed = t.typed_result.as_ref().expect("typed result");
    let failure = typed.error.as_ref().expect("failure");
    assert_eq!(failure.stage, FailureStage::Input);
    assert!(
        failure
            .message
            .contains("the resolved input makes the task record"),
        "{}",
        failure.message
    );
    let snapshot = t.input_snapshot.as_ref().expect("snapshot");
    assert!(snapshot.failure.is_some());
    assert!(snapshot.execution.is_empty());
}
