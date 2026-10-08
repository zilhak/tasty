use serde_json::{Value, json};

use super::*;
use crate::task::contract::{TaskContract, check_task};
use crate::task::{OnFailure, TaskCommand, TaskState};

fn spec(v: Value) -> PostprocessSpec {
    serde_json::from_value(v).expect("postprocess spec")
}

fn json_out() -> StdoutSpec {
    StdoutSpec::default()
}

fn json_at(p: &str) -> StdoutSpec {
    StdoutSpec {
        format: StdoutFormat::Json,
        pointer: Some(p.into()),
    }
}

fn cause_of(r: Result<Value, (PostprocessCause, String)>) -> PostprocessCause {
    r.expect_err("expected a failure").0
}

fn task(command: TaskCommand, contract: Value) -> Task {
    Task {
        id: "t".into(),
        workspace_id: 1,
        name: "t".into(),
        command,
        depends_on: Vec::new(),
        state: TaskState::Waiting,
        created_at: 0,
        started_at: None,
        finished_at: None,
        result: None,
        on_failure: OnFailure::Abort,
        metadata: Value::Null,
        reserved_for_fallback: false,
        contract: Some(serde_json::from_value::<TaskContract>(contract).expect("contract")),
        typed_result: None,
        graph_id: None,
        input_snapshot: None,
        accepted: None,
        attempt: None,
        route: None,
        skip: None,
    }
}

fn run() -> TaskCommand {
    TaskCommand::Run {
        command: vec!["true".into()],
        workspace_id: 1,
        cwd: None,
    }
}

fn checked(t: &Task) -> Result<(), crate::task::contract::TaskFailure> {
    check_task(t, "", |_| None)
}

#[test]
fn the_declaration_rejects_unknown_keys_and_needs_a_finite_timeout() {
    assert!(
        serde_json::from_value::<PostprocessSpec>(
            json!({"command": ["x"], "timeout_ms": 1, "shell": true})
        )
        .is_err()
    );
    assert!(serde_json::from_value::<PostprocessSpec>(json!({"command": ["x"]})).is_err());
    let ok = spec(json!({"command": ["x"], "timeout_ms": 1000}));
    assert_eq!(ok.stdout.format, StdoutFormat::Json);
    assert_eq!(ok.max_runs(), 1);
    assert!(check_spec(&ok).is_ok());

    for (bad, loc) in [
        (
            json!({"command": [], "timeout_ms": 1000}),
            "/postprocess/command",
        ),
        (
            json!({"command": [""], "timeout_ms": 1000}),
            "/postprocess/command",
        ),
        (
            json!({"command": ["x"], "timeout_ms": 0}),
            "/postprocess/timeout_ms",
        ),
        (
            json!({"command": ["x"], "timeout_ms": MAX_POSTPROCESS_TIMEOUT_MS + 1}),
            "/postprocess/timeout_ms",
        ),
        (
            json!({"command": ["x"], "timeout_ms": 1, "retry": {"max_retries": 0}}),
            "/postprocess/retry/max_retries",
        ),
        (
            json!({"command": ["x"], "timeout_ms": 1, "retry": {"max_retries": MAX_POSTPROCESS_RETRIES + 1}}),
            "/postprocess/retry/max_retries",
        ),
        (
            json!({"command": ["x"], "timeout_ms": 1, "retry": {"max_retries": 1, "delay_ms": MAX_POSTPROCESS_RETRY_DELAY_MS + 1}}),
            "/postprocess/retry/delay_ms",
        ),
        (
            json!({"command": ["x"], "timeout_ms": 1, "stdout": {"format": "text", "pointer": "/a"}}),
            "/postprocess/stdout/pointer",
        ),
        (
            json!({"command": ["x"], "timeout_ms": 1, "stdout": {"pointer": "a"}}),
            "/postprocess/stdout/pointer",
        ),
        (
            json!({"command": ["x"], "timeout_ms": 1, "stdin": {"v": {"from": "input", "pointer": "x"}}}),
            "/postprocess/stdin/v/pointer",
        ),
    ] {
        let f = check_spec(&spec(bad.clone())).expect_err(&bad.to_string());
        assert_eq!(f.location.as_deref(), Some(loc), "{bad}");
    }
    let retried = spec(
        json!({"command": ["x"], "timeout_ms": 1, "retry": {"max_retries": 2, "delay_ms": 5}}),
    );
    assert_eq!(retried.max_runs(), 3);
    assert_eq!(retried.retry_delay_ms(), 5);
}

