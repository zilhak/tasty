//! agent task 계약: 생성 검사, 기본 출력, 결과 확정, 세션 연결 기록.

use std::sync::atomic::AtomicU64;

use serde_json::{Value, json};
use tasty_memory::MemoryStore;
use tempfile::TempDir;

use super::agent::{self, AgentLink, AgentSession, report};
use super::contract::{FailureCode, FailureStage, TaskContract};
use super::*;
use crate::AgentError;

fn fresh_store() -> (TempDir, MemoryStore, AtomicU64) {
    let td = tempfile::tempdir().expect("tempdir");
    let mem = MemoryStore::open(&td.path().join("mem.db")).expect("mem");
    (td, mem, AtomicU64::new(0))
}

fn agent_cmd(provider: &str) -> TaskCommand {
    TaskCommand::Agent {
        provider: provider.into(),
        workspace_id: 1,
        session: AgentSession::Existing { surface_id: 7 },
        instruction: "review the diff".into(),
        timeout_ms: None,
    }
}

fn opts(command: TaskCommand) -> TaskCreateOpts {
    TaskCreateOpts {
        workspace_id: 1,
        name: "review".into(),
        command,
        depends_on: vec![],
        on_failure: OnFailure::Abort,
        metadata: Value::Null,
        now_ms: 0,
    }
}

fn contract(v: Value) -> TaskContract {
    serde_json::from_value(v).expect("contract")
}

fn v2() -> TaskContract {
    contract(json!({ "contract_version": 2 }))
}

fn review_contract() -> TaskContract {
    contract(json!({
        "contract_version": 2,
        "output_schema": { "type": "enum", "values": ["approve", "revise"] }
    }))
}

fn input_failure(e: AgentError) -> contract::TaskFailure {
    match e {
        AgentError::TypeContract(f) => *f,
        other => panic!("expected a contract failure, got {other:?}"),
    }
}

/// Running 으로 만들고 회차 id 를 돌려준다.
fn start(store: &mut TaskStore, id: &TaskId) -> String {
    store
        .set_state(1, id, TaskState::Running, 1)
        .expect("running");
    store
        .get(1, id)
        .unwrap()
        .unwrap()
        .attempt
        .expect("attempt")
        .id
}

fn done(output: Value) -> Completion {
    Completion {
        attempt_id: None,
        result: TaskResult {
            exit_code: None,
            output: Some(output),
            error: None,
        },
        outcome: CompletionOutcome::Succeeded,
        postprocess: None,
    }
}

fn failed(error: String) -> Completion {
    Completion {
        attempt_id: None,
        result: TaskResult {
            exit_code: None,
            output: None,
            error: Some(error.clone()),
        },
        outcome: CompletionOutcome::Failed { error },
        postprocess: None,
    }
}

#[test]
fn agent_tasks_need_a_v2_contract_and_a_supported_provider() {
    let (_td, mut mem, seq) = fresh_store();
    let mut store = TaskStore::new(&mut mem, "host", &seq);
    let e = store
        .create(opts(agent_cmd("claude")))
        .expect_err("v1 agent");
    assert!(
        matches!(&e, AgentError::InvalidArgument(m) if m == agent::AGENT_NEEDS_CONTRACT),
        "{e}"
    );
    let f = input_failure(
        store
            .create_typed(opts(agent_cmd("gemini")), v2())
            .expect_err("unknown provider"),
    );
    assert!(
        f.message.contains("does not support result collection"),
        "{f:?}"
    );
    let t = store
        .create_typed(opts(agent_cmd("codex")), v2())
        .expect("codex agent");
    assert_eq!(
        t.contract.as_ref().unwrap().output_schema(&t.command),
        super::types::TypeSchema::string(),
        "기본 출력은 최종 답변 string"
    );
}

#[test]
fn agent_input_reaches_the_session_only_through_the_input_block() {
    let (_td, mut mem, seq) = fresh_store();
    let mut store = TaskStore::new(&mut mem, "host", &seq);
    let typed_input = json!({
        "contract_version": 2,
        "input_schema": { "type": "object", "fields": { "diff": { "type": "string" } } },
        "bindings": { "diff": { "literal": "x" } }
    });
    let f = input_failure(
        store
            .create_typed(opts(agent_cmd("claude")), contract(typed_input.clone()))
            .expect_err("unmapped input"),
    );
    assert!(f.message.contains("input_block"), "{f:?}");
    let mut with_block = typed_input;
    with_block["input_mapping"] = json!({ "input_block": true });
    store
        .create_typed(opts(agent_cmd("claude")), contract(with_block))
        .expect("input block");
    let run = TaskCommand::Run {
        command: vec!["true".into()],
        workspace_id: 1,
        cwd: None,
    };
    let f = input_failure(
        store
            .create_typed(
                opts(run),
                contract(json!({
                    "contract_version": 2,
                    "input_mapping": { "input_block": true }
                })),
            )
            .expect_err("input_block on run"),
    );
    assert!(f.message.contains("applies to agent"), "{f:?}");
}

#[test]
fn the_composed_instruction_keeps_the_input_as_a_json_block() {
    let text = agent::compose_instruction("do it", Some(&json!({ "n": "1" })));
    assert!(text.starts_with("do it\n\n"));
    assert!(text.contains(agent::INPUT_BLOCK_HEADER));
    assert!(text.contains("\"n\": \"1\""));
    assert_eq!(agent::compose_instruction("do it", None), "do it");
}

