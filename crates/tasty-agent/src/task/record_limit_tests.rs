use std::sync::atomic::AtomicU64;

use serde_json::{Value, json};
use tasty_memory::MemoryStore;
use tempfile::TempDir;

use super::*;
use crate::AgentError;
use crate::task::attempt::Completion;
use crate::task::binding::InputSnapshot;
use crate::task::contract::{CappedJson, EXECUTION_RESPONSE_CAP, FailureStage, TaskContract};
use crate::task::types::TypedValue;
use crate::task::{OnFailure, TaskCommand, TaskCreateOpts, TaskGraphSpec, TaskState, TaskStore};

fn fresh_store() -> (TempDir, MemoryStore, AtomicU64) {
    let td = tempfile::tempdir().expect("tempdir");
    let mem = MemoryStore::open(&td.path().join("mem.db")).expect("mem");
    (td, mem, AtomicU64::new(0))
}

fn custom(params: Value) -> TaskCreateOpts {
    TaskCreateOpts {
        workspace_id: 1,
        name: "c".into(),
        command: TaskCommand::Custom {
            ipc_method: "system.ping".into(),
            params,
            poll: None,
        },
        depends_on: vec![],
        on_failure: OnFailure::Abort,
        metadata: Value::Null,
        now_ms: 0,
    }
}

fn string_out() -> TaskContract {
    serde_json::from_value(json!({"contract_version": 2, "output_schema": {"type": "string"}}))
        .expect("contract")
}

fn size(t: &Task) -> usize {
    serde_json::to_vec(t).unwrap().len()
}

/// 레코드가 정확히 `target` 바이트가 되는 params 문자열 길이. 같은 모양의 task 를 한 번 만들어
/// 재고 지운다.
fn pad_for(store: &mut TaskStore, contract: Option<TaskContract>, target: usize) -> usize {
    let probe = 1024;
    let opts = custom(json!("p".repeat(probe)));
    let t = match contract {
        Some(c) => store.create_typed(opts, c),
        None => store.create(opts),
    }
    .unwrap();
    store.delete(1, &t.id).unwrap();
    probe + target - size(&t)
}

/// 정의가 상한을 넘으면 만들지 않고 이유를 알린다. 상한 안이면 만든다.
#[test]
fn a_definition_over_the_limit_is_refused_at_creation() {
    let (_td, mut mem, seq) = fresh_store();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let pad = pad_for(&mut store, None, MAX_RECORD_BEFORE_RESULT_BYTES);
    let fill = |extra: usize| json!("p".repeat(pad + extra));
    let e = store.create(custom(fill(1))).expect_err("over the limit");
    let AgentError::InvalidArgument(message) = &e else {
        panic!("{e:?}");
    };
    assert!(
        message.contains(&format!(
            "the definition makes the task record {} bytes, over the {MAX_RECORD_BEFORE_RESULT_BYTES} byte limit",
            MAX_RECORD_BEFORE_RESULT_BYTES + 1
        )),
        "{message}"
    );
    assert!(store.list(1).unwrap().is_empty());

    let t = store.create(custom(fill(0))).expect("at the limit");
    assert_eq!(size(&t), MAX_RECORD_BEFORE_RESULT_BYTES);
}

/// 그래프 제출은 상한을 넘는 task 를 그 위치와 함께 거절하고 아무것도 저장하지 않는다.
#[test]
fn a_graph_task_over_the_limit_is_refused_with_its_location() {
    let (_td, mut mem, seq) = fresh_store();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let spec: TaskGraphSpec = serde_json::from_value(json!({"contract_version": 2, "tasks": [
        {"id": "a", "command": {"kind": "custom", "ipc_method": "system.ping"}},
        {"id": "b", "command": {"kind": "custom", "ipc_method": "system.ping"},
         "metadata": {"pad": "m".repeat(MAX_RECORD_BEFORE_RESULT_BYTES)}}
    ]}))
    .unwrap();
    let e = store.submit_graph(1, spec, 0).expect_err("over the limit");
    let AgentError::TypeContract(f) = &e else {
        panic!("{e:?}");
    };
    assert_eq!(f.stage, FailureStage::Input);
    assert_eq!(f.location.as_deref(), Some("/tasks/1"));
    assert!(
        f.message
            .starts_with("task record too large: task b: the definition makes the task record"),
        "{}",
        f.message
    );
    assert!(store.list(1).unwrap().is_empty());
}