#[test]
fn json_stdout_keeps_false_zero_null_and_enum_values_as_values() {
    for (text, want) in [
        ("\"pass\"\n", json!("pass")),
        ("false", json!(false)),
        ("0", json!(0)),
        ("null", Value::Null),
        (
            "  {\"verdict\": \"revise\", \"n\": 0}  \n",
            json!({"verdict": "revise", "n": 0}),
        ),
        ("[1, 2]", json!([1, 2])),
    ] {
        assert_eq!(
            collect_stdout(&json_out(), text.as_bytes(), false).unwrap(),
            want,
            "{text}"
        );
    }
    // pointer 는 있는 false·0·null 을 값으로 고르고, 없는 위치만 실패한다. 수집 값은 stdout 전체다.
    let doc = br#"{"ok": false, "n": 0, "v": null}"#;
    let whole = json!({"ok": false, "n": 0, "v": null});
    for (p, want) in [("/ok", json!(false)), ("/n", json!(0)), ("/v", Value::Null)] {
        let got = collect_stdout(&json_at(p), doc, false).unwrap();
        assert_eq!(got, whole);
        assert_eq!(output_candidate(&json_at(p), &got), Some(want), "{p}");
    }
    assert_eq!(output_candidate(&json_out(), &whole), Some(whole.clone()));
    assert_eq!(
        cause_of(collect_stdout(&json_at("/missing"), doc, false)),
        PostprocessCause::PointerMissing
    );
}

#[test]
fn json_stdout_failures_never_fall_back_to_text() {
    let j = json_out();
    assert_eq!(
        cause_of(collect_stdout(&j, b"", false)),
        PostprocessCause::EmptyOutput
    );
    assert_eq!(
        cause_of(collect_stdout(&j, b" \n\t", false)),
        PostprocessCause::EmptyOutput
    );
    assert_eq!(
        cause_of(collect_stdout(&j, b"pass", false)),
        PostprocessCause::InvalidJson
    );
    assert_eq!(
        cause_of(collect_stdout(&j, b"{\"a\": 1", false)),
        PostprocessCause::InvalidJson
    );
    assert_eq!(
        cause_of(collect_stdout(&j, b"1 2", false)),
        PostprocessCause::MultipleDocuments
    );
    assert_eq!(
        cause_of(collect_stdout(&j, b"{} {}", false)),
        PostprocessCause::MultipleDocuments
    );
    assert_eq!(
        cause_of(collect_stdout(&j, b"{} x", false)),
        PostprocessCause::InvalidJson
    );
    assert_eq!(
        cause_of(collect_stdout(&j, b"\"\xff\"", false)),
        PostprocessCause::InvalidUtf8
    );
    // 상한을 넘긴 수집은 앞부분이 온전한 JSON 이어도 실패한다.
    assert_eq!(
        cause_of(collect_stdout(&j, b"{}", true)),
        PostprocessCause::StdoutTooLarge
    );
}

#[test]
fn text_stdout_is_the_utf8_string_including_empty() {
    let t = StdoutSpec {
        format: StdoutFormat::Text,
        pointer: None,
    };
    assert_eq!(collect_stdout(&t, b"", false).unwrap(), json!(""));
    assert_eq!(
        collect_stdout(&t, b"not json\n", false).unwrap(),
        json!("not json\n")
    );
    assert_eq!(
        cause_of(collect_stdout(&t, b"\xc3\x28", false)),
        PostprocessCause::InvalidUtf8
    );
    assert_eq!(
        cause_of(collect_stdout(&t, b"x", true)),
        PostprocessCause::StdoutTooLarge
    );
}

