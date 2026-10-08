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

fn one_of_graph() -> Value {
    let input = json!({"type": "object", "fields": {"v": {"type": "int64"}}});
    fallback_graph(json!({"id": "use", "command": custom(json!({})),
        "depends_on": ["main"], "input_schema": input,
        "bindings": {"v": {"one_of": [
            {"from_task": "main", "pointer": "/value"},
            {"from_task": "recover", "pointer": "/value"}]}}}))
}

#[test]
fn a_typed_task_whose_fallback_ran_cannot_be_retried() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    store.submit_graph(1, spec(one_of_graph()), 0).unwrap();
    fail(&mut store, "main");
    let main = "main".to_string();
    // fallback 이 Ready 인 동안에도 거절한다(곧 실행된다).
    let e = store.retry(1, &main, true, 3).unwrap_err();
    assert!(
        matches!(e, AgentError::InvalidArgument(ref m) if m.contains("recover")),
        "{e:?}"
    );
    finish(&mut store, "recover", json!({"value": 41}));
    store.retry(1, &main, true, 4).unwrap_err();
    assert!(matches!(
        get(&store, "main").state,
        TaskState::Failed { .. }
    ));
    // 소비자는 fallback 값 하나만 받는다.
    let consumer = get(&store, "use");
    let lookup = |id: &TaskId| store.get(1, id).unwrap();
    let snap = resolve_inputs(
        &consumer,
        consumer.contract.as_ref().unwrap(),
        None,
        5,
        &lookup,
    );
    assert!(snap.failure.is_none(), "{:?}", snap.failure);
}

/// 재시도는 새 회차를 열지만 이미 실패를 받은 하류를 되감지 않는다. 되감으면 그 하류가 이전
/// 회차의 실패 전파와 다른 결과로 다시 실행된다.
#[test]
fn a_typed_task_whose_fallback_failed_is_retried_without_rewinding_its_consumer() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    store.submit_graph(1, spec(one_of_graph()), 0).unwrap();
    fail(&mut store, "main");
    fail(&mut store, "recover");
    assert_eq!(get(&store, "use").state, TaskState::Skipped);
    let main = "main".to_string();
    let e = store.retry(1, &main, true, 3).unwrap_err();
    assert!(
        matches!(e, AgentError::InvalidArgument(ref m) if m.contains("reset_downstream")),
        "{e:?}"
    );
    store.retry(1, &main, false, 3).expect("retry");
    assert_eq!(get(&store, "main").state, TaskState::Ready);
    finish(&mut store, "main", json!({"value": 7}));
    assert_eq!(get(&store, "main").state, TaskState::Succeeded);
    assert_eq!(get(&store, "use").state, TaskState::Skipped);
}

/// 재시작 뒤 순번이 0 부터 다시 시작해도 살아 있는 그래프의 ID 를 다시 쓰지 않는다.
#[test]
fn a_restarted_sequence_does_not_reuse_a_live_graph_id() {
    let (_td, mut mem, seq) = fresh();
    let graph = |id: &str| {
        spec(json!({"contract_version": 2, "tasks": [
        {"id": id, "command": custom(json!({}))}]}))
    };
    let (first, _) = TaskStore::new(&mut mem, "_host", &seq)
        .submit_graph(1, graph("before"), 0)
        .unwrap();
    let restarted = AtomicU64::new(0);
    let mut store = TaskStore::new(&mut mem, "_host", &restarted);
    let (second, _) = store.submit_graph(1, graph("after"), 0).unwrap();
    assert_ne!(second, first);
    assert_eq!(
        get(&store, "before").graph_id.as_deref(),
        Some(first.as_str())
    );
}

