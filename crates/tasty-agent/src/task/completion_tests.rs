//! v2 완료 경계 — 회차, 한 번의 쓰기, 중복·충돌·옛 회차 보고, 쓰기 실패 뒤의 하류.

use std::sync::atomic::AtomicU64;

use serde_json::{Value, json};
use tasty_memory::MemoryStore;
use tempfile::TempDir;

use super::store::{FAIL_ACTIVATION_PUT, FAIL_COMPLETION_PUT};
use super::*;
use crate::{AgentError, CompletionRejection};

fn fresh() -> (TempDir, MemoryStore, AtomicU64) {
    let td = tempfile::tempdir().expect("tempdir");
    let mem = MemoryStore::open(&td.path().join("mem.db")).expect("mem");
    (td, mem, AtomicU64::new(0))
}

fn custom() -> Value {
    json!({"kind": "custom", "ipc_method": "system.ping", "params": {}})
}

/// producer(int64 n 출력) → consumer(n 입력).
fn submit_pair(store: &mut TaskStore) {
    let spec: TaskGraphSpec = serde_json::from_value(json!({
    "contract_version": 2,
    "tasks": [
        {"id": "p", "command": custom(),
         "output_schema": {"type": "object", "fields": {"n": {"type": "int64"}}}},
        {"id": "c", "command": custom(),
         "input_schema": {"type": "object", "fields": {"n": {"type": "int64"}}},
         "bindings": {"n": {"from_task": "p", "pointer": "/n"}}}
    ]}))
    .expect("spec");
    store.submit_graph(1, spec, 0).expect("submit");
}

fn get(store: &TaskStore, id: &str) -> Task {
    store.get(1, &id.to_string()).unwrap().expect("task")
}

fn ok(n: i64) -> TaskResult {
    TaskResult {
        exit_code: None,
        output: Some(json!({"n": n})),
        error: None,
    }
}

fn start(store: &mut TaskStore, id: &str, now: u64) -> String {
    store
        .set_state(1, &id.to_string(), TaskState::Running, now)
        .expect("running");
    get(store, id).attempt.expect("attempt").id
}

fn rejection(e: AgentError) -> CompletionRejection {
    match e {
        AgentError::CompletionRejected { reason, .. } => reason,
        other => panic!("expected a rejected completion, got {other:?}"),
    }
}

#[test]
fn each_run_gets_a_new_attempt_and_v1_tasks_get_none() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    submit_pair(&mut store);
    assert_eq!(start(&mut store, "p", 1), "p#1");
    let p = "p".to_string();
    store
        .complete(
            1,
            &p,
            Completion::failed(Some("p#1".into()), "boom".into()),
            2,
        )
        .expect("failed");
    store.retry(1, &p, false, 3).expect("retry");
    let p_task = get(&store, "p");
    assert_eq!(p_task.attempt.as_ref().map(|a| a.number), Some(1));
    assert_eq!(start(&mut store, "p", 4), "p#2");

    let v1 = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "v1".into(),
            command: TaskCommand::WaitBarrier { name: "b".into() },
            depends_on: vec![],
            on_failure: OnFailure::default(),
            metadata: Value::Null,
            now_ms: 5,
        })
        .expect("v1");
    store.set_state(1, &v1.id, TaskState::Running, 6).unwrap();
    assert!(get(&store, &v1.id).attempt.is_none());
}

#[test]
fn a_completion_writes_result_and_terminal_state_together_and_releases_the_consumer() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    submit_pair(&mut store);
    let attempt = start(&mut store, "p", 1);
    let receipt = store
        .complete(
            1,
            &"p".into(),
            Completion::succeeded(Some(attempt), ok(3)),
            2,
        )
        .expect("complete");
    assert!(!receipt.duplicate);
    assert_eq!(receipt.task.state, TaskState::Succeeded);
    assert!(receipt.task.typed_result.is_some());
    assert!(receipt.task.attempt.unwrap().completion.is_some());
    assert_eq!(get(&store, "c").state, TaskState::Ready);
    assert!(receipt.transitioned.iter().any(|t| t.id == "c"));
}

#[test]
fn an_output_that_breaks_the_contract_fails_in_the_same_write() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    submit_pair(&mut store);
    start(&mut store, "p", 1);
    let bad = TaskResult {
        exit_code: None,
        output: Some(json!({"n": "three"})),
        error: None,
    };
    let receipt = store
        .complete(1, &"p".into(), Completion::succeeded(None, bad), 2)
        .expect("recorded");
    assert!(matches!(receipt.task.state, TaskState::Failed { .. }));
    assert_ne!(get(&store, "c").state, TaskState::Ready);
}

