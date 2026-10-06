//! v2 그래프 제출 — 전체 검증, 활성화 경계, 데이터 엣지의 readiness·삭제 보호, 입력 해석.

use std::sync::atomic::AtomicU64;

use serde_json::{Value, json};
use tasty_memory::{ListOpts, MemoryStore, Scope};
use tempfile::TempDir;

use super::binding::resolve_inputs;
use super::contract::FailureStage;
use super::types::{TypeErrorKind, TypedValue};
use super::*;
use crate::AgentError;

fn fresh() -> (TempDir, MemoryStore, AtomicU64) {
    let td = tempfile::tempdir().expect("tempdir");
    let mem = MemoryStore::open(&td.path().join("mem.db")).expect("mem");
    (td, mem, AtomicU64::new(0))
}

fn spec(v: Value) -> TaskGraphSpec {
    serde_json::from_value(v).expect("graph spec")
}

fn custom(params: Value) -> Value {
    json!({"kind": "custom", "ipc_method": "system.ping", "params": params})
}

fn failure(e: AgentError) -> contract::TaskFailure {
    match e {
        AgentError::TypeContract(f) => *f,
        other => panic!("expected a type contract error, got {other:?}"),
    }
}

fn get(store: &TaskStore, id: &str) -> Task {
    store.get(1, &id.to_string()).unwrap().expect("task")
}

/// Ready 인 task 를 실행한 것처럼 결과와 성공을 기록한다.
fn finish(store: &mut TaskStore, id: &str, output: Value) {
    let id = id.to_string();
    store.set_state(1, &id, TaskState::Running, 1).unwrap();
    store
        .set_result(
            1,
            &id,
            TaskResult {
                exit_code: None,
                output: Some(output),
                error: None,
            },
        )
        .unwrap();
    store.set_state(1, &id, TaskState::Succeeded, 2).unwrap();
}

fn fail(store: &mut TaskStore, id: &str) {
    let id = id.to_string();
    store.set_state(1, &id, TaskState::Running, 1).unwrap();
    store
        .set_state(
            1,
            &id,
            TaskState::Failed {
                error: "boom".into(),
            },
            2,
        )
        .unwrap();
}

fn verdict_graph(label_binding: Value) -> Value {
    json!({"contract_version": 2,
    "types": {"Verdict": {"type": "object", "fields": {
        "count": {"type": "int64"},
        "verdict": {"type": "enum", "values": ["pass", "revise"]}}}},
    "tasks": [
        {"id": "count.user", "command": custom(json!({})),
         "input_schema": {"type": "object", "fields": {"n": {"type": "int64"}}},
         "bindings": {"n": {"from_task": "producer", "pointer": "/count"}}},
        {"id": "label.user", "command": custom(json!({})),
         "input_schema": {"type": "object", "fields": {"label": {"type": "string"}}},
         "bindings": {"label": label_binding}},
        {"id": "producer", "command": custom(json!({})), "output_schema": {"ref": "Verdict"}}
    ]})
}

