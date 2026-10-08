//! 기록하지 못할 만큼 큰 후처리 실행 보고 — 같은 실행의 `result_too_large` 실패로 줄여 회차를
//! 확정한다. 줄이지 않으면 같은 보고를 계속 다시 내며 task 가 Running 에 머문다.

use std::sync::atomic::AtomicU64;

use serde_json::{Value, json};
use tasty_memory::MemoryStore;

use super::postprocess::{PostprocessCause, PostprocessOutcome, PostprocessReport};
use super::*;
use crate::runner::{completion_retryable, shrink_too_large_completion};

const KIB: usize = 1024;

/// 정의를 `pad` 바이트만큼 키운 judge(후처리 string 출력, pointer `/v`).
fn submit(store: &mut TaskStore, pad: usize) {
    let spec: TaskGraphSpec = serde_json::from_value(json!({
    "contract_version": 2,
    "tasks": [
        {"id": "judge",
         "command": {"kind": "custom", "ipc_method": "system.ping", "params": {"pad": "d".repeat(pad)}},
         "output_schema": {"type": "string"},
         "postprocess": {"command": ["judge"], "timeout_ms": 1000,
                         "stdout": {"format": "json", "pointer": "/v"}}}
    ]}))
    .expect("spec");
    store.submit_graph(1, spec, 0).expect("submit");
}

fn judge(store: &TaskStore) -> Task {
    store.get(1, &"judge".to_string()).unwrap().expect("task")
}

fn record_bytes(task: &Task) -> usize {
    serde_json::to_vec(task).expect("json").len()
}

/// 본 작업이 `main` 바이트 문자열로 성공하고 첫 후처리 실행을 시작한 상태. 회차 id 를 돌려준다.
fn into_postprocess(store: &mut TaskStore, main: usize) -> String {
    let id = "judge".to_string();
    store
        .set_state(1, &id, TaskState::Running, 10)
        .expect("running");
    let attempt = judge(store).attempt.expect("attempt").id;
    let result = TaskResult {
        exit_code: None,
        output: Some(json!("m".repeat(main))),
        error: None,
    };
    store
        .complete(
            1,
            &id,
            Completion::succeeded(Some(attempt.clone()), result),
            11,
        )
        .expect("main completion");
    store
        .begin_postprocess_run(1, &id, &attempt, 1, 12)
        .expect("begin");
    attempt
}

fn collected(stdout: Value) -> PostprocessReport {
    PostprocessReport {
        run: 1,
        exit_code: Some(0),
        stderr: Some("e".repeat(16 * KIB)),
        stderr_truncated: true,
        outcome: PostprocessOutcome::Collected { stdout },
    }
}

/// 기록을 시도하고, 너무 크면 러너처럼 줄여 다시 기록한다. 줄인 보고를 돌려준다.
fn record_shrinking(store: &mut TaskStore, completion: Completion) -> Completion {
    let id = "judge".to_string();
    let e = store
        .complete(1, &id, completion.clone(), 20)
        .expect_err("the report is too large to record");
    assert!(completion_retryable(&e), "{e}");
    let shrunk = shrink_too_large_completion(&e, &completion).expect("shrinkable");
    store
        .complete(1, &id, shrunk.clone(), 21)
        .expect("the shrunk report is recorded");
    shrunk
}

fn assert_failed_as_too_large(task: &Task) {
    assert!(
        matches!(task.state, TaskState::Failed { .. }),
        "{:?}",
        task.state
    );
    let typed = task.typed_result.as_ref().expect("typed result");
    let error = typed.error.as_ref().expect("error");
    assert_eq!(error.stage, contract::FailureStage::Postprocess);
    assert!(
        error.message.starts_with(
            "postprocess result_too_large: the postprocess result could not be stored: "
        ),
        "{}",
        error.message
    );
    let pp = typed.raw.postprocess.as_ref().expect("raw postprocess");
    assert_eq!(pp.cause, Some(PostprocessCause::ResultTooLarge));
    assert_eq!(pp.run, 1);
    assert_eq!(pp.exit_code, Some(0));
    assert_eq!(pp.stderr, None);
    assert!(record_bytes(task) < tasty_memory::MAX_VALUE_BYTES);
}

/// 출력이 레코드에 세 번 들어가 기록하지 못하는 성공 보고.
#[test]
fn a_collected_report_too_large_to_record_settles_as_result_too_large() {
    let td = tempfile::tempdir().expect("tempdir");
    let mut mem = MemoryStore::open(&td.path().join("mem.db")).expect("mem");
    let seq = AtomicU64::new(0);
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    submit(&mut store, 0);
    let attempt = into_postprocess(&mut store, 250 * KIB);
    let report = collected(json!({"v": "o".repeat(250 * KIB)}));
    let shrunk = record_shrinking(&mut store, Completion::postprocessed(Some(attempt), report));
    assert_failed_as_too_large(&judge(&store));
    // 줄인 보고는 다시 줄이지 않는다(호출자의 재기록 재귀가 한 번에 끝난다).
    let e =
        crate::AgentError::Memory(tasty_memory::MemoryError::ValueTooLarge { actual: 1, max: 0 });
    assert!(shrink_too_large_completion(&e, &shrunk).is_none());
}

/// 정의가 상한 가까이 크면 본 작업 결과 사본 둘(회차 진행과 raw)만으로 넘친다. 짧은 실패도
/// 본 작업 결과 사본을 비워야 들어간다.
#[test]
fn the_shrunk_report_drops_the_main_result_copy_to_fit_beside_a_large_definition() {
    let td = tempfile::tempdir().expect("tempdir");
    let mut mem = MemoryStore::open(&td.path().join("mem.db")).expect("mem");
    let seq = AtomicU64::new(0);
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    submit(&mut store, 760 * KIB);
    let attempt = into_postprocess(&mut store, 240 * KIB);
    let report = collected(json!({"v": "ok"}));
    record_shrinking(&mut store, Completion::postprocessed(Some(attempt), report));
    let task = judge(&store);
    assert_failed_as_too_large(&task);
    let typed = task.typed_result.as_ref().expect("typed result");
    assert_eq!(typed.raw.execution, None);
    let progress = task
        .attempt
        .as_ref()
        .and_then(|a| a.postprocess.as_ref())
        .expect("progress");
    assert_eq!(progress.execution.output, None);
}

/// 재시도가 남은 실패 보고도 기록하지 못하면 재시도 없이 회차를 확정한다.
#[test]
fn a_too_large_report_is_not_retried_even_with_runs_left() {
    let spec: postprocess::PostprocessSpec = serde_json::from_value(json!({
        "command": ["judge"], "timeout_ms": 1000, "retry": {"max_retries": 3, "delay_ms": 0}
    }))
    .expect("spec");
    let report = PostprocessReport::failed(1, PostprocessCause::ResultTooLarge, "x");
    assert_eq!(postprocess::next_run(&spec, &report), None);
}
