//! v2 task 의 저장 namespace·envelope, v1 레코드 호환, 결과 확정 흐름.

use std::sync::atomic::AtomicU64;

use serde_json::{Value, json};
use tasty_memory::{ListOpts, MemoryStore, MemoryValue, PutOpts, Scope};
use tempfile::TempDir;

use super::contract::{FailureStage, TaskContract};
use super::types::TypeErrorKind;
use super::*;

fn fresh_store() -> (TempDir, MemoryStore, AtomicU64) {
    let td = tempfile::tempdir().expect("tempdir");
    let mem = MemoryStore::open(&td.path().join("mem.db")).expect("mem");
    (td, mem, AtomicU64::new(0))
}

fn opts(name: &str, command: TaskCommand) -> TaskCreateOpts {
    TaskCreateOpts {
        workspace_id: 1,
        name: name.to_string(),
        command,
        depends_on: vec![],
        on_failure: OnFailure::Abort,
        metadata: Value::Null,
        now_ms: 0,
    }
}

fn custom() -> TaskCommand {
    TaskCommand::Custom {
        ipc_method: "system.ping".into(),
        params: Value::Null,
        poll: None,
    }
}

fn run() -> TaskCommand {
    TaskCommand::Run {
        command: vec!["true".into()],
        workspace_id: 1,
        cwd: None,
    }
}

fn contract(v: Value) -> TaskContract {
    serde_json::from_value(v).expect("contract")
}

fn raw_keys(mem: &MemoryStore, prefix: &str) -> Vec<String> {
    mem.list(
        &Scope::Workspace(1),
        &ListOpts {
            prefix: Some(prefix.to_string()),
            ..Default::default()
        },
    )
    .expect("list")
    .into_iter()
    .map(|e| e.key)
    .collect()
}

fn raw_json(mem: &MemoryStore, key: &str) -> Value {
    match mem
        .get(&Scope::Workspace(1), key)
        .expect("get")
        .expect("entry")
        .value
    {
        MemoryValue::Json(v) => v,
        other => panic!("not json: {other:?}"),
    }
}

/// 754f09840 시점의 `Task` 모델. 구버전 앱이 저장소를 읽는 방식을 재현한다.
#[derive(Debug, serde::Deserialize)]
#[allow(dead_code)] // reason: 구버전 모델 복제본이라 역직렬화 성공 여부만 본다
struct LegacyTask {
    id: TaskId,
    workspace_id: u32,
    name: String,
    command: TaskCommand,
    #[serde(default)]
    depends_on: Vec<TaskId>,
    state: TaskState,
    created_at: u64,
    #[serde(default)]
    started_at: Option<u64>,
    #[serde(default)]
    finished_at: Option<u64>,
    #[serde(default)]
    result: Option<TaskResult>,
    #[serde(default)]
    on_failure: OnFailure,
    #[serde(default)]
    metadata: Value,
    #[serde(default)]
    reserved_for_fallback: bool,
}

