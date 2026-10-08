//! 결과 전 레코드 상한 — 상한을 넘는 정의·치환·입력은 실행하지 않고 실패로 끝난다.

use serde_json::{Value, json};
use tasty_agent::OnFailure;
use tasty_agent::TaskState;
use tasty_agent::runner::RunnerLoop;
use tasty_agent::task::contract::FailureStage;
use tasty_agent::task::record_limit::{MAX_RECORD_BEFORE_RESULT_BYTES, REFUSED_TO_START};
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
    assert_refused_to_start(&t);
}

/// 시작하지 않은 실패는 고정된 사유를 상태에만 싣고 회차 기록·결과를 붙이지 않는다.
fn assert_refused_to_start(t: &Task) {
    assert_eq!(failure_message(t), REFUSED_TO_START);
    assert!(t.attempt.is_none(), "{:?}", t.attempt);
    assert!(t.result.is_none() && t.typed_result.is_none());
    assert!(t.started_at.is_none());
}

/// 직렬화가 정확히 `size` 바이트인 옛 v1 레코드를 저장한다(생성 검사를 거치지 않는다).
fn old_record_of(ctx: &RunnerContext, size: usize) -> String {
    store_op(ctx, |s| {
        let mut t = s
            .create(v1_run("big", vec!["true".into()], vec![], Value::Null))
            .unwrap();
        t.metadata = json!({"x": ""});
        let base = serde_json::to_vec(&t).unwrap().len();
        t.metadata = json!({"x": "m".repeat(size - base)});
        assert_eq!(serde_json::to_vec(&t).unwrap().len(), size);
        s.put(&t).unwrap();
        t.id
    })
}

/// 직렬화가 정확히 `size` 바이트인 옛 v2 레코드를 저장한다. 저장 값은 봉투만큼 더 크다.
fn old_v2_record_of(ctx: &RunnerContext, size: usize) -> String {
    store_op(ctx, |s| {
        let spec: TaskGraphSpec = serde_json::from_value(json!({"contract_version": 2, "tasks": [
            {"id": "big", "command": {"kind": "run", "command": ["true"], "workspace_id": 1}}
        ]}))
        .unwrap();
        s.submit_graph(1, spec, 0).unwrap();
        let mut t = s.get(1, &"big".to_string()).unwrap().unwrap();
        t.metadata = json!({"x": ""});
        let base = serde_json::to_vec(&t).unwrap().len();
        t.metadata = json!({"x": "m".repeat(size - base)});
        assert_eq!(serde_json::to_vec(&t).unwrap().len(), size);
        s.put(&t).unwrap();
        t.id
    })
}

/// 실패를 기록할 수 있는 최소 여유(memory 값 상한 − 레코드 직렬화 크기, 바이트). 상태가 `ready`
/// 에서 고정 사유의 `failed` 로 바뀌고 `finished_at` 이 붙는 만큼이며, v2 는 봉투가 더해진다.
const V1_MIN_SLACK: usize = 94;
const V2_MIN_SLACK: usize = 135;

/// memory 값 상한에 수백 바이트 안으로 닿은 옛 레코드도 실패를 기록하고 끝난다. 회차 기록과
/// 사유 사본이 붙던 때는 남은 몫이 약 0.5 KiB 이하이면 실패조차 기록하지 못해 Running 에 남았다.
#[test]
fn an_old_record_just_under_the_memory_entry_limit_still_fails() {
    for slack in [500, 400, V1_MIN_SLACK, 700, 2000] {
        let (_td, ctx) = fresh_ctx();
        let id = old_record_of(&ctx, tasty_memory::MAX_VALUE_BYTES - slack);
        assert_refused_to_start(&run_to_end(&ctx, &id));
    }
    for slack in [500, 400, V2_MIN_SLACK, 2000] {
        let (_td, ctx) = fresh_ctx();
        let id = old_v2_record_of(&ctx, tasty_memory::MAX_VALUE_BYTES - slack);
        assert_refused_to_start(&run_to_end(&ctx, &id));
    }
}

/// 최소 여유보다 작으면 실패도 기록하지 못한다. 시작하지 않으므로 Ready 로 남고 회차 기록도
/// 자원 점유도 없다.
#[test]
fn an_old_record_without_room_for_the_failure_stays_ready() {
    for (slack, v2) in [(V1_MIN_SLACK - 1, false), (V2_MIN_SLACK - 1, true)] {
        let (_td, ctx) = fresh_ctx();
        let size = tasty_memory::MAX_VALUE_BYTES - slack;
        let id = if v2 {
            old_v2_record_of(&ctx, size)
        } else {
            old_record_of(&ctx, size)
        };
        let mut runner = RunnerLoop::new(HostExecutor::new(ctx.clone()));
        for n in 0..3 {
            let snapshot = store_op(&ctx, |s| s.list(1).unwrap());
            let set_ctx = ctx.clone();
            let res_ctx = ctx.clone();
            runner.tick(
                1,
                10 + n,
                &snapshot,
                move |ws, id, st, n| store_op(&set_ctx, |s| s.set_state(ws, id, st, n).map(|_| ())),
                move |ws, id, c, n| store_op(&res_ctx, |s| s.complete(ws, id, c, n).map(|_| ())),
            );
        }
        let t = get(&ctx, &id);
        assert!(matches!(t.state, TaskState::Ready), "{:?}", t.state);
        assert!(t.attempt.is_none());
        // 지우기는 레코드를 다시 쓰지 않으므로 정리할 수 있다.
        store_op(&ctx, |s| s.delete_checked(1, &id, Default::default())).expect("delete");
        store_op(&ctx, |s| assert!(s.get(1, &id).unwrap().is_none()));
    }
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
        error.starts_with("task output substitution: task record too large: task ")
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