#[test]
fn an_int64_field_feeds_an_int64_input_and_a_string_input_only_with_a_conversion() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);

    let bad = store
        .submit_graph(
            1,
            spec(verdict_graph(
                json!({"from_task": "producer", "pointer": "/count"}),
            )),
            0,
        )
        .unwrap_err();
    let f = failure(bad);
    assert_eq!(f.stage, FailureStage::Input);
    assert_eq!(f.task_id.as_deref(), Some("label.user"));
    assert_eq!(f.location.as_deref(), Some("/tasks/1/bindings/label"));
    let te = f.type_error.expect("type error");
    assert_eq!(te.kind, TypeErrorKind::Incompatible);
    assert_eq!(
        (te.expected.as_str(), te.actual.as_str()),
        ("string", "int64")
    );
    assert!(
        f.message.contains("producer") && f.message.contains("/count"),
        "{}",
        f.message
    );
    // 잘못된 제출은 아무것도 남기지 않는다.
    assert!(store.list(1).unwrap().is_empty());

    let (gid, tasks) = store
        .submit_graph(
            1,
            spec(verdict_graph(
                json!({"from_task": "producer", "pointer": "/count", "convert": "to_string"}),
            )),
            0,
        )
        .unwrap();
    assert_eq!(tasks.len(), 3);
    assert!(
        tasks
            .iter()
            .all(|t| t.graph_id.as_deref() == Some(gid.as_str()))
    );
    // 뒤에 선언한 producer 를 앞의 task 가 참조해도 된다. 소비자는 producer 를 기다린다.
    assert_eq!(get(&store, "producer").state, TaskState::Ready);
    assert_eq!(get(&store, "count.user").state, TaskState::Waiting);
    assert_eq!(get(&store, "label.user").state, TaskState::Waiting);

    finish(
        &mut store,
        "producer",
        json!({"count": 3, "verdict": "pass"}),
    );
    let count_user = get(&store, "count.user");
    let label_user = get(&store, "label.user");
    assert_eq!(count_user.state, TaskState::Ready);
    assert_eq!(label_user.state, TaskState::Ready);

    let lookup = |id: &TaskId| store.get(1, id).unwrap();
    let snap = resolve_inputs(
        &count_user,
        count_user.contract.as_ref().unwrap(),
        None,
        5,
        &lookup,
    );
    assert!(snap.failure.is_none(), "{:?}", snap.failure);
    assert_eq!(
        snap.value,
        TypedValue::Object([("n".to_string(), TypedValue::Int64(3))].into())
    );
    assert_eq!(snap.sources[0].from_task, "producer");
    assert_eq!(
        snap.sources[0].producer_attempt.as_deref(),
        Some("producer#1")
    );
    let snap = resolve_inputs(
        &label_user,
        label_user.contract.as_ref().unwrap(),
        None,
        5,
        &lookup,
    );
    assert_eq!(snap.value.to_internal(), json!({"label": "3"}));
}

#[test]
fn dotted_ids_and_escaped_pointers_reach_the_right_field() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let graph = json!({"contract_version": 2, "tasks": [
        {"id": "stage.one", "command": custom(json!({})),
         "output_schema": {"type": "object", "fields": {
             "a/b": {"type": "string"}, "a~b": {"type": "int64"}}}},
        {"id": "stage.two", "command": custom(json!({})),
         "input_schema": {"type": "object", "fields": {
             "slash": {"type": "string"}, "tilde": {"type": "int64"}}},
         "bindings": {
             "slash": {"from_task": "stage.one", "pointer": "/a~1b"},
             "tilde": {"from_task": "stage.one", "pointer": "/a~0b"}}}
    ]});
    store.submit_graph(1, spec(graph), 0).unwrap();
    finish(&mut store, "stage.one", json!({"a/b": "slash", "a~b": 7}));
    let consumer = get(&store, "stage.two");
    assert_eq!(consumer.state, TaskState::Ready);
    let lookup = |id: &TaskId| store.get(1, id).unwrap();
    let snap = resolve_inputs(
        &consumer,
        consumer.contract.as_ref().unwrap(),
        None,
        3,
        &lookup,
    );
    assert_eq!(
        snap.value.to_internal(),
        json!({"slash": "slash", "tilde": 7})
    );
}