#[test]
fn typed_tasks_live_outside_the_v1_namespace_and_old_readers_cannot_run_them() {
    let (_td, mut mem, seq) = fresh_store();
    let id = {
        let mut store = TaskStore::new(&mut mem, "_host", &seq);
        store.create(opts("v1", run())).expect("v1");
        store
            .create_typed(opts("v2", run()), contract(json!({"contract_version": 2})))
            .expect("v2")
            .id
    };

    // 구버전의 목록 조회는 v1 접두사만 본다. v2 레코드는 그 목록에 없다.
    let v1_keys = raw_keys(&mem, TASK_KEY_PREFIX);
    assert_eq!(v1_keys.len(), 1);
    assert!(!TYPED_TASK_KEY_PREFIX.starts_with(TASK_KEY_PREFIX));
    let typed_keys = raw_keys(&mem, TYPED_TASK_KEY_PREFIX);
    assert_eq!(typed_keys, vec![format!("{TYPED_TASK_KEY_PREFIX}{id}")]);

    // envelope 를 구버전 모델로 읽으면 실패한다. 다른 namespace 로 옮겨져도 실행되지 않는다.
    let envelope = raw_json(&mem, &typed_keys[0]);
    assert_eq!(envelope["record_format"], json!(TYPED_TASK_RECORD_FORMAT));
    assert!(serde_json::from_value::<LegacyTask>(envelope.clone()).is_err());

    // 반대로 envelope 없이 v2 task 를 저장했다면 구버전은 계약을 무시하고 run 으로 읽는다.
    // 버전 필드 추가만으로는 보호되지 않는다는 근거다.
    let inner = envelope["task"].clone();
    assert!(inner.get("contract").is_some());
    let legacy: LegacyTask = serde_json::from_value(inner).expect("old model ignores contract");
    assert!(matches!(legacy.command, TaskCommand::Run { .. }));

    let store = TaskStore::new(&mut mem, "_host", &seq);
    let all = store.list(1).expect("list");
    assert_eq!(all.len(), 2);
    assert!(store.get(1, &id).expect("get").expect("found").is_typed());
}

#[test]
fn contract_records_in_the_v1_namespace_and_bad_envelopes_are_reported() {
    let (_td, mut mem, seq) = fresh_store();
    let id = {
        let mut store = TaskStore::new(&mut mem, "_host", &seq);
        store
            .create_typed(opts("v2", run()), contract(json!({"contract_version": 2})))
            .expect("v2")
            .id
    };
    let inner = raw_json(&mem, &format!("{TYPED_TASK_KEY_PREFIX}{id}"))["task"].clone();
    let put = |mem: &mut MemoryStore, key: String, v: Value| {
        mem.put(
            "_host",
            &Scope::Workspace(1),
            &key,
            &MemoryValue::Json(v),
            &PutOpts::default(),
        )
        .expect("put");
    };
    put(&mut mem, format!("{TASK_KEY_PREFIX}moved"), inner.clone());
    {
        let store = TaskStore::new(&mut mem, "_host", &seq);
        assert!(store.list(1).is_err());
        assert!(store.get(1, &"moved".to_string()).is_err());
    }
    let (_td, mut mem, seq) = fresh_store();
    put(
        &mut mem,
        format!("{TYPED_TASK_KEY_PREFIX}future"),
        json!({"record_format": "tasty.task/v3", "task": inner}),
    );
    let store = TaskStore::new(&mut mem, "_host", &seq);
    assert!(
        store.list(1).is_err(),
        "모르는 형식을 v2 로 실행하지 않는다"
    );
}

/// 754f09840 이 저장한 v1 레코드.
fn v1_fixtures() -> Vec<Value> {
    vec![
        json!({
            "id": "t-1-000000", "workspace_id": 1, "name": "build",
            "command": {"kind": "run", "command": ["make"], "workspace_id": 1},
            "depends_on": [], "state": {"kind": "succeeded"},
            "created_at": 1, "started_at": 2, "finished_at": 3,
            "result": {"exit_code": 0, "output": {"pid": 9,
                "stdout": {"text": "ok\n", "truncated": false, "dropped_bytes": 0},
                "stderr": {"text": "", "truncated": false, "dropped_bytes": 0}}},
            "on_failure": {"kind": "abort"}, "metadata": null, "reserved_for_fallback": false
        }),
        json!({
            "id": "t-1-000001", "workspace_id": 1, "name": "gather",
            "command": {"kind": "reduce", "inputs": ["t-1-000000"], "strategy": {"kind": "all"}},
            "depends_on": ["t-1-000000"], "state": {"kind": "succeeded"},
            "created_at": 4, "finished_at": 5,
            "result": {"exit_code": 0, "output": [{"pid": 9}, "two", 9007199254740993_u64]},
            "on_failure": {"kind": "abort"}, "metadata": {"dag": "d"}, "reserved_for_fallback": false
        }),
        json!({
            "id": "t-1-000002", "workspace_id": 1, "name": "summarize",
            "command": {"kind": "reduce", "inputs": ["t-1-000000"],
                "strategy": {"kind": "custom", "command": "cat"}},
            "depends_on": ["t-1-000000"], "state": {"kind": "succeeded"},
            "created_at": 6, "finished_at": 7,
            "result": {"exit_code": 0, "output": "not json"},
            "on_failure": {"kind": "abort"}, "metadata": null, "reserved_for_fallback": false
        }),
    ]
}