#[test]
fn the_stdin_document_holds_only_the_mapped_fields() {
    let t = task(run(), json!({"contract_version": 2}));
    let execution = crate::task::TaskResult {
        exit_code: Some(0),
        output: Some(json!({"stdout": {"text": "hello"}})),
        error: None,
    };
    let s = spec(json!({
        "command": ["x"], "timeout_ms": 1,
        "stdin": {
            "all_raw": {"from": "raw"},
            "text": {"from": "raw", "pointer": "/execution/stdout/text"},
            "code": {"from": "raw", "pointer": "/exit_code"},
            "input": {"from": "input"},
            "artifacts": {"from": "artifacts"}
        }
    }));
    let doc = stdin_document(&t, &s, &execution).unwrap();
    assert_eq!(
        doc,
        json!({
            "all_raw": {"exit_code": 0, "execution": {"stdout": {"text": "hello"}}},
            "text": "hello",
            "code": 0,
            "input": null,
            "artifacts": []
        })
    );
    let empty = spec(json!({"command": ["x"], "timeout_ms": 1}));
    assert_eq!(stdin_document(&t, &empty, &execution).unwrap(), json!({}));
    let missing = spec(
        json!({"command": ["x"], "timeout_ms": 1, "stdin": {"v": {"from": "raw", "pointer": "/nope"}}}),
    );
    assert_eq!(
        stdin_document(&t, &missing, &execution).unwrap_err().0,
        PostprocessCause::StdinMapping
    );
    // custom 비동기 task 의 접수 응답도 raw 에서 읽을 수 있다.
    let mut t = t;
    t.accepted = Some(crate::task::contract::AcceptedResponse::capture(
        &json!({"job": "J"}),
    ));
    let job = spec(json!({"command": ["x"], "timeout_ms": 1,
        "stdin": {"job": {"from": "raw", "pointer": "/accepted/response/job"}}}));
    assert_eq!(
        stdin_document(&t, &job, &execution).unwrap(),
        json!({"job": "J"})
    );
}

#[test]
fn a_postprocessed_run_may_declare_a_non_exit_code_output() {
    let plain = task(
        run(),
        json!({"contract_version": 2, "output_schema": {"type": "boolean"}}),
    );
    assert!(checked(&plain).is_err());
    let post = task(
        run(),
        json!({"contract_version": 2, "output_schema": {"type": "boolean"},
               "postprocess": {"command": ["judge"], "timeout_ms": 1000}}),
    );
    assert!(checked(&post).is_ok());
    let custom = task(
        TaskCommand::Custom {
            ipc_method: "system.ping".into(),
            params: Value::Null,
            poll: None,
        },
        json!({"contract_version": 2, "postprocess": {"command": ["judge"], "timeout_ms": 1000}}),
    );
    assert!(checked(&custom).is_ok());
    let barrier = task(
        TaskCommand::WaitBarrier { name: "b".into() },
        json!({"contract_version": 2, "postprocess": {"command": ["judge"], "timeout_ms": 1000}}),
    );
    assert!(checked(&barrier).is_err());
    let bad = task(
        run(),
        json!({"contract_version": 2, "postprocess": {"command": ["judge"], "timeout_ms": 0}}),
    );
    assert_eq!(
        checked(&bad).unwrap_err().location.as_deref(),
        Some("/postprocess/timeout_ms")
    );
}

#[test]
fn only_cancellation_unknown_outcomes_and_stdin_mapping_skip_retries() {
    for c in [
        PostprocessCause::StdinMapping,
        PostprocessCause::Cancelled,
        PostprocessCause::OutcomeUnknown,
    ] {
        assert!(!c.retryable(), "{}", c.name());
    }
    for c in [
        PostprocessCause::Spawn,
        PostprocessCause::NonzeroExit,
        PostprocessCause::Timeout,
        PostprocessCause::InvalidJson,
        PostprocessCause::StdoutTooLarge,
    ] {
        assert!(c.retryable(), "{}", c.name());
    }
}
