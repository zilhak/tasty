//! 후처리 단계의 저장소 경계 — 본 작업 성공 뒤 Running 유지, 재시도 예약, 마지막 보고에서만 종결.

use std::sync::atomic::AtomicU64;

use serde_json::{Value, json};
use tasty_memory::MemoryStore;
use tempfile::TempDir;

use super::postprocess::{
    PostprocessCause, PostprocessOutcome, PostprocessPhase, PostprocessReport,
};
use super::*;
use crate::{AgentError, CompletionRejection};

fn fresh() -> (TempDir, MemoryStore, AtomicU64) {
    let td = tempfile::tempdir().expect("tempdir");
    let mem = MemoryStore::open(&td.path().join("mem.db")).expect("mem");
    (td, mem, AtomicU64::new(0))
}

/// judge(후처리 boolean 출력) → after(judge 에 depends_on).
fn submit(store: &mut TaskStore, postprocess: Value, output_schema: Value) {
    let spec: TaskGraphSpec = serde_json::from_value(json!({
    "contract_version": 2,
    "tasks": [
        {"id": "judge", "command": {"kind": "custom", "ipc_method": "system.ping", "params": {}},
         "output_schema": output_schema, "postprocess": postprocess},
        {"id": "after", "command": {"kind": "custom", "ipc_method": "system.ping", "params": {}},
         "depends_on": ["judge"]}
    ]}))
    .expect("spec");
    store.submit_graph(1, spec, 0).expect("submit");
}

fn get(store: &TaskStore, id: &str) -> Task {
    store.get(1, &id.to_string()).unwrap().expect("task")
}

fn judge() -> TaskId {
    "judge".to_string()
}

fn main_ok() -> TaskResult {
    TaskResult {
        exit_code: Some(0),
        output: Some(json!({"stdout": {"text": "draft"}})),
        error: None,
    }
}

fn collected(run: u32, stdout: Value) -> PostprocessReport {
    PostprocessReport {
        run,
        exit_code: Some(0),
        stderr: Some("thinking...\n".into()),
        stderr_truncated: false,
        outcome: PostprocessOutcome::Collected { stdout },
    }
}

fn failed(run: u32, cause: PostprocessCause) -> PostprocessReport {
    let mut r = PostprocessReport::failed(run, cause, "boom");
    r.exit_code = (cause == PostprocessCause::NonzeroExit).then_some(3);
    r
}

fn phase(t: &Task) -> PostprocessPhase {
    t.attempt
        .as_ref()
        .and_then(|a| a.postprocess.as_ref())
        .expect("postprocess progress")
        .phase
}

fn rejection(e: AgentError) -> CompletionRejection {
    match e {
        AgentError::CompletionRejected { reason, .. } => reason,
        other => panic!("expected a rejected completion, got {other:?}"),
    }
}

/// judge 를 Running 으로 만들고 본 작업 성공을 보고한다.
fn main_done(store: &mut TaskStore, now: u64) -> String {
    store
        .set_state(1, &judge(), TaskState::Running, now)
        .expect("running");
    let attempt = get(store, "judge").attempt.expect("attempt").id;
    let receipt = store
        .complete(
            1,
            &judge(),
            Completion::succeeded(Some(attempt.clone()), main_ok()),
            now + 1,
        )
        .expect("main completion");
    assert!(matches!(receipt.task.state, TaskState::Running));
    assert!(receipt.transitioned.is_empty());
    attempt
}

fn report(
    store: &mut TaskStore,
    attempt: &str,
    r: PostprocessReport,
    now: u64,
) -> crate::Result<CompletionReceipt> {
    store.complete(
        1,
        &judge(),
        Completion::postprocessed(Some(attempt.to_string()), r),
        now,
    )
}

fn submit_plain(store: &mut TaskStore) {
    submit(
        store,
        json!({"command": ["judge"], "timeout_ms": 1000}),
        json!({"type": "boolean"}),
    );
}

#[test]
fn main_success_keeps_the_task_running_until_the_postprocess_reports() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    submit_plain(&mut store);
    main_done(&mut store, 10);
    let t = get(&store, "judge");
    assert!(matches!(t.state, TaskState::Running));
    assert!(t.typed_result.is_none());
    assert_eq!(
        phase(&t),
        PostprocessPhase::Pending {
            run: 1,
            not_before_ms: 11
        }
    );
    let progress = t.attempt.as_ref().unwrap().postprocess.clone().unwrap();
    assert_eq!(progress.phase_name(), "postprocessing");
    assert_eq!(progress.execution, main_ok());
    // 다음 task 는 후처리가 끝나기 전에 실행 가능 상태가 되지 않는다.
    assert!(matches!(get(&store, "after").state, TaskState::Waiting));
}