#[test]
fn v1_fixtures_read_and_resave_to_the_same_record() {
    let (_td, mut mem, seq) = fresh_store();
    for f in v1_fixtures() {
        let key = format!("{TASK_KEY_PREFIX}{}", f["id"].as_str().unwrap());
        mem.put(
            "_host",
            &Scope::Workspace(1),
            &key,
            &MemoryValue::Json(f),
            &PutOpts::default(),
        )
        .expect("put");
    }
    {
        let mut store = TaskStore::new(&mut mem, "_host", &seq);
        let tasks = store.list(1).expect("list");
        assert_eq!(tasks.len(), 3);
        for t in &tasks {
            assert!(!t.is_typed());
            assert!(t.typed_result.is_none());
            store.put(t).expect("resave");
        }
        let all = store.get(1, &"t-1-000001".to_string()).unwrap().unwrap();
        assert_eq!(
            all.result.unwrap().output,
            Some(json!([{"pid": 9}, "two", 9007199254740993_u64]))
        );
    }
    for f in v1_fixtures() {
        let key = format!("{TASK_KEY_PREFIX}{}", f["id"].as_str().unwrap());
        assert_eq!(raw_json(&mem, &key), f, "{key}");
    }
    assert!(raw_keys(&mem, TYPED_TASK_KEY_PREFIX).is_empty());
}

fn finish(
    store: &mut TaskStore,
    id: &TaskId,
    result: TaskResult,
    state: TaskState,
) -> (Task, Vec<Task>) {
    store
        .set_state(1, id, TaskState::Running, 1)
        .expect("running");
    store.set_result(1, id, result).expect("set_result");
    store.set_state(1, id, state, 2).expect("terminal")
}

fn output(v: Value) -> TaskResult {
    TaskResult {
        exit_code: None,
        output: Some(v),
        error: None,
    }
}

#[test]
fn enum_results_succeed_for_any_declared_value_and_unknown_values_fail_validation() {
    let (_td, mut mem, seq) = fresh_store();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let c = contract(json!({
        "contract_version": 2,
        "output_schema": {"type": "enum", "values": ["pass", "revise", "review"]}
    }));
    let ok = store.create_typed(opts("ok", custom()), c.clone()).unwrap();
    let (t, _) = finish(
        &mut store,
        &ok.id,
        output(json!("revise")),
        TaskState::Succeeded,
    );
    assert_eq!(t.state, TaskState::Succeeded);
    let typed = t.typed_result.unwrap();
    assert_eq!(typed.output.to_internal(), json!("revise"));
    assert_eq!(t.result.unwrap().output, Some(json!("revise")));

    let bad = store.create_typed(opts("bad", custom()), c).unwrap();
    let (t, _) = finish(
        &mut store,
        &bad.id,
        output(json!("unknown")),
        TaskState::Succeeded,
    );
    assert!(matches!(t.state, TaskState::Failed { .. }), "{:?}", t.state);
    let e = t.typed_result.unwrap().error.unwrap();
    assert_eq!(e.stage, FailureStage::OutputValidation);
    assert_eq!(e.type_error.unwrap().kind, TypeErrorKind::UnknownEnumValue);
}