#[test]
fn a_failed_write_changes_nothing_and_the_same_report_can_be_sent_again() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    submit_pair(&mut store);
    let attempt = start(&mut store, "p", 1);
    let report = Completion::succeeded(Some(attempt), ok(3));
    FAIL_COMPLETION_PUT.with(|f| f.set(true));
    store
        .complete(1, &"p".into(), report.clone(), 2)
        .expect_err("injected failure");
    let p = get(&store, "p");
    assert_eq!(p.state, TaskState::Running);
    assert!(p.result.is_none() && p.typed_result.is_none());
    assert_eq!(get(&store, "c").state, TaskState::Waiting);

    let receipt = store.complete(1, &"p".into(), report, 3).expect("retry");
    assert!(!receipt.duplicate);
    assert_eq!(get(&store, "c").state, TaskState::Ready);
}

#[test]
fn the_same_report_twice_answers_the_same_and_a_different_one_is_refused() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    submit_pair(&mut store);
    let attempt = start(&mut store, "p", 1);
    let first = store
        .complete(
            1,
            &"p".into(),
            Completion::succeeded(Some(attempt.clone()), ok(3)),
            2,
        )
        .expect("first");
    let again = store
        .complete(
            1,
            &"p".into(),
            Completion::succeeded(Some(attempt.clone()), ok(3)),
            9,
        )
        .expect("duplicate");
    assert!(again.duplicate);
    assert_eq!(again.task, first.task);
    // 회차 id 없이 같은 내용을 내도 같은 회차의 재전송이다.
    assert!(
        store
            .complete(1, &"p".into(), Completion::succeeded(None, ok(3)), 9)
            .expect("duplicate without id")
            .duplicate
    );
    let different = store
        .complete(
            1,
            &"p".into(),
            Completion::succeeded(Some(attempt), ok(4)),
            9,
        )
        .expect_err("different report");
    assert_eq!(rejection(different), CompletionRejection::DifferentReport);
    assert_eq!(get(&store, "p").finished_at, Some(2));
}

#[test]
fn a_report_for_an_earlier_attempt_does_not_finish_the_current_one() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    submit_pair(&mut store);
    let p = "p".to_string();
    start(&mut store, "p", 1);
    store
        .complete(
            1,
            &p,
            Completion::failed(Some("p#1".into()), "boom".into()),
            2,
        )
        .unwrap();
    let consumer_before = get(&store, "c").state;
    store.retry(1, &p, false, 3).unwrap();
    start(&mut store, "p", 4);
    let stale = store
        .complete(1, &p, Completion::succeeded(Some("p#1".into()), ok(3)), 5)
        .expect_err("stale");
    assert_eq!(rejection(stale), CompletionRejection::StaleAttempt);
    assert_eq!(get(&store, "p").state, TaskState::Running);
    assert_eq!(get(&store, "c").state, consumer_before);
}

#[test]
fn a_terminal_task_finished_without_an_attempt_record_refuses_reports() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    submit_pair(&mut store);
    let p = "p".to_string();
    start(&mut store, "p", 1);
    store.cancel(1, &p, 2).expect("cancel");
    let e = store
        .complete(1, &p, Completion::succeeded(None, ok(3)), 3)
        .expect_err("cancelled");
    assert_eq!(rejection(e), CompletionRejection::AlreadyTerminal);
}

/// 완료 쓰기 뒤 하류 반영이 끊긴 상태를 만든다.
fn interrupted_after_the_write(store: &mut TaskStore) -> Completion {
    submit_pair(store);
    let attempt = start(store, "p", 1);
    let report = Completion::succeeded(Some(attempt), ok(3));
    FAIL_ACTIVATION_PUT.with(|f| f.set(true));
    store
        .complete(1, &"p".into(), report.clone(), 2)
        .expect_err("propagation interrupted");
    assert_eq!(get(store, "p").state, TaskState::Succeeded);
    assert_eq!(get(store, "c").state, TaskState::Waiting);
    report
}

#[test]
fn resending_the_report_finishes_an_interrupted_propagation() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let report = interrupted_after_the_write(&mut store);
    let receipt = store.complete(1, &"p".into(), report, 3).expect("resend");
    assert!(receipt.duplicate);
    assert_eq!(get(&store, "c").state, TaskState::Ready);
}

#[test]
fn resettling_after_a_restart_finishes_an_interrupted_propagation() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    interrupted_after_the_write(&mut store);
    let changed = store.resettle_waiting(1, 3).expect("resettle");
    assert_eq!(
        changed.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
        ["c"]
    );
    assert_eq!(get(&store, "c").state, TaskState::Ready);
    assert!(store.resettle_waiting(1, 4).expect("again").is_empty());
}