#[test]
fn the_final_answer_is_the_default_output() {
    let (_td, mut mem, seq) = fresh_store();
    let mut store = TaskStore::new(&mut mem, "host", &seq);
    let t = store.create_typed(opts(agent_cmd("claude")), v2()).unwrap();
    start(&mut store, &t.id);
    let receipt = store
        .complete(
            1,
            &t.id,
            done(json!({ report::FINAL_ANSWER: "looks good", report::PROVIDER: "claude" })),
            2,
        )
        .expect("complete");
    assert_eq!(receipt.task.state, TaskState::Succeeded);
    let typed = receipt.task.typed_result.expect("typed");
    assert_eq!(typed.output.to_wire(), json!("looks good"));
    assert_eq!(typed.provenance.output_source, "agent.final_answer");
    assert_eq!(
        typed.raw.execution.expect("raw")[report::FINAL_ANSWER],
        "looks good",
        "원본 보고를 raw 에 보존한다"
    );
}

#[test]
fn a_submitted_revise_is_a_successful_business_result() {
    let (_td, mut mem, seq) = fresh_store();
    let mut store = TaskStore::new(&mut mem, "host", &seq);
    let t = store
        .create_typed(opts(agent_cmd("claude")), review_contract())
        .unwrap();
    assert!(agent::needs_submission(
        t.contract.as_ref().unwrap(),
        &t.command
    ));
    start(&mut store, &t.id);
    let receipt = store
        .complete(
            1,
            &t.id,
            done(json!({ report::FINAL_ANSWER: "please fix", report::SUBMITTED: "revise" })),
            2,
        )
        .expect("complete");
    assert_eq!(receipt.task.state, TaskState::Succeeded);
    let typed = receipt.task.typed_result.unwrap();
    assert_eq!(typed.output.to_wire(), json!("revise"));
    assert_eq!(typed.provenance.output_source, "agent.submitted");
}

#[test]
fn missing_result_exit_and_turn_error_stay_distinguishable() {
    let cases = [
        (FailureCode::ResultMissing, FailureStage::OutputValidation),
        (FailureCode::AgentExited, FailureStage::Execution),
        (FailureCode::AgentTurnError, FailureStage::Execution),
        (FailureCode::TimedOut, FailureStage::Execution),
    ];
    for (code, stage) in cases {
        let (_td, mut mem, seq) = fresh_store();
        let mut store = TaskStore::new(&mut mem, "host", &seq);
        let t = store
            .create_typed(opts(agent_cmd("claude")), review_contract())
            .unwrap();
        start(&mut store, &t.id);
        let receipt = store
            .complete(1, &t.id, failed(code.message("detail")), 2)
            .expect("complete");
        assert!(matches!(receipt.task.state, TaskState::Failed { .. }));
        let err = receipt.task.typed_result.unwrap().error.expect("error");
        assert_eq!((err.code, err.stage), (Some(code), stage), "{err:?}");
    }
}

#[test]
fn a_report_without_any_answer_is_result_missing() {
    let (_td, mut mem, seq) = fresh_store();
    let mut store = TaskStore::new(&mut mem, "host", &seq);
    let t = store.create_typed(opts(agent_cmd("claude")), v2()).unwrap();
    start(&mut store, &t.id);
    let receipt = store
        .complete(1, &t.id, done(json!({ report::FINAL_ANSWER: null })), 2)
        .expect("complete");
    let err = receipt.task.typed_result.unwrap().error.expect("error");
    assert_eq!(err.code, Some(FailureCode::ResultMissing));
}

#[test]
fn the_session_link_belongs_to_the_running_attempt_only() {
    let (_td, mut mem, seq) = fresh_store();
    let mut store = TaskStore::new(&mut mem, "host", &seq);
    let t = store.create_typed(opts(agent_cmd("claude")), v2()).unwrap();
    let link = AgentLink {
        provider: "claude".into(),
        surface_id: 7,
        awaiting_input_since: Some(5),
    };
    let attempt = start(&mut store, &t.id);
    assert!(
        !store
            .set_agent_link(1, &t.id, "other#9", link.clone())
            .unwrap(),
        "다른 회차에는 쓰지 않는다"
    );
    assert!(
        store
            .set_agent_link(1, &t.id, &attempt, link.clone())
            .unwrap()
    );
    let got = store.get(1, &t.id).unwrap().unwrap();
    assert_eq!(got.attempt.as_ref().unwrap().agent, Some(link));
    assert_eq!(agent::phase(&got), Some("awaiting_input"));
    store
        .set_state(1, &t.id, TaskState::Cancelled, 3)
        .expect("cancel");
    let got = store.get(1, &t.id).unwrap().unwrap();
    assert_eq!(agent::phase(&got), None);
    assert!(
        !store
            .set_agent_link(
                1,
                &t.id,
                &attempt,
                AgentLink {
                    awaiting_input_since: None,
                    ..got.attempt.unwrap().agent.unwrap()
                }
            )
            .unwrap(),
        "끝난 task 에는 쓰지 않는다"
    );
}

#[test]
fn failure_codes_round_trip_through_the_message() {
    for c in FailureCode::ALL {
        assert_eq!(FailureCode::parse_message(&c.message("x: y")), Some(c));
    }
    assert_eq!(FailureCode::parse_message("plain error"), None);
    assert_eq!(FailureCode::parse_message("other_code: x"), None);
}