#[test]
fn nullable_optional_unit_and_missing_results_stay_distinct() {
    let (_td, mut mem, seq) = fresh_store();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let c = contract(json!({
        "contract_version": 2,
        "output_schema": {"type": "object", "fields": {
            "reviewer": {"type": "string", "nullable": true},
            "note": {"type": "string", "optional": true}
        }}
    }));
    let t = store.create_typed(opts("obj", custom()), c).unwrap();
    let (t, _) = finish(
        &mut store,
        &t.id,
        output(json!({"reviewer": null})),
        TaskState::Succeeded,
    );
    assert_eq!(t.state, TaskState::Succeeded);
    let out = t.typed_result.unwrap().output;
    assert_eq!(out.to_internal(), json!({"reviewer": null}));

    let barrier = store
        .create_typed(
            opts(
                "unit",
                TaskCommand::WaitBarrier {
                    name: Some("b".into()),
                },
            ),
            contract(json!({"contract_version": 2})),
        )
        .unwrap();
    let (t, _) = finish(
        &mut store,
        &barrier.id,
        output(json!({})),
        TaskState::Succeeded,
    );
    let typed = t.typed_result.unwrap();
    assert!(typed.has_output);
    assert_eq!(typed.output, super::types::TypedValue::Null);

    // 결과를 저장하지 않은 채 성공을 요청하면 실패로 끝난다.
    let missing = store
        .create_typed(
            opts("missing", custom()),
            contract(json!({"contract_version": 2})),
        )
        .unwrap();
    store
        .set_state(1, &missing.id, TaskState::Running, 1)
        .unwrap();
    let (t, _) = store
        .set_state(1, &missing.id, TaskState::Succeeded, 2)
        .unwrap();
    assert!(matches!(t.state, TaskState::Failed { .. }));
    let typed = t.typed_result.unwrap();
    assert!(!typed.has_output);
    assert_eq!(typed.error.unwrap().stage, FailureStage::Persistence);

    // 출력이 없는 완료와 unit 완료는 저장소에서 다시 읽어도 구별된다.
    let reread = store.get(1, &barrier.id).unwrap().unwrap();
    assert!(reread.typed_result.unwrap().has_output);
    let reread = store.get(1, &missing.id).unwrap().unwrap();
    assert!(!reread.typed_result.unwrap().has_output);
}

#[test]
fn int64_extremes_survive_the_memory_store_exactly() {
    let (td, mut mem, seq) = fresh_store();
    let values = json!([i64::MIN, i64::MAX, 9007199254740993_i64]);
    let id = {
        let mut store = TaskStore::new(&mut mem, "_host", &seq);
        let c = contract(json!({
            "contract_version": 2,
            "output_schema": {"type": "list", "items": {"type": "int64"}}
        }));
        let t = store.create_typed(opts("ints", custom()), c).unwrap();
        let (t, _) = finish(
            &mut store,
            &t.id,
            output(values.clone()),
            TaskState::Succeeded,
        );
        assert_eq!(t.state, TaskState::Succeeded);
        t.id
    };
    drop(mem);
    let mut mem = MemoryStore::open(&td.path().join("mem.db")).expect("reopen");
    let store = TaskStore::new(&mut mem, "_host", &seq);
    let t = store.get(1, &id).unwrap().unwrap();
    // 읽은 task 의 값은 내부 표현(정수)이다.
    let typed = t.typed_result.as_ref().unwrap();
    assert_eq!(
        typed.output,
        super::types::TypedValue::List(vec![
            super::types::TypedValue::Int64(i64::MIN),
            super::types::TypedValue::Int64(i64::MAX),
            super::types::TypedValue::Int64(9007199254740993),
        ])
    );
    assert_eq!(typed.output.to_internal(), values);
    // 저장된 레코드에서는 출력과 v1 투영 모두 10진 문자열이다.
    let stored = raw_json(&mem, &format!("{TYPED_TASK_KEY_PREFIX}{id}"));
    let wire = json!([
        "-9223372036854775808",
        "9223372036854775807",
        "9007199254740993"
    ]);
    assert_eq!(stored["task"]["typed_result"]["output"], wire);
    assert_eq!(stored["task"]["result"]["output"], wire);
    // v1 투영은 무타입 JSON 이라 메모리에서도 wire 형식이다.
    assert_eq!(t.result.as_ref().unwrap().output, Some(wire.clone()));
    // IPC 응답도 같은 serde 경계를 지난다.
    assert_eq!(
        serde_json::to_value(&t).unwrap()["typed_result"]["output"],
        wire
    );
}