/// wait_barrier 는 이름을 command `name` 이나 `input_mapping.barrier` 중 하나에서만 받는다.
/// 둘 다·둘 다 없음·string 이 아닌 입력·규칙에 맞지 않는 정적 이름은 제출 때 위치와 함께 거절한다.
#[test]
fn a_wait_barrier_takes_its_name_from_exactly_one_place() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let named_in = json!({"type": "object", "fields": {"gate": {"type": "string"}}});
    let refused = |store: &mut TaskStore, task: Value| {
        let graph = json!({"contract_version": 2, "tasks": [task]});
        failure(store.submit_graph(1, spec(graph), 0).unwrap_err())
    };
    let f = refused(
        &mut store,
        json!({"id": "w", "command": {"kind": "wait_barrier", "name": "b"},
               "input_schema": named_in, "bindings": {"gate": {"literal": "g"}},
               "input_mapping": {"barrier": "/gate"}}),
    );
    assert_eq!(
        f.location.as_deref(),
        Some("/tasks/0/input_mapping/barrier")
    );
    assert!(f.message.contains("not both"), "{}", f.message);

    let f = refused(
        &mut store,
        json!({"id": "w", "command": {"kind": "wait_barrier"}}),
    );
    assert_eq!(f.location.as_deref(), Some("/tasks/0/command"));
    assert!(f.message.contains("needs a barrier name"), "{}", f.message);

    let f = refused(
        &mut store,
        json!({"id": "w", "command": {"kind": "wait_barrier"},
               "input_schema": {"type": "object", "fields": {"gate": {"type": "int64"}}},
               "bindings": {"gate": {"literal": 3}}, "input_mapping": {"barrier": "/gate"}}),
    );
    assert_eq!(
        f.location.as_deref(),
        Some("/tasks/0/input_mapping/barrier")
    );
    assert!(
        f.message.contains("a barrier name is a string"),
        "{}",
        f.message
    );

    let f = refused(
        &mut store,
        json!({"id": "w", "command": {"kind": "wait_barrier", "name": "v6S"}}),
    );
    assert_eq!(f.location.as_deref(), Some("/tasks/0/command/name"));
    assert!(f.message.contains("invalid char at 2"), "{}", f.message);

    let f = refused(
        &mut store,
        json!({"id": "r", "command": {"kind": "run", "workspace_id": 1, "command": ["true"]},
               "input_schema": named_in, "bindings": {"gate": {"literal": "g"}},
               "input_mapping": {"barrier": "/gate"}}),
    );
    assert_eq!(
        f.location.as_deref(),
        Some("/tasks/0/input_mapping/barrier")
    );
    assert!(
        f.message.contains("applies to wait_barrier"),
        "{}",
        f.message
    );

    // v1 task 는 입력이 없으므로 이름이 반드시 있어야 한다.
    let v1 = store.create(TaskCreateOpts {
        workspace_id: 1,
        name: "w".into(),
        command: TaskCommand::WaitBarrier { name: None },
        depends_on: vec![],
        on_failure: OnFailure::Abort,
        metadata: Value::Null,
        now_ms: 0,
    });
    assert!(matches!(v1, Err(AgentError::InvalidArgument(_))), "{v1:?}");
    assert!(store.list(1).unwrap().is_empty());
}

/// 입력에서 받은 barrier 이름은 snapshot 의 `execution.barrier` 에 고정된다. 이름 규칙에 맞지 않으면
/// 실행하지 않고 input 단계에서 `/input_mapping/barrier` 위치로 실패한다.
#[test]
fn a_barrier_name_from_input_is_resolved_and_checked_before_running() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let graph = json!({"contract_version": 2, "tasks": [
        {"id": "p", "command": custom(json!({})),
         "output_schema": {"type": "object", "fields": {"gate": {"type": "string"}}}},
        {"id": "w", "command": {"kind": "wait_barrier"},
         "input_schema": {"type": "object", "fields": {"gate": {"type": "string"}}},
         "bindings": {"gate": {"from_task": "p", "pointer": "/gate"}},
         "input_mapping": {"barrier": "/gate"}}
    ]});
    store.submit_graph(1, spec(graph), 0).unwrap();
    let w = get(&store, "w");
    assert_eq!(w.barrier_name(), None);
    finish(&mut store, "p", json!({"gate": "release.v2"}));
    let w = get(&store, "w");
    let lookup = |id: &String| store.get(1, id).ok().flatten();
    let snap = resolve_inputs(&w, w.contract.as_ref().unwrap(), None, 5, &lookup);
    assert!(snap.failure.is_none(), "{:?}", snap.failure);
    assert_eq!(snap.execution.barrier.as_deref(), Some("release.v2"));
    let mut resolved = w.clone();
    resolved.input_snapshot = Some(snap);
    assert_eq!(resolved.barrier_name(), Some("release.v2"));

    let mut bad = w.clone();
    let p = get(&store, "p");
    let mut p_bad = p.clone();
    p_bad.typed_result.as_mut().unwrap().output = TypedValue::Object(
        [("gate".to_string(), TypedValue::String("Bad Name".into()))]
            .into_iter()
            .collect(),
    );
    let lookup = |id: &String| (id == "p").then(|| p_bad.clone());
    let snap = resolve_inputs(&bad, bad.contract.as_ref().unwrap(), None, 5, &lookup);
    let f = snap.failure.clone().expect("input failure");
    assert_eq!(f.stage, FailureStage::Input);
    assert_eq!(f.location.as_deref(), Some("/input_mapping/barrier"));
    assert!(f.message.contains("barrier name"), "{}", f.message);
    bad.input_snapshot = Some(snap);
    assert_eq!(bad.barrier_name(), None);
}