#[test]
fn cycles_missing_fields_and_unknown_sources_are_refused_before_anything_is_stored() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let int_in = json!({"type": "object", "fields": {"v": {"type": "json"}}});
    let cycle = json!({"contract_version": 2, "tasks": [
        {"id": "a", "command": custom(json!({})), "input_schema": int_in,
         "bindings": {"v": {"from_task": "b"}}},
        {"id": "b", "command": custom(json!({})), "input_schema": int_in,
         "bindings": {"v": {"from_task": "a"}}}
    ]});
    let f = failure(store.submit_graph(1, spec(cycle), 0).unwrap_err());
    assert_eq!(f.location.as_deref(), Some("/tasks"));
    assert!(f.message.starts_with("dependency cycle"), "{}", f.message);

    let missing_field = json!({"contract_version": 2, "tasks": [
        {"id": "p", "command": custom(json!({})),
         "output_schema": {"type": "object", "fields": {"count": {"type": "int64"}}}},
        {"id": "c", "command": custom(json!({})),
         "input_schema": {"type": "object", "fields": {"n": {"type": "int64"}}},
         "bindings": {"n": {"from_task": "p", "pointer": "/cnt"}}}
    ]});
    let f = failure(store.submit_graph(1, spec(missing_field), 0).unwrap_err());
    assert_eq!(f.location.as_deref(), Some("/tasks/1/bindings/n/pointer"));
    assert_eq!(f.type_error.unwrap().kind, TypeErrorKind::MissingField);

    let unknown = json!({"contract_version": 2, "tasks": [
        {"id": "c", "command": custom(json!({})),
         "input_schema": {"type": "object", "fields": {"n": {"type": "int64"}}},
         "bindings": {"n": {"from_task": "ghost"}}}
    ]});
    let f = failure(store.submit_graph(1, spec(unknown), 0).unwrap_err());
    assert_eq!(f.location.as_deref(), Some("/tasks/0/bindings/n/from_task"));

    let unbound = json!({"contract_version": 2, "tasks": [
        {"id": "c", "command": custom(json!({})),
         "input_schema": {"type": "object", "fields": {"n": {"type": "int64"}}}}
    ]});
    let f = failure(store.submit_graph(1, spec(unbound), 0).unwrap_err());
    assert_eq!(f.location.as_deref(), Some("/tasks/0/bindings/n"));

    assert!(store.list(1).unwrap().is_empty());
    assert!(
        mem_keys(&store, TASK_GRAPH_KEY_PREFIX).is_empty(),
        "no activation record for a refused graph"
    );
}

fn mem_keys(store: &TaskStore, prefix: &str) -> Vec<String> {
    store
        .memory()
        .list(
            &Scope::Workspace(1),
            &ListOpts {
                prefix: Some(prefix.to_string()),
                ..Default::default()
            },
        )
        .unwrap()
        .into_iter()
        .map(|e| e.key)
        .collect()
}

#[test]
fn defaults_fill_unbound_fields_and_literals_are_checked_against_the_field() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let input = json!({"type": "object", "fields": {
        "mode": {"type": "enum", "values": ["fast", "slow"], "default": "fast"},
        "limit": {"type": "int64"}}});
    let bad = json!({"contract_version": 2, "tasks": [
        {"id": "c", "command": custom(json!({})), "input_schema": input,
         "bindings": {"limit": {"literal": "ten"}}}]});
    let f = failure(store.submit_graph(1, spec(bad), 0).unwrap_err());
    assert_eq!(f.location.as_deref(), Some("/tasks/0/bindings/limit"));
    assert_eq!(f.type_error.unwrap().path, "/literal");

    let ok = json!({"contract_version": 2, "tasks": [
        {"id": "c", "command": custom(json!({})), "input_schema": input,
         "bindings": {"limit": {"literal": "10"}}}]});
    store.submit_graph(1, spec(ok), 0).unwrap();
    let c = get(&store, "c");
    assert_eq!(c.state, TaskState::Ready);
    let lookup = |id: &TaskId| store.get(1, id).unwrap();
    let snap = resolve_inputs(&c, c.contract.as_ref().unwrap(), None, 1, &lookup);
    assert_eq!(
        snap.value.to_internal(),
        json!({"mode": "fast", "limit": 10})
    );
}

#[test]
fn a_staged_graph_never_becomes_ready_until_its_activation_record_exists() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let graph = json!({"contract_version": 2, "tasks": [
        {"id": "root", "command": custom(json!({}))},
        {"id": "leaf", "command": custom(json!({})), "depends_on": ["root"]}
    ]});
    let plan = store.plan_graph(1, spec(graph), 0).unwrap();
    store.stage_graph(&plan).unwrap();

    // 활성화 전에 다른 task 의 종결로 cascade 가 일어나도(실행 tick 대신) Ready 가 되지 않는다.
    let all = store.list(1).unwrap();
    assert!(all.iter().all(|t| t.state == TaskState::Waiting));
    let graph = store.readiness_graph(1, &all).unwrap();
    assert_eq!(graph.evaluate_readiness(&"root".to_string()), None);
    let other = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "unrelated".into(),
            command: TaskCommand::Run {
                command: vec!["true".into()],
                workspace_id: 1,
                cwd: None,
            },
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: Value::Null,
            now_ms: 0,
        })
        .unwrap();
    store
        .set_state(1, &other.id, TaskState::Running, 1)
        .unwrap();
    store
        .set_state(1, &other.id, TaskState::Succeeded, 2)
        .unwrap();
    assert_eq!(get(&store, "root").state, TaskState::Waiting);

    let tasks = store.activate_graph(&plan, 3).unwrap();
    assert_eq!(tasks.len(), 2);
    assert_eq!(get(&store, "root").state, TaskState::Ready);
    assert_eq!(get(&store, "leaf").state, TaskState::Waiting);
    assert_eq!(
        get(&store, "root").metadata["dag"],
        json!(plan.graph_id),
        "the submitted graph shows as one group"
    );
}