#[test]
fn run_exit_code_is_the_output_and_execution_failures_keep_their_stage() {
    let (_td, mut mem, seq) = fresh_store();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let t = store
        .create_typed(
            opts("run", run()),
            contract(json!({"contract_version": 2, "allowed_exit_codes": [0, 7]})),
        )
        .unwrap();
    let streams = json!({"pid": 1, "stdout": {"text": "x"}});
    let (t, _) = finish(
        &mut store,
        &t.id,
        TaskResult {
            exit_code: Some(7),
            output: Some(streams.clone()),
            error: None,
        },
        TaskState::Succeeded,
    );
    assert_eq!(t.state, TaskState::Succeeded);
    let typed = t.typed_result.unwrap();
    assert_eq!(typed.output.to_internal(), json!(7));
    assert_eq!(typed.raw.execution, Some(streams));

    let f = store
        .create_typed(
            opts("fail", run()),
            contract(json!({"contract_version": 2})),
        )
        .unwrap();
    let (t, _) = finish(
        &mut store,
        &f.id,
        TaskResult {
            exit_code: None,
            output: None,
            error: Some("Run exited non-zero: code=Some(3)".into()),
        },
        TaskState::Failed {
            error: "Run exited non-zero: code=Some(3)".into(),
        },
    );
    assert!(matches!(t.state, TaskState::Failed { .. }));
    assert_eq!(
        t.typed_result.unwrap().error.unwrap().stage,
        FailureStage::Execution
    );
}

#[test]
fn contract_errors_at_creation_store_nothing() {
    let (_td, mut mem, seq) = fresh_store();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let bad = contract(json!({"contract_version": 2, "output_schema": {"type": "string"}}));
    let err = store.create_typed(opts("bad", run()), bad).unwrap_err();
    assert!(matches!(err, crate::AgentError::TypeContract(_)), "{err}");
    assert!(store.list(1).unwrap().is_empty());
}

#[test]
fn retry_clears_the_typed_result_and_delete_removes_the_typed_record() {
    let (_td, mut mem, seq) = fresh_store();
    {
        let mut store = TaskStore::new(&mut mem, "_host", &seq);
        let int = contract(json!({"contract_version": 2, "output_schema": {"type": "int64"}}));
        let t = store.create_typed(opts("i", custom()), int).unwrap();
        let (t, _) = finish(&mut store, &t.id, output(json!("x")), TaskState::Succeeded);
        assert!(t.typed_result.is_some());
        let t = store.retry(1, &t.id, false, 3).unwrap();
        assert!(t.typed_result.is_none());
        assert!(t.result.is_none());
        assert!(store.get(1, &t.id).unwrap().unwrap().is_typed());
        store
            .delete_checked(1, &t.id, TaskDeleteOpts::default())
            .unwrap();
    }
    assert!(raw_keys(&mem, TYPED_TASK_KEY_PREFIX).is_empty());
}

#[test]
fn stored_int64_that_is_not_an_integer_fails_to_read() {
    let (_td, mut mem, seq) = fresh_store();
    let id = {
        let mut store = TaskStore::new(&mut mem, "_host", &seq);
        let c = contract(json!({"contract_version": 2, "output_schema": {"type": "int64"}}));
        let t = store.create_typed(opts("i", custom()), c).unwrap();
        finish(&mut store, &t.id, output(json!(5)), TaskState::Succeeded);
        t.id
    };
    let key = format!("{TYPED_TASK_KEY_PREFIX}{id}");
    let mut record = raw_json(&mem, &key);
    assert_eq!(record["task"]["typed_result"]["output"], json!("5"));
    record["task"]["typed_result"]["output"] = json!("5.5");
    mem.put(
        "_host",
        &Scope::Workspace(1),
        &key,
        &MemoryValue::Json(record),
        &PutOpts::default(),
    )
    .expect("put");
    let store = TaskStore::new(&mut mem, "_host", &seq);
    let e = store.get(1, &id).unwrap_err().to_string();
    assert!(e.contains("stored output"), "{e}");
}