#[test]
fn a_reduce_all_record_reads_back_as_its_input_type_and_names_the_attempt() {
    use crate::reducer::{TypedReducerInput, reduce_typed};
    use crate::task::ReducerStrategy;
    use crate::task::contract::{MergeConflict, reduce_all_record_output};
    use crate::task::types::TypedValue;

    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let spec: TaskGraphSpec = serde_json::from_value(json!({
    "contract_version": 2,
    "tasks": [
        {"id": "p", "command": custom(),
         "output_schema": {"type": "object", "fields": {"n": {"type": "int64"}}}},
        {"id": "q", "command": custom(), "output_schema": {"type": "string"}},
        {"id": "all", "command": {"kind": "reduce", "inputs": ["p", "q"],
                                  "strategy": {"kind": "all"}}}
    ]}))
    .expect("spec");
    store.submit_graph(1, spec, 0).expect("submit");
    let big = 9_007_199_254_740_993_i64;
    let a = start(&mut store, "p", 1);
    store
        .complete(1, &"p".into(), Completion::succeeded(Some(a), ok(big)), 2)
        .unwrap();
    start(&mut store, "q", 1);
    store
        .complete(1, &"q".into(), Completion::failed(None, "no".into()), 2)
        .unwrap();

    let inputs: Vec<TypedReducerInput> = ["p", "q"]
        .iter()
        .map(|id| TypedReducerInput::from_task(&get(&store, id)))
        .collect();
    let value = reduce_typed(
        &ReducerStrategy::All,
        &inputs,
        MergeConflict::Error,
        |_, _| unreachable!(),
    )
    .unwrap();
    assert_eq!(value[0]["attempt_id"], json!("p#1"));
    let attempt = start(&mut store, "all", 3);
    let result = TaskResult {
        exit_code: None,
        output: Some(value),
        error: None,
    };
    store
        .complete(
            1,
            &"all".into(),
            Completion::succeeded(Some(attempt), result),
            4,
        )
        .expect("all");

    let reducer = get(&store, "all");
    let p = reduce_all_record_output(&reducer, &get(&store, "p")).expect("p");
    assert_eq!(
        p,
        Some(TypedValue::Object(
            [("n".to_string(), TypedValue::Int64(big))].into()
        ))
    );
    assert_eq!(
        reduce_all_record_output(&reducer, &get(&store, "q")).expect("q"),
        None
    );
    let not_all = reduce_all_record_output(&get(&store, "p"), &get(&store, "q"));
    assert!(not_all.is_err());
}

// 결과를 회수할 수 없는 회차는 Unknown 으로 남는다. 하류는 실패 전파 없이 기다리고, 늦은
// 보고는 받지 않으며, retry 가 새 회차를 연다.
#[test]
fn a_lost_result_leaves_the_task_unknown_until_a_retry() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    submit_pair(&mut store);
    let attempt = start(&mut store, "p", 1);
    let p = "p".to_string();
    let receipt = store
        .complete(
            1,
            &p,
            Completion::lost(Some(attempt.clone()), "run result lost: pid 7".into()),
            2,
        )
        .expect("lost");
    assert!(receipt.transitioned.is_empty());
    let task = get(&store, "p");
    assert_eq!(
        task.state,
        TaskState::Unknown {
            reason: Some("run result lost: pid 7".into())
        }
    );
    assert!(task.typed_result.is_none(), "no result is made up");
    assert_eq!(task.finished_at, None, "unknown is not a terminal state");
    assert!(matches!(get(&store, "c").state, TaskState::Waiting));

    // 같은 회차의 늦은 보고는 Unknown 을 끝내지 않는다.
    assert!(
        store
            .complete(1, &p, Completion::succeeded(Some(attempt), ok(1)), 3)
            .is_err()
    );
    assert!(matches!(get(&store, "p").state, TaskState::Unknown { .. }));

    store.retry(1, &p, false, 4).expect("retry from unknown");
    assert_eq!(start(&mut store, "p", 5), "p#2");
    store
        .complete(1, &p, Completion::succeeded(Some("p#2".into()), ok(3)), 6)
        .expect("second attempt");
    assert!(matches!(get(&store, "c").state, TaskState::Ready));
}

#[test]
fn a_lost_v1_result_leaves_the_task_unknown_and_its_dependents_waiting() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let opts = |name: &str, depends_on: Vec<TaskId>| TaskCreateOpts {
        workspace_id: 1,
        name: name.into(),
        command: TaskCommand::WaitBarrier { name: "b".into() },
        depends_on,
        on_failure: OnFailure::default(),
        metadata: Value::Null,
        now_ms: 0,
    };
    let a = store.create(opts("a", vec![])).expect("a");
    let b = store.create(opts("b", vec![a.id.clone()])).expect("b");
    store
        .set_state(1, &a.id, TaskState::Running, 1)
        .expect("running");
    store
        .complete(1, &a.id, Completion::lost(None, "gone".into()), 2)
        .expect("lost");
    assert!(matches!(
        get(&store, &a.id).state,
        TaskState::Unknown { .. }
    ));
    assert!(matches!(get(&store, &b.id).state, TaskState::Waiting));
}

// 이전 레코드의 unknown(이유 없음)도 그대로 읽힌다.
#[test]
fn an_unknown_state_without_a_reason_still_reads() {
    let state: TaskState = serde_json::from_value(json!({"kind": "unknown"})).expect("old form");
    assert_eq!(state, TaskState::Unknown { reason: None });
    assert_eq!(
        serde_json::to_value(&state).unwrap(),
        json!({"kind": "unknown"})
    );
}
