//! v2 계약 task 의 실행 경계 — Run 허용 종료 코드와 v2 reduce.

use serde_json::{Value, json};
use tasty_agent::task::contract::TaskContract;
use tasty_agent::task::{TaskCreateOpts, TaskStore};
use tasty_agent::{OnFailure, ReducerStrategy, TaskState};

use super::tests::fresh_ctx;
use super::*;

fn contract(v: Value) -> TaskContract {
    serde_json::from_value(v).expect("contract")
}

fn opts(name: &str, command: TaskCommand) -> TaskCreateOpts {
    TaskCreateOpts {
        workspace_id: 1,
        name: name.into(),
        command,
        depends_on: vec![],
        on_failure: OnFailure::Abort,
        metadata: Value::Null,
        now_ms: 0,
    }
}

#[cfg(unix)]
fn wait_outcome(exec: &mut HostExecutor, handle: &DispatchHandle) -> PollOutcome {
    for _ in 0..100 {
        match exec.poll(handle) {
            PollOutcome::Active => std::thread::sleep(Duration::from_millis(50)),
            other => return other,
        }
    }
    panic!("run did not finish");
}

fn exit_seven() -> TaskCommand {
    TaskCommand::Run {
        command: vec!["sh".into(), "-c".into(), "echo out; exit 7".into()],
        workspace_id: 1,
        cwd: None,
    }
}

#[cfg(unix)]
#[test]
fn v2_run_accepts_declared_exit_codes_and_reports_the_code_as_output() {
    let (_td, ctx) = fresh_ctx();
    let mut exec = HostExecutor::new(ctx.clone());
    let allowed = contract(json!({"contract_version": 2, "allowed_exit_codes": [0, 7]}));
    let strict = contract(json!({"contract_version": 2}));
    let (seven_ok, seven_strict) = ctx.with_memory(|mem| {
        let seq = ctx.agent_seq.clone();
        let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
        let a = store
            .create_typed(opts("allowed", exit_seven()), allowed)
            .unwrap();
        let b = store
            .create_typed(opts("strict", exit_seven()), strict)
            .unwrap();
        (a, b)
    });

    let outcome_of = |exec: &mut HostExecutor, task: &Task| match exec.dispatch(task) {
        DispatchOutcome::Started(h) => wait_outcome(exec, &h),
        other => panic!("expected Started, got {other:?}"),
    };
    let ok = outcome_of(&mut exec, &seven_ok);
    let PollOutcome::Done(result) = ok else {
        panic!("declared exit 7 should succeed: {ok:?}");
    };
    assert_eq!(result.exit_code, Some(7));
    let strict_outcome = outcome_of(&mut exec, &seven_strict);
    assert!(
        matches!(strict_outcome, PollOutcome::Failed(_)),
        "exit 7 outside the declared list fails: {strict_outcome:?}"
    );

    let finished = ctx.with_memory(|mem| {
        let seq = ctx.agent_seq.clone();
        let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
        store
            .set_state(1, &seven_ok.id, TaskState::Running, 1)
            .unwrap();
        store.set_result(1, &seven_ok.id, result).unwrap();
        store
            .set_state(1, &seven_ok.id, TaskState::Succeeded, 2)
            .unwrap()
            .0
    });
    assert_eq!(finished.state, TaskState::Succeeded);
    let typed = finished.typed_result.unwrap();
    assert_eq!(typed.output.to_internal(), json!(7));
    let raw = typed.raw.execution.unwrap();
    assert_eq!(raw["stdout"]["text"], json!("out\n"));
}

#[cfg(unix)]
#[test]
fn v1_run_keeps_treating_nonzero_exit_as_failure() {
    let (_td, ctx) = fresh_ctx();
    let mut exec = HostExecutor::new(ctx.clone());
    let task = ctx.with_memory(|mem| {
        let seq = ctx.agent_seq.clone();
        let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
        store.create(opts("v1", exit_seven())).unwrap()
    });
    let outcome = match exec.dispatch(&task) {
        DispatchOutcome::Started(h) => wait_outcome(&mut exec, &h),
        other => panic!("expected Started, got {other:?}"),
    };
    assert!(matches!(outcome, PollOutcome::Failed(_)), "{outcome:?}");
}

fn finished_custom(store: &mut TaskStore, name: &str, c: Option<TaskContract>, out: Value) -> Task {
    let cmd = TaskCommand::Custom {
        ipc_method: "system.ping".into(),
        params: Value::Null,
        poll: None,
    };
    let t = match c {
        Some(c) => store.create_typed(opts(name, cmd), c).unwrap(),
        None => store.create(opts(name, cmd)).unwrap(),
    };
    store.set_state(1, &t.id, TaskState::Running, 1).unwrap();
    store
        .set_result(
            1,
            &t.id,
            TaskResult {
                exit_code: None,
                output: Some(out),
                error: None,
            },
        )
        .unwrap();
    store
        .set_state(1, &t.id, TaskState::Succeeded, 2)
        .unwrap()
        .0
}