/// 직렬화한 task 레코드에서 접수 응답이 몇 벌인지 센다.
fn accepted_copies(t: &Task) -> usize {
    serde_json::to_string(t)
        .unwrap()
        .matches("\"accepted\"")
        .count()
}

/// 접수 응답은 확정 전에는 task 의 `accepted` 에만, 결과를 확정하면 `raw.accepted` 에만 있다.
/// 러너·재시작 복구·외부 보고가 지나는 `complete`(성공·실패·결과 불명), 결과 없는 실패 전이,
/// 성공 결과를 기록한 뒤의 실패 전이 모두 한 벌이고 retry 뒤에는 없다.
#[test]
fn the_accepted_response_is_kept_once_before_and_after_the_result_is_settled() {
    use super::attempt::Completion;
    use super::contract::AcceptedResponse;
    let (_td, mut mem, seq) = fresh_store();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let c = contract(json!({"contract_version": 2}));
    let accepted = AcceptedResponse::capture(&json!({"job": "J"}));
    let start = |store: &mut TaskStore| {
        let t = store.create_typed(opts("c", custom()), c.clone()).unwrap();
        store.set_accepted(1, &t.id, accepted.clone()).unwrap();
        let (t, _) = store.set_state(1, &t.id, TaskState::Running, 1).unwrap();
        // 확정 전 조회에서는 task 쪽에 보인다.
        assert_eq!(t.accepted.as_ref(), Some(&accepted));
        assert!(t.typed_result.is_none());
        assert_eq!(accepted_copies(&t), 1);
        assert_eq!(
            serde_json::to_value(&t).unwrap()["accepted"]["response"],
            json!({"job": "J"})
        );
        t.id
    };
    let settled = |t: &Task, label: &str| {
        assert!(t.accepted.is_none(), "{label}");
        let raw = &t.typed_result.as_ref().expect(label).raw;
        assert_eq!(raw.accepted.as_ref(), Some(&accepted), "{label}");
        assert_eq!(accepted_copies(t), 1, "{label}");
    };

    let id = start(&mut store);
    let done = Completion::succeeded(None, output(json!({"state": "done"})));
    let t = store.complete(1, &id, done, 2).unwrap().task;
    assert_eq!(t.state, TaskState::Succeeded);
    settled(&t, "complete succeeded");

    let id = start(&mut store);
    let t = store
        .complete(1, &id, Completion::failed(None, "poll failed".into()), 2)
        .unwrap()
        .task;
    assert!(matches!(t.state, TaskState::Failed { .. }));
    settled(&t, "complete failed");
    let t = store.retry(1, &t.id, false, 3).unwrap();
    assert!(t.accepted.is_none() && t.typed_result.is_none());
    assert_eq!(accepted_copies(&store.get(1, &t.id).unwrap().unwrap()), 0);

    let id = start(&mut store);
    let (t, _) = store
        .set_state(1, &id, TaskState::Failed { error: "x".into() }, 2)
        .unwrap();
    settled(&t, "failed without a result");

    // 성공 결과를 기록한 뒤 실패로 바꾸면 그 결과의 접수 응답을 이어받는다.
    let id = start(&mut store);
    let t = store.set_result(1, &id, output(json!(1))).unwrap();
    settled(&t, "result recorded");
    let (t, _) = store
        .set_state(1, &id, TaskState::Failed { error: "x".into() }, 2)
        .unwrap();
    settled(&t, "failed after a result");

    // 결과 불명은 결과를 확정하지 않으므로 task 쪽에 한 벌 남고 retry 가 지운다.
    let id = start(&mut store);
    let lost = Completion::lost(None, "host restart".into());
    let t = store.complete(1, &id, lost, 2).unwrap().task;
    assert!(
        matches!(t.state, TaskState::Unknown { .. }),
        "{:?}",
        t.state
    );
    assert_eq!(t.accepted.as_ref(), Some(&accepted));
    assert_eq!(accepted_copies(&t), 1);
    let t = store.retry(1, &t.id, false, 3).unwrap();
    assert_eq!(accepted_copies(&t), 0);

    let v1 = store.create(opts("v1", custom())).unwrap();
    assert!(store.set_accepted(1, &v1.id, accepted).is_err());
}