#[test]
fn main_and_postprocess_reports_are_checked_against_the_progress() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    submit_plain(&mut store);
    let attempt = main_done(&mut store, 10);
    // 같은 본 작업 보고의 재전송은 중복, 다른 보고는 충돌이다.
    let again = store
        .complete(
            1,
            &judge(),
            Completion::succeeded(Some(attempt.clone()), main_ok()),
            12,
        )
        .expect("duplicate main");
    assert!(again.duplicate);
    let mut other = main_ok();
    other.exit_code = Some(1);
    let e = store
        .complete(
            1,
            &judge(),
            Completion::succeeded(Some(attempt.clone()), other),
            12,
        )
        .unwrap_err();
    assert_eq!(rejection(e), CompletionRejection::DifferentReport);
    // 예약되지 않은 실행의 보고는 받지 않는다.
    let e = report(&mut store, &attempt, collected(2, json!(true)), 13).unwrap_err();
    assert_eq!(rejection(e), CompletionRejection::StaleAttempt);
    // 같은 실행을 두 번 시작하지 않는다.
    let (_, spec, execution) = store
        .begin_postprocess_run(1, &judge(), &attempt, 1, 14)
        .expect("begin");
    assert_eq!(spec.command, vec!["judge".to_string()]);
    assert_eq!(execution, main_ok());
    assert_eq!(
        phase(&get(&store, "judge")),
        PostprocessPhase::Started {
            run: 1,
            started_at: 14
        }
    );
    assert!(
        store
            .begin_postprocess_run(1, &judge(), &attempt, 1, 15)
            .is_err()
    );
}

#[test]
fn the_postprocess_report_finalizes_the_attempt_with_both_raw_results() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    submit_plain(&mut store);
    let attempt = main_done(&mut store, 10);
    store
        .begin_postprocess_run(1, &judge(), &attempt, 1, 14)
        .expect("begin");
    let done = report(&mut store, &attempt, collected(1, json!(false)), 20).expect("report");
    assert!(!done.duplicate);
    let t = get(&store, "judge");
    assert!(matches!(t.state, TaskState::Succeeded));
    assert_eq!(phase(&t), PostprocessPhase::Finished { run: 1 });
    let typed = t.typed_result.expect("typed result");
    // false 는 비즈니스 값이며 실패가 아니다.
    assert!(typed.has_output);
    assert_eq!(serde_json::to_value(&typed.output).unwrap(), json!(false));
    assert_eq!(typed.provenance.output_source, "postprocess.stdout.json");
    assert_eq!(typed.raw.exit_code, Some(0));
    assert_eq!(typed.raw.execution, main_ok().output);
    let pp = typed.raw.postprocess.expect("postprocess raw");
    assert_eq!(pp.command, vec!["judge".to_string()]);
    assert_eq!((pp.run, pp.exit_code), (1, Some(0)));
    assert_eq!(pp.stderr.as_deref(), Some("thinking...\n"));
    assert!(pp.stdout.is_none());
    assert!(matches!(get(&store, "after").state, TaskState::Ready));
}

#[test]
fn a_finished_attempt_answers_repeats_and_rejects_different_reports() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    submit_plain(&mut store);
    let attempt = main_done(&mut store, 10);
    store
        .begin_postprocess_run(1, &judge(), &attempt, 1, 14)
        .expect("begin");
    report(&mut store, &attempt, collected(1, json!(false)), 20).expect("report");
    let again = report(&mut store, &attempt, collected(1, json!(false)), 21).unwrap();
    assert!(again.duplicate);
    let e = report(&mut store, &attempt, collected(1, json!(true)), 21).unwrap_err();
    assert_eq!(rejection(e), CompletionRejection::DifferentReport);
}

#[test]
fn retries_stay_running_and_fail_once_when_the_budget_runs_out() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    submit(
        &mut store,
        json!({"command": ["judge"], "timeout_ms": 1000, "retry": {"max_retries": 2, "delay_ms": 50}}),
        json!({"type": "boolean"}),
    );
    let attempt = main_done(&mut store, 10);
    let mut now = 100;
    for run in 1..=2 {
        store
            .begin_postprocess_run(1, &judge(), &attempt, run, now)
            .expect("begin");
        let r = report(
            &mut store,
            &attempt,
            failed(run, PostprocessCause::NonzeroExit),
            now + 1,
        )
        .expect("report");
        assert!(matches!(r.task.state, TaskState::Running), "run {run}");
        assert!(r.transitioned.is_empty());
        let t = get(&store, "judge");
        assert_eq!(
            phase(&t),
            PostprocessPhase::Pending {
                run: run + 1,
                not_before_ms: now + 1 + 50
            }
        );
        assert_eq!(
            t.attempt
                .as_ref()
                .unwrap()
                .postprocess
                .as_ref()
                .unwrap()
                .phase_name(),
            "retry_wait"
        );
        // 재시도 대기 중 같은 실패 보고는 중복이다.
        assert!(
            report(
                &mut store,
                &attempt,
                failed(run, PostprocessCause::NonzeroExit),
                now + 2
            )
            .unwrap()
            .duplicate
        );
        assert!(matches!(get(&store, "after").state, TaskState::Waiting));
        now += 100;
    }
    store
        .begin_postprocess_run(1, &judge(), &attempt, 3, now)
        .expect("begin 3");
    report(
        &mut store,
        &attempt,
        failed(3, PostprocessCause::Timeout),
        now + 1,
    )
    .expect("final");
    let t = get(&store, "judge");
    assert!(
        matches!(&t.state, TaskState::Failed { error } if error.contains("postprocess timeout"))
    );
    let typed = t.typed_result.expect("typed");
    let err = typed.error.expect("error");
    assert_eq!(err.stage, contract::FailureStage::Postprocess);
    let pp = typed.raw.postprocess.expect("raw");
    assert_eq!(pp.run, 3);
    assert_eq!(pp.cause, Some(PostprocessCause::Timeout));
    assert_eq!(
        pp.failed_runs
            .iter()
            .map(|r| (r.run, r.cause))
            .collect::<Vec<_>>(),
        vec![
            (1, PostprocessCause::NonzeroExit),
            (2, PostprocessCause::NonzeroExit)
        ]
    );
    // 본 작업 결과는 재시도 내내 그대로다.
    assert_eq!(typed.raw.execution, main_ok().output);
    // on_failure(abort)는 마지막 실패에서 한 번 하류를 건너뛴다.
    assert!(matches!(get(&store, "after").state, TaskState::Skipped));
}

