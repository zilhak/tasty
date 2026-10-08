use serde_json::{Value, json};

use super::*;
use crate::task::TaskState;
use crate::task::types::{TypeErrorKind, TypedValue};

fn contract(v: Value) -> TaskContract {
    serde_json::from_value(v).expect("contract")
}

fn task(id: &str, command: TaskCommand, contract: Option<TaskContract>) -> Task {
    Task {
        id: id.to_string(),
        workspace_id: 1,
        name: id.to_string(),
        command,
        depends_on: Vec::new(),
        state: TaskState::Succeeded,
        created_at: 0,
        started_at: None,
        finished_at: None,
        result: None,
        on_failure: OnFailure::Abort,
        metadata: Value::Null,
        reserved_for_fallback: false,
        contract,
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

fn custom() -> TaskCommand {
    TaskCommand::Custom {
        ipc_method: "system.ping".into(),
        params: Value::Null,
        poll: None,
    }
}

fn reduce(inputs: &[&str], strategy: ReducerStrategy) -> TaskCommand {
    TaskCommand::Reduce {
        inputs: inputs.iter().map(|s| s.to_string()).collect(),
        strategy,
    }
}

fn v2() -> TaskContract {
    contract(json!({"contract_version": 2}))
}

fn checked(c: &TaskContract, command: &TaskCommand, others: &[Task]) -> Result<(), TaskFailure> {
    let t = task("t", command.clone(), Some(c.clone()));
    check_task(&t, "", |id| others.iter().find(|t| &t.id == id))
}

#[test]
fn contract_json_rejects_unknown_fields_and_other_versions() {
    assert!(
        serde_json::from_value::<TaskContract>(json!({"contract_version": 2, "output": {}}))
            .is_err()
    );
    let v1 = contract(json!({"contract_version": 1}));
    assert!(checked(&v1, &run(), &[]).is_err());
    assert!(checked(&v2(), &run(), &[]).is_ok());
}

#[test]
fn kind_defaults_are_int64_json_unit_and_strategy_specific() {
    assert_eq!(default_output_schema(&run()), TypeSchema::int64());
    assert_eq!(default_output_schema(&custom()), TypeSchema::json());
    assert_eq!(
        default_output_schema(&TaskCommand::WaitBarrier {
            name: Some("b".into())
        }),
        TypeSchema::unit()
    );
    assert_eq!(
        default_output_schema(&reduce(&[], ReducerStrategy::ConcatText)),
        TypeSchema::string()
    );
    assert_eq!(
        default_output_schema(&reduce(&[], ReducerStrategy::All)),
        reduce_all_record_list_schema()
    );
    assert_eq!(v2().input_schema(), TypeSchema::unit());
}

#[test]
fn run_and_barrier_outputs_cannot_be_redeclared() {
    let as_string = contract(json!({"contract_version": 2, "output_schema": {"type": "string"}}));
    assert!(checked(&as_string, &run(), &[]).is_err());
    assert!(
        checked(
            &as_string,
            &TaskCommand::WaitBarrier {
                name: Some("b".into())
            },
            &[]
        )
        .is_err()
    );
    assert!(checked(&as_string, &custom(), &[]).is_ok());
    let exit = contract(json!({"contract_version": 2, "allowed_exit_codes": [0, 7]}));
    assert!(checked(&exit, &run(), &[]).is_ok());
    assert!(checked(&exit, &custom(), &[]).is_err());
}

#[test]
fn inputs_without_bindings_must_be_fully_defaulted() {
    let required = contract(json!({
        "contract_version": 2,
        "input_schema": {"type": "object", "fields": {"requirement": {"type": "string"}}}
    }));
    let e = checked(&required, &custom(), &[]).unwrap_err();
    assert_eq!(e.stage, FailureStage::Input);
    assert_eq!(e.location.as_deref(), Some("/bindings/requirement"));
    assert!(e.message.contains("no binding or default"), "{}", e.message);

    let defaulted = contract(json!({
        "contract_version": 2,
        "input_schema": {"type": "object", "fields": {"n": {"type": "int64", "default": 3}}}
    }));
    assert!(checked(&defaulted, &custom(), &[]).is_ok());
    let scalar = contract(json!({"contract_version": 2, "input_schema": {"type": "int64"}}));
    assert!(checked(&scalar, &custom(), &[]).is_err());
}

#[test]
fn inline_fallback_is_rejected_for_v2() {
    let spec = crate::task::InlineFallbackSpec {
        name: "fb".into(),
        command: run(),
        depends_on_override: None,
        on_failure: OnFailure::Abort,
        metadata: Value::Null,
    };
    let fb = OnFailure::Fallback {
        task: None,
        inline: Some(Box::new(spec)),
    };
    assert!(check_contract(&v2(), &run(), &fb, |_| None).is_err());
}

#[test]
fn first_success_requires_inputs_assignable_to_the_common_type() {
    let int_out = |id: &str| task(id, run(), Some(v2()));
    let legacy = task("legacy", run(), None);
    let as_int = contract(json!({"contract_version": 2, "output_schema": {"type": "int64"}}));
    let cmd = reduce(&["a", "b"], ReducerStrategy::FirstSuccess);
    assert!(checked(&as_int, &cmd, &[int_out("a"), int_out("b")]).is_ok());

    // v1 출력은 json 이라 구체 타입으로 받으려면 projection 이 필요하다.
    let cmd = reduce(&["a", "legacy"], ReducerStrategy::FirstSuccess);
    let e = checked(&as_int, &cmd, &[int_out("a"), legacy.clone()]).unwrap_err();
    assert_eq!(e.task_id.as_deref(), Some("legacy"));
    assert_eq!(e.type_error.unwrap().kind, TypeErrorKind::Incompatible);
    // 공통 타입을 json 으로 두면 받을 수 있다.
    assert!(checked(&v2(), &cmd, &[int_out("a"), legacy]).is_ok());
}

#[test]
fn concat_text_needs_string_inputs_and_all_keeps_its_record_list() {
    let s = contract(json!({"contract_version": 2, "output_schema": {"type": "string"}}));
    let text = task("t", custom(), Some(s.clone()));
    let cmd = reduce(&["t"], ReducerStrategy::ConcatText);
    assert!(checked(&v2(), &cmd, &[text]).is_ok());
    let num = task("n", run(), Some(v2()));
    let cmd = reduce(&["n"], ReducerStrategy::ConcatText);
    assert!(checked(&v2(), &cmd, &[num]).is_err());

    let all = reduce(&[], ReducerStrategy::All);
    assert!(checked(&v2(), &all, &[]).is_ok());
    assert!(checked(&s, &all, &[]).is_err());
}

fn reported(exit_code: Option<i32>, output: Option<Value>, error: Option<&str>) -> TaskResult {
    TaskResult {
        exit_code,
        output,
        error: error.map(str::to_string),
    }
}

#[test]
fn run_output_is_the_exit_code_with_streams_kept_as_raw() {
    let t = task("r", run(), Some(v2()));
    let streams = json!({"pid": 1, "stdout": {"text": "hi\n"}});
    let r = finalize_result(&t, &v2(), &reported(Some(7), Some(streams.clone()), None));
    assert!(r.has_output);
    assert_eq!(r.output, TypedValue::Int64(7));
    assert_eq!(r.raw.exit_code, Some(7));
    assert_eq!(r.raw.execution, Some(streams));
    assert_eq!(r.provenance.output_source, "run.exit_code");

    // Task 밖에서 결과만 직렬화해도 int64 는 10진 문자열이다.
    let alone = serde_json::to_value(&r).unwrap();
    assert_eq!(alone["output"], json!("7"));
    assert_eq!(alone["raw"]["exit_code"], json!(7));

    let no_code = finalize_result(&t, &v2(), &reported(None, None, None));
    assert!(!no_code.has_output);
    assert_eq!(no_code.error.unwrap().stage, FailureStage::Execution);
}

#[test]
fn unit_output_is_a_confirmed_null_not_a_missing_output() {
    let t = task(
        "b",
        TaskCommand::WaitBarrier {
            name: Some("x".into()),
        },
        Some(v2()),
    );
    let r = finalize_result(&t, &v2(), &reported(None, None, None));
    assert!(r.has_output);
    assert_eq!(r.output, TypedValue::Null);
    let text = serde_json::to_value(&r).unwrap();
    // null 출력도 필드로 직렬화돼 부재와 구별된다.
    assert_eq!(text["has_output"], json!(true));
    assert!(text.as_object().unwrap().contains_key("output"));

    let c = task("c", custom(), Some(v2()));
    let none = finalize_result(&c, &v2(), &reported(None, None, None));
    assert!(!none.has_output);
    assert_eq!(none.error.unwrap().stage, FailureStage::OutputValidation);
}

#[test]
fn business_values_like_false_or_revise_are_valid_outputs_and_bad_values_are_type_errors() {
    let verdict = contract(json!({
        "contract_version": 2,
        "output_schema": {"type": "enum", "values": ["pass", "revise", "review"]}
    }));
    let t = task("v", custom(), Some(verdict.clone()));
    let ok = finalize_result(&t, &verdict, &reported(None, Some(json!("revise")), None));
    assert!(ok.has_output && ok.error.is_none());
    assert_eq!(ok.output, TypedValue::String("revise".into()));

    let bad = finalize_result(&t, &verdict, &reported(None, Some(json!("unknown")), None));
    assert!(!bad.has_output);
    let e = bad.error.unwrap();
    assert_eq!(e.stage, FailureStage::OutputValidation);
    assert_eq!(e.task_id.as_deref(), Some("v"));
    let te = e.type_error.unwrap();
    assert_eq!(te.kind, TypeErrorKind::UnknownEnumValue);
    assert_eq!(te.expected, "enum(pass|revise|review)");

    let flag = contract(json!({"contract_version": 2, "output_schema": {"type": "boolean"}}));
    let f = task("f", custom(), Some(flag.clone()));
    let r = finalize_result(&f, &flag, &reported(None, Some(json!(false)), None));
    assert!(r.has_output && r.error.is_none());
}

#[test]
fn int64_submissions_keep_or_reject_without_conversion() {
    let int = contract(json!({"contract_version": 2, "output_schema": {"type": "int64"}}));
    let t = task("i", custom(), Some(int.clone()));
    let kind = |v: Value| {
        finalize_result(&t, &int, &reported(None, Some(v), None))
            .error
            .and_then(|e| e.type_error)
            .map(|e| e.kind)
    };
    assert_eq!(kind(json!(42)), None);
    assert_eq!(kind(json!("42")), None);
    assert_eq!(kind(json!("4 2")), Some(TypeErrorKind::TypeMismatch));
    assert_eq!(kind(json!(1.5)), Some(TypeErrorKind::NotInteger));
    assert_eq!(
        kind(serde_json::from_str("9223372036854775808").unwrap()),
        Some(TypeErrorKind::OutOfRange)
    );
}

#[test]
fn execution_failures_keep_the_reported_error_and_project_to_v1_fields() {
    let t = task("x", run(), Some(v2()));
    let r = finalize_result(&t, &v2(), &reported(None, None, Some("spawn failed")));
    assert!(!r.has_output);
    let e = r.error.clone().unwrap();
    assert_eq!(e.stage, FailureStage::Execution);
    assert_eq!(e.message, "spawn failed");
    let v1 = project_v1(&r);
    assert_eq!(v1.output, None);
    assert_eq!(v1.error.as_deref(), Some("spawn failed"));

    let ok = finalize_result(&t, &v2(), &reported(Some(0), Some(json!({})), None));
    let v1 = project_v1(&ok);
    // v1 투영은 무타입 JSON 이라 wire 형식이다.
    assert_eq!(v1.output, Some(json!("0")));
    assert_eq!(v1.exit_code, Some(0));
}