#[test]
fn an_accepted_response_over_the_cap_keeps_a_char_aligned_prefix() {
    use super::contract::{ACCEPTED_RESPONSE_CAP, AcceptedResponse};
    let small = AcceptedResponse::capture(&json!({"a": 1}));
    assert_eq!(small.response, Some(json!({"a": 1})));
    assert!(!small.truncated && small.text.is_none() && small.dropped_bytes == 0);
    assert_eq!(
        serde_json::to_value(&small).unwrap(),
        json!({"response": {"a": 1}})
    );
    // `"ab` 세 바이트 뒤에 3바이트 문자가 이어져 상한이 문자 가운데에 걸린다.
    let v = json!(format!("ab{}", "가".repeat(ACCEPTED_RESPONSE_CAP)));
    let full = v.to_string();
    let big = AcceptedResponse::capture(&v);
    let text = big.text.clone().expect("text");
    assert!(big.truncated && big.response.is_none());
    assert_eq!(text.len(), ACCEPTED_RESPONSE_CAP - 1);
    assert!(full.starts_with(&text));
    assert_eq!(big.dropped_bytes as usize, full.len() - text.len());
}

/// `artifacts` 를 없애기 전에 저장한 결과에 그 키가 남아 있어도 읽힌다.
#[test]
fn a_stored_result_with_the_removed_artifacts_key_still_reads() {
    let (_td, mut mem, seq) = fresh_store();
    let id = {
        let mut store = TaskStore::new(&mut mem, "_host", &seq);
        let c = contract(json!({"contract_version": 2}));
        let t = store.create_typed(opts("a", custom()), c).unwrap();
        finish(&mut store, &t.id, output(json!(1)), TaskState::Succeeded);
        t.id
    };
    let key = format!("{TYPED_TASK_KEY_PREFIX}{id}");
    let mut record = raw_json(&mem, &key);
    record["task"]["typed_result"]["artifacts"] =
        json!([{"name": "log", "uri": "file:///tmp/log", "bytes": 3}]);
    mem.put(
        "_host",
        &Scope::Workspace(1),
        &key,
        &MemoryValue::Json(record),
        &PutOpts::default(),
    )
    .expect("put");
    let store = TaskStore::new(&mut mem, "_host", &seq);
    let t = store.get(1, &id).unwrap().expect("task");
    assert_eq!(t.state, TaskState::Succeeded);
    assert!(
        !serde_json::to_string(&t.typed_result.unwrap())
            .unwrap()
            .contains("artifacts")
    );
}