#[test]
fn unknown_outcomes_and_validation_failures_are_not_retried() {
    for (r, stage) in [
        (
            failed(1, PostprocessCause::OutcomeUnknown),
            contract::FailureStage::Postprocess,
        ),
        (
            failed(1, PostprocessCause::Cancelled),
            contract::FailureStage::Postprocess,
        ),
        (
            collected(1, json!("pass")),
            contract::FailureStage::OutputValidation,
        ),
    ] {
        let (_td, mut mem, seq) = fresh();
        let mut store = TaskStore::new(&mut mem, "_host", &seq);
        submit(
            &mut store,
            json!({"command": ["judge"], "timeout_ms": 1000, "retry": {"max_retries": 3}}),
            json!({"type": "boolean"}),
        );
        let attempt = main_done(&mut store, 10);
        store
            .begin_postprocess_run(1, &judge(), &attempt, 1, 20)
            .expect("begin");
        report(&mut store, &attempt, r.clone(), 21).expect("report");
        let t = get(&store, "judge");
        assert!(matches!(t.state, TaskState::Failed { .. }), "{r:?}");
        assert_eq!(t.typed_result.unwrap().error.unwrap().stage, stage, "{r:?}");
    }
}

#[test]
fn a_failed_main_execution_skips_the_postprocess() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    submit(
        &mut store,
        json!({"command": ["judge"], "timeout_ms": 1000}),
        json!({"type": "boolean"}),
    );
    store
        .set_state(1, &judge(), TaskState::Running, 1)
        .expect("running");
    let attempt = get(&store, "judge").attempt.unwrap().id;
    store
        .complete(
            1,
            &judge(),
            Completion::failed(Some(attempt), "exit 2".into()),
            2,
        )
        .expect("failed");
    let t = get(&store, "judge");
    assert!(matches!(t.state, TaskState::Failed { .. }));
    assert!(t.attempt.unwrap().postprocess.is_none());
    assert_eq!(
        t.typed_result.unwrap().error.unwrap().stage,
        contract::FailureStage::Execution
    );
}

#[test]
fn the_stdout_pointer_selects_the_output_and_raw_keeps_the_document() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    submit(
        &mut store,
        json!({"command": ["judge"], "timeout_ms": 1000, "stdout": {"pointer": "/verdict"}}),
        json!({"type": "enum", "values": ["pass", "revise"]}),
    );
    let attempt = main_done(&mut store, 10);
    store
        .begin_postprocess_run(1, &judge(), &attempt, 1, 20)
        .expect("begin");
    let doc = json!({"verdict": "revise", "model": "m-1"});
    report(&mut store, &attempt, collected(1, doc.clone()), 21).expect("report");
    let typed = get(&store, "judge").typed_result.expect("typed");
    assert_eq!(
        serde_json::to_value(&typed.output).unwrap(),
        json!("revise")
    );
    assert_eq!(typed.raw.postprocess.unwrap().stdout, Some(doc));
}

#[test]
fn text_output_defaults_to_a_string_and_keeps_the_empty_string() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    submit(
        &mut store,
        json!({"command": ["judge"], "timeout_ms": 1000, "stdout": {"format": "text"}}),
        Value::Null,
    );
    let attempt = main_done(&mut store, 10);
    store
        .begin_postprocess_run(1, &judge(), &attempt, 1, 20)
        .expect("begin");
    report(&mut store, &attempt, collected(1, json!("")), 21).expect("report");
    let t = get(&store, "judge");
    assert!(matches!(t.state, TaskState::Succeeded));
    let typed = t.typed_result.unwrap();
    assert!(typed.has_output);
    assert_eq!(serde_json::to_value(&typed.output).unwrap(), json!(""));
    assert_eq!(typed.provenance.output_source, "postprocess.stdout.text");
}