/// 해석한 입력이 레코드를 상한 너머로 키우면 값 대신 입력 실패를 snapshot 에 남긴다.
#[test]
fn a_resolved_input_over_the_limit_is_kept_as_an_input_failure() {
    let (_td, mut mem, seq) = fresh_store();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let c: TaskContract = serde_json::from_value(json!({"contract_version": 2,
        "input_schema": {"type": "json"}, "bindings": {}}))
    .unwrap();
    let t = store.create_typed(custom(Value::Null), c).unwrap();
    let snapshot = |n: usize| InputSnapshot {
        resolved_at: 5,
        value: TypedValue::Json(json!("v".repeat(n))),
        sources: vec![],
        execution: Default::default(),
        failure: None,
    };
    let kept = store.set_input_snapshot(1, &t.id, snapshot(1024)).unwrap();
    assert!(kept.input_snapshot.unwrap().failure.is_none());

    let t = store
        .set_input_snapshot(1, &t.id, snapshot(MAX_RECORD_BEFORE_RESULT_BYTES))
        .unwrap();
    let s = t.input_snapshot.as_ref().unwrap();
    assert_eq!(s.value, TypedValue::Null);
    let f = s.failure.as_ref().expect("input failure");
    assert_eq!(f.stage, FailureStage::Input);
    assert_eq!(f.location.as_deref(), Some("/bindings"));
    assert!(
        f.message.starts_with("task record too large: task ")
            && f.message
                .contains("the resolved input makes the task record"),
        "{}",
        f.message
    );
    assert_eq!(store.get(1, &t.id).unwrap().as_ref(), Some(&t));
}

/// 결과 전 레코드가 상한에 닿아도, 이스케이프가 가장 큰 사유로 줄인 실패와 가장 큰 접수 응답
/// 머리를 함께 실은 레코드가 memory 값 상한 안에 든다. 그래서 줄인 보고는 늘 기록된다.
#[test]
fn a_shrunk_failure_always_fits_beside_a_definition_at_the_limit() {
    let (_td, mut mem, seq) = fresh_store();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let pad = pad_for(
        &mut store,
        Some(string_out()),
        MAX_RECORD_BEFORE_RESULT_BYTES,
    );
    let t = store
        .create_typed(custom(json!("p".repeat(pad))), string_out())
        .unwrap();
    assert_eq!(size(&t), MAX_RECORD_BEFORE_RESULT_BYTES);

    store.set_state(1, &t.id, TaskState::Running, 1).unwrap();
    let attempt = store.get(1, &t.id).unwrap().unwrap().attempt.unwrap().id;
    // JSON 텍스트를 다시 이스케이프하면 2배가 되는 머리.
    let accepted =
        CappedJson::capture_within(&json!("\"".repeat(EXECUTION_RESPONSE_CAP)), 1024 * 64);
    assert!(accepted.truncated);
    store.set_accepted(1, &t.id, accepted).unwrap();

    let report = Completion::failed(Some(attempt), "\u{1}".repeat(1100 * 1024));
    let e = store
        .complete(1, &t.id, report.clone(), 2)
        .expect_err("too large to store");
    let shrunk = crate::runner::shrink_too_large_completion(&e, &report).expect("shrunk");
    let t = store.complete(1, &t.id, shrunk, 3).expect("stored").task;
    assert!(matches!(t.state, TaskState::Failed { .. }));
    let record = size(&t);
    assert!(record <= tasty_memory::MAX_VALUE_BYTES, "{record}");
}