fn fallback_graph(consumer: Value) -> Value {
    let out = json!({"type": "object", "fields": {"value": {"type": "int64"}}});
    json!({"contract_version": 2, "tasks": [
        {"id": "main", "command": custom(json!({})), "output_schema": out,
         "on_failure": {"kind": "fallback", "task": "recover"}},
        {"id": "recover", "command": custom(json!({})), "output_schema": out},
        consumer
    ]})
}

#[test]
fn a_fallback_replaces_readiness_but_not_the_main_output() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let input = json!({"type": "object", "fields": {"v": {"type": "int64"}}});

    // 제어는 fallback 으로 복구되지만 값은 main 에서만 읽는 조합은 값 출처가 없다.
    let mixed = fallback_graph(json!({"id": "use", "command": custom(json!({})),
        "depends_on": ["main"], "input_schema": input,
        "bindings": {"v": {"from_task": "main", "pointer": "/value"}}}));
    let f = failure(store.submit_graph(1, spec(mixed), 0).unwrap_err());
    assert_eq!(f.location.as_deref(), Some("/tasks/2/bindings"));
    assert!(store.list(1).unwrap().is_empty());

    let recovered = fallback_graph(json!({"id": "use", "command": custom(json!({})),
        "depends_on": ["main"], "input_schema": input,
        "bindings": {"v": {"one_of": [
            {"from_task": "main", "pointer": "/value"},
            {"from_task": "recover", "pointer": "/value"}]}}}));
    store.submit_graph(1, spec(recovered), 0).unwrap();
    assert_eq!(
        get(&store, "recover").state,
        TaskState::Waiting,
        "dormant until main fails"
    );
    fail(&mut store, "main");
    assert_eq!(get(&store, "recover").state, TaskState::Ready);
    assert_eq!(get(&store, "use").state, TaskState::Waiting);
    finish(&mut store, "recover", json!({"value": 41}));
    let consumer = get(&store, "use");
    assert_eq!(consumer.state, TaskState::Ready);
    let lookup = |id: &TaskId| store.get(1, id).unwrap();
    let snap = resolve_inputs(
        &consumer,
        consumer.contract.as_ref().unwrap(),
        None,
        3,
        &lookup,
    );
    assert_eq!(snap.value.to_internal(), json!({"v": 41}));
    assert_eq!(snap.sources.len(), 1);
    assert_eq!(snap.sources[0].from_task, "recover");
}

#[test]
fn a_main_only_binding_skips_the_consumer_when_the_fallback_ran() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let input = json!({"type": "object", "fields": {"v": {"type": "int64"}}});
    let main_only = fallback_graph(json!({"id": "use", "command": custom(json!({})),
        "input_schema": input,
        "bindings": {"v": {"from_task": "main", "pointer": "/value"}}}));
    store.submit_graph(1, spec(main_only), 0).unwrap();
    fail(&mut store, "main");
    finish(&mut store, "recover", json!({"value": 41}));
    // main 의 성공 경로로 제한한 소비자는 fallback 의 출력을 main 출력으로 받지 않는다.
    assert_eq!(get(&store, "use").state, TaskState::Skipped);
}