#[test]
fn v2_reduce_dispatch_uses_typed_inputs_and_v1_reduce_is_unchanged() {
    let (_td, ctx) = fresh_ctx();
    let mut exec = HostExecutor::new(ctx.clone());
    let text = contract(json!({"contract_version": 2, "output_schema": {"type": "string"}}));
    let (typed_all, typed_concat, legacy_all) = ctx.with_memory(|mem| {
        let seq = ctx.agent_seq.clone();
        let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
        let a = finished_custom(&mut store, "a", Some(text.clone()), json!("x"));
        let b = finished_custom(&mut store, "b", Some(text.clone()), json!("y"));
        let ids = vec![a.id.clone(), b.id.clone()];
        let reduce = |strategy| TaskCommand::Reduce {
            inputs: ids.clone(),
            strategy,
        };
        let v2 = contract(json!({"contract_version": 2}));
        (
            store
                .create_typed(opts("all", reduce(ReducerStrategy::All)), v2.clone())
                .unwrap(),
            store
                .create_typed(opts("concat", reduce(ReducerStrategy::ConcatText)), v2)
                .unwrap(),
            {
                // v1 reduce 는 v1 입력만 받는다.
                let c = finished_custom(&mut store, "c", None, json!("x"));
                let d = finished_custom(&mut store, "d", None, json!("y"));
                let legacy = TaskCommand::Reduce {
                    inputs: vec![c.id, d.id],
                    strategy: ReducerStrategy::All,
                };
                store.create(opts("legacy", legacy)).unwrap()
            },
        )
    });
    let immediate = |exec: &mut HostExecutor, t: &Task| match exec.dispatch(t) {
        DispatchOutcome::Started(DispatchHandle::ReduceImmediate(r)) => r,
        other => panic!("expected ReduceImmediate, got {other:?}"),
    };
    let all = immediate(&mut exec, &typed_all);
    assert_eq!(all.exit_code, None);
    let records = all.output.unwrap();
    assert_eq!(records[0]["has_output"], json!(true));
    assert_eq!(records[0]["output"], json!("x"));
    assert_eq!(records[1]["state"], json!("succeeded"));
    assert_eq!(
        immediate(&mut exec, &typed_concat).output,
        Some(json!("xy"))
    );
    // v1 reduce all 은 출력 값만 담은 배열이다.
    assert_eq!(
        immediate(&mut exec, &legacy_all).output,
        Some(json!(["x", "y"]))
    );
}

fn ref_to(id: &str) -> TaskCommand {
    TaskCommand::Custom {
        ipc_method: "system.ping".into(),
        params: json!({ "x": format!("${{task.{id}.output}}") }),
        poll: None,
    }
}

#[test]
fn output_placeholders_cannot_read_typed_results() {
    let (_td, ctx) = fresh_ctx();
    let mut exec = HostExecutor::new(ctx.clone());
    let (typed, legacy, stored_ref) = ctx.with_memory(|mem| {
        let seq = ctx.agent_seq.clone();
        let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
        let v2 = contract(json!({"contract_version": 2}));
        let typed = store.create_typed(opts("v2run", exit_seven()), v2).unwrap();
        let legacy = finished_custom(&mut store, "v1", None, json!({"id": 1}));
        // 생성 검사를 거치지 않고 저장된 참조도 실행 직전에 막히는지 본다.
        let mut o = opts("stored", ref_to(&typed.id));
        o.depends_on = vec![typed.id.clone()];
        let stored_ref = store.create(o).unwrap();
        (typed, legacy, stored_ref)
    });

    let err = ctx
        .with_memory(|mem| {
            let seq = ctx.agent_seq.clone();
            let store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            crate::task::reject_v1_reads_of_typed(&store, 1, &ref_to(&typed.id), &OnFailure::Abort)
        })
        .unwrap_err();
    let tasty_agent::AgentError::TypeContract(f) = err else {
        panic!("expected a type contract error");
    };
    assert_eq!(f.task_id.as_deref(), Some(typed.id.as_str()));
    let ok = ctx.with_memory(|mem| {
        let seq = ctx.agent_seq.clone();
        let store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
        crate::task::reject_v1_reads_of_typed(&store, 1, &ref_to(&legacy.id), &OnFailure::Abort)
    });
    assert!(ok.is_ok(), "v1 producers stay readable: {ok:?}");

    match exec.dispatch(&stored_ref) {
        DispatchOutcome::PermanentFail(e) => assert!(e.contains("typed v2"), "{e}"),
        other => panic!("expected PermanentFail, got {other:?}"),
    }
}

#[test]
fn v1_reduce_cannot_take_typed_inputs() {
    let (_td, ctx) = fresh_ctx();
    let mut exec = HostExecutor::new(ctx.clone());
    let (typed, legacy, stored_reduce) = ctx.with_memory(|mem| {
        let seq = ctx.agent_seq.clone();
        let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
        let v2 = contract(json!({"contract_version": 2}));
        let typed = finished_custom(&mut store, "v2", Some(v2), json!(1));
        let legacy = finished_custom(&mut store, "v1", None, json!(2));
        // 생성 검사를 거치지 않고 저장된 v1 reduce 도 실행 직전에 막히는지 본다.
        let reduce = TaskCommand::Reduce {
            inputs: vec![legacy.id.clone(), typed.id.clone()],
            strategy: ReducerStrategy::All,
        };
        let stored_reduce = store.create(opts("stored", reduce)).unwrap();
        (typed, legacy, stored_reduce)
    });
    let check = |inputs: Vec<String>| {
        ctx.with_memory(|mem| {
            let seq = ctx.agent_seq.clone();
            let store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            crate::task::reject_v1_reads_of_typed(
                &store,
                1,
                &TaskCommand::Reduce {
                    inputs,
                    strategy: ReducerStrategy::All,
                },
                &OnFailure::Abort,
            )
        })
    };
    let err = check(vec![legacy.id.clone(), typed.id.clone()]).unwrap_err();
    let tasty_agent::AgentError::TypeContract(f) = err else {
        panic!("expected a type contract error");
    };
    assert_eq!(f.task_id.as_deref(), Some(typed.id.as_str()));
    assert!(check(vec![legacy.id.clone()]).is_ok());

    match exec.dispatch(&stored_reduce) {
        DispatchOutcome::PermanentFail(e) => assert!(e.contains("v1 reduce"), "{e}"),
        other => panic!("expected PermanentFail, got {other:?}"),
    }
}