/// Custom 의 최종 응답은 raw 에 상한까지만 둔다. 출력은 따로 저장하므로 상한을 넘는 응답도
/// 출력 값 상한 안이면 그대로 출력이 되고, 그보다 크면 출력 검증 실패로 끝나되 저장은 된다.
#[test]
fn a_custom_response_over_the_raw_cap_is_truncated_in_raw_but_not_in_the_output() {
    use super::attempt::Completion;
    use super::contract::EXECUTION_RESPONSE_CAP;
    use super::types::MAX_VALUE_BYTES;
    let (_td, mut mem, seq) = fresh_store();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let c = contract(json!({"contract_version": 2}));
    let complete = |store: &mut TaskStore, response: Value| {
        let t = store.create_typed(opts("c", custom()), c.clone()).unwrap();
        store.set_state(1, &t.id, TaskState::Running, 1).unwrap();
        let done = Completion::succeeded(None, output(response));
        let t = store.complete(1, &t.id, done, 2).expect("stored").task;
        // 조회도 저장된 레코드에서 같은 값을 읽는다.
        assert_eq!(store.get(1, &t.id).unwrap().as_ref(), Some(&t));
        t
    };

    let t = complete(&mut store, json!({"ok": true}));
    let raw = &t.typed_result.as_ref().unwrap().raw;
    assert_eq!(raw.execution, Some(json!({"ok": true})));
    assert!(raw.execution_truncated.is_none());

    // raw 상한은 넘지만 출력 값 상한 안이다.
    let mid = json!("가".repeat(EXECUTION_RESPONSE_CAP / 2));
    let t = complete(&mut store, mid.clone());
    assert_eq!(t.state, TaskState::Succeeded);
    let typed = t.typed_result.as_ref().unwrap();
    assert_eq!(typed.output.to_wire(), mid);
    assert!(typed.raw.execution.is_none());
    let head = typed.raw.execution_truncated.as_ref().expect("truncated");
    let full = mid.to_string();
    let text = head.text.as_deref().expect("text");
    assert!(head.truncated && head.response.is_none());
    assert!(text.len() <= EXECUTION_RESPONSE_CAP && full.starts_with(text));
    assert_eq!(head.dropped_bytes as usize, full.len() - text.len());

    // memory 값 상한(1 MiB)보다 큰 응답.
    let huge = json!("x".repeat(1024 * 1024 + 1));
    let t = complete(&mut store, huge);
    assert!(matches!(t.state, TaskState::Failed { .. }), "{:?}", t.state);
    let typed = t.typed_result.as_ref().unwrap();
    assert!(!typed.has_output);
    let error = typed.error.as_ref().unwrap();
    assert_eq!(error.stage, super::contract::FailureStage::OutputValidation);
    assert!(
        error.message.contains(&MAX_VALUE_BYTES.to_string()),
        "{}",
        error.message
    );
    assert!(typed.raw.execution.is_none());
    assert!(typed.raw.execution_truncated.as_ref().unwrap().truncated);
}

/// 계약 없는(v1) custom 은 응답이 곧 출력이라 레코드에 한 벌만 두고 자르지 않는다(하류가 그대로
/// 읽는다). 레코드가 memory 값 상한을 넘으면 저장이 거절되고, 러너는 같은 보고를 출력 없는
/// 짧은 실패로 바꿔 기록한다. 이스케이프로 커지는 응답도 같은 경로다.
#[test]
fn a_v1_custom_response_too_large_to_store_settles_as_a_short_failure() {
    use super::attempt::Completion;
    let (_td, mut mem, seq) = fresh_store();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    for response in [
        json!("x".repeat(1100 * 1024)),
        // 문자마다 직렬화가 6 바이트(\u0001)라 문자 수는 상한의 약 1/5 이다.
        json!("\u{1}".repeat(200 * 1024)),
    ] {
        let t = store.create(opts("c", custom())).unwrap();
        store.set_state(1, &t.id, TaskState::Running, 1).unwrap();
        let report = Completion::succeeded(None, output(response));
        let e = store
            .complete(1, &t.id, report.clone(), 2)
            .expect_err("too large to store");
        assert!(
            matches!(
                e,
                crate::AgentError::Memory(tasty_memory::MemoryError::ValueTooLarge { .. })
            ),
            "{e}"
        );
        // 저장이 거절된 보고는 상태도 결과도 바꾸지 않는다.
        let kept = store.get(1, &t.id).unwrap().unwrap();
        assert_eq!(kept.state, TaskState::Running);
        assert!(kept.result.is_none());

        let shrunk = crate::runner::shrink_too_large_completion(&e, &report).expect("shrunk");
        let t = store.complete(1, &t.id, shrunk, 3).expect("stored").task;
        assert!(matches!(t.state, TaskState::Failed { .. }), "{:?}", t.state);
        let result = t.result.as_ref().unwrap();
        assert!(result.output.is_none());
        let error = result.error.as_deref().unwrap();
        assert!(
            error.starts_with("the result could not be stored: memory: value too large"),
            "{error}"
        );
    }
}