#[test]
fn v2_fallbacks_must_be_declared_typed_tasks() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let v1 = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "v1".into(),
            command: TaskCommand::Run {
                command: vec!["true".into()],
                workspace_id: 1,
                cwd: None,
            },
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: Value::Null,
            now_ms: 0,
        })
        .unwrap();
    let to_v1 = json!({"contract_version": 2, "tasks": [
        {"id": "m", "command": custom(json!({})),
         "on_failure": {"kind": "fallback", "task": v1.id}}]});
    let f = failure(store.submit_graph(1, spec(to_v1), 0).unwrap_err());
    assert_eq!(f.location.as_deref(), Some("/tasks/0/on_failure/task"));

    let inline = json!({"contract_version": 2, "tasks": [
        {"id": "m", "command": custom(json!({})),
         "on_failure": {"kind": "fallback", "inline": {"name": "x",
             "command": {"kind": "run", "command": ["true"], "workspace_id": 1}}}}]});
    let f = failure(store.submit_graph(1, spec(inline), 0).unwrap_err());
    assert!(f.message.contains("inline fallback"), "{}", f.message);

    let continue_down = json!({"contract_version": 2, "tasks": [
        {"id": "p", "command": custom(json!({}))},
        {"id": "c", "command": custom(json!({})), "on_failure": {"kind": "continue_downstream"},
         "input_schema": {"type": "object", "fields": {"v": {"type": "json"}}},
         "bindings": {"v": {"from_task": "p"}}}]});
    let f = failure(store.submit_graph(1, spec(continue_down), 0).unwrap_err());
    assert_eq!(f.location.as_deref(), Some("/tasks/1/on_failure"));
    assert_eq!(store.list(1).unwrap().len(), 1, "only the v1 task remains");
}

#[test]
fn a_retried_producer_does_not_change_a_stored_snapshot() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let graph = json!({"contract_version": 2, "tasks": [
        {"id": "p", "command": custom(json!({})), "output_schema": {"type": "string"}},
        {"id": "c", "command": custom(json!({})),
         "input_schema": {"type": "object", "fields": {"text": {"type": "string"}}},
         "bindings": {"text": {"from_task": "p"}}}]});
    store.submit_graph(1, spec(graph), 0).unwrap();
    finish(&mut store, "p", json!("${task.p.output} $(rm -rf /)"));
    let c = get(&store, "c");
    let snap = {
        let lookup = |id: &TaskId| store.get(1, id).unwrap();
        resolve_inputs(&c, c.contract.as_ref().unwrap(), None, 3, &lookup)
    };
    store.set_input_snapshot(1, &c.id, snap).unwrap();
    store.set_state(1, &c.id, TaskState::Running, 4).unwrap();

    // producer 를 실패시켜 재시도해도 실행 중인 소비자의 snapshot 은 그대로다.
    let p = "p".to_string();
    store.retry(1, &p, false, 5).unwrap_err();
    let stored = get(&store, "c").input_snapshot.expect("snapshot");
    // 값 안의 placeholder 와 셸 구문은 문자열 그대로다.
    assert_eq!(
        stored.value.to_internal(),
        json!({"text": "${task.p.output} $(rm -rf /)"})
    );
    assert_eq!(stored.sources[0].producer_attempt.as_deref(), Some("p#1"));
    // 저장된 레코드를 다시 읽어도 같다(int64 가 아닌 값도 스키마로 읽힌다).
    assert_eq!(get(&store, "c").input_snapshot, Some(stored));
}

#[test]
fn binding_sources_protect_producers_from_deletion_and_prune_the_activation_record() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let graph = json!({"contract_version": 2, "tasks": [
        {"id": "p", "command": custom(json!({}))},
        {"id": "c", "command": custom(json!({})),
         "input_schema": {"type": "object", "fields": {"v": {"type": "json"}}},
         "bindings": {"v": {"from_task": "p"}}}]});
    let (gid, _) = store.submit_graph(1, spec(graph), 0).unwrap();
    let p = "p".to_string();
    match store.delete_checked(1, &p, TaskDeleteOpts::default()) {
        Err(AgentError::TaskReferenced { referenced_by, .. }) => {
            assert_eq!(referenced_by, vec!["c".to_string()]);
        }
        other => panic!("expected TaskReferenced, got {other:?}"),
    }
    assert_eq!(mem_keys(&store, TASK_GRAPH_KEY_PREFIX).len(), 1);
    let report = store
        .delete_checked(
            1,
            &p,
            TaskDeleteOpts {
                cascade: true,
                force: false,
            },
        )
        .unwrap();
    assert_eq!(report.deleted.len(), 2);
    assert!(mem_keys(&store, TASK_GRAPH_KEY_PREFIX).is_empty(), "{gid}");
}

#[test]
fn a_one_of_with_no_succeeded_source_skips_a_required_input() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let graph = json!({"contract_version": 2, "tasks": [
        {"id": "a", "command": custom(json!({}))},
        {"id": "b", "command": custom(json!({}))},
        {"id": "c", "command": custom(json!({})),
         "input_schema": {"type": "object", "fields": {"v": {"type": "json"}}},
         "bindings": {"v": {"one_of": [{"from_task": "a"}, {"from_task": "b"}]}}},
        {"id": "d", "command": custom(json!({})),
         "input_schema": {"type": "object", "fields": {"v": {"type": "json", "optional": true}}},
         "bindings": {"v": {"one_of": [{"from_task": "a"}, {"from_task": "b"}]}}}]});
    store.submit_graph(1, spec(graph), 0).unwrap();
    fail(&mut store, "a");
    assert_eq!(
        get(&store, "c").state,
        TaskState::Waiting,
        "waits for every source"
    );
    fail(&mut store, "b");
    // 필수 입력은 null 로 채우지 않고 skip 한다. optional 입력은 비운 채 진행한다.
    assert_eq!(get(&store, "c").state, TaskState::Skipped);
    let d = get(&store, "d");
    assert_eq!(d.state, TaskState::Ready);
    let lookup = |id: &TaskId| store.get(1, id).unwrap();
    let snap = resolve_inputs(&d, d.contract.as_ref().unwrap(), None, 3, &lookup);
    assert_eq!(snap.value.to_internal(), json!({}));
}

#[test]
fn a_graph_above_the_task_limit_is_refused_and_one_at_the_limit_is_stored() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let tasks = |n: usize| -> Vec<Value> {
        (0..n)
            .map(|i| json!({"id": format!("t{i}"), "command": custom(json!({}))}))
            .collect()
    };
    let over = json!({"contract_version": 2, "tasks": tasks(MAX_GRAPH_TASKS + 1)});
    let f = failure(store.submit_graph(1, spec(over), 0).unwrap_err());
    assert_eq!(f.location.as_deref(), Some("/tasks"));
    assert!(
        f.message.contains(&MAX_GRAPH_TASKS.to_string()),
        "{}",
        f.message
    );
    assert!(store.list(1).unwrap().is_empty());

    let at = json!({"contract_version": 2, "tasks": tasks(MAX_GRAPH_TASKS)});
    let (_, stored) = store.submit_graph(1, spec(at), 0).unwrap();
    assert_eq!(stored.len(), MAX_GRAPH_TASKS);
}

#[test]
fn a_readiness_failure_after_the_record_reports_an_active_graph_without_rollback() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let graph = json!({"contract_version": 2, "tasks": [
        {"id": "a", "command": custom(json!({}))},
        {"id": "b", "command": custom(json!({})), "depends_on": ["a"]}]});
    let plan = store.plan_graph(1, spec(graph.clone()), 0).unwrap();
    store.stage_graph(&plan).unwrap();
    super::store::FAIL_ACTIVATION_PUT.with(|f| f.set(true));
    match store.activate_graph(&plan, 1) {
        Err(AgentError::GraphPartiallyActivated { graph_id, source }) => {
            assert_eq!(graph_id, plan.graph_id);
            assert!(source.to_string().contains("injected"), "{source}");
        }
        other => panic!("expected GraphPartiallyActivated, got {other:?}"),
    }
    // 레코드는 남아 그래프가 활성이고, task 는 지우지 않는다.
    assert_eq!(mem_keys(&store, TASK_GRAPH_KEY_PREFIX).len(), 1);
    assert_eq!(store.list(1).unwrap().len(), 2);
    // 같은 id 로 다시 내면 거절된다(복구는 남은 task 를 지우고 다른 id 로).
    let f = failure(store.submit_graph(1, spec(graph), 2).unwrap_err());
    assert_eq!(f.location.as_deref(), Some("/tasks/0/id"));
}
