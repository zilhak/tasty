//! 전이 조건과 선택 경로 합류. 결과는 실행 경로와 같은 `complete` 로 보고한다.

use std::sync::atomic::AtomicU64;

use serde_json::{Value, json};
use tasty_memory::MemoryStore;
use tempfile::TempDir;

use super::binding::resolve_inputs;
use super::contract::FailureStage;
use super::*;
use crate::AgentError;

fn fresh() -> (TempDir, MemoryStore, AtomicU64) {
    let td = tempfile::tempdir().expect("tempdir");
    let mem = MemoryStore::open(&td.path().join("mem.db")).expect("mem");
    (td, mem, AtomicU64::new(0))
}

fn custom() -> Value {
    json!({"kind": "custom", "ipc_method": "system.ping", "params": {}})
}

fn submit(store: &mut TaskStore, graph: Value) -> Result<Vec<Task>, AgentError> {
    let spec: TaskGraphSpec = serde_json::from_value(graph).expect("graph spec");
    store.submit_graph(1, spec, 0).map(|(_, tasks)| tasks)
}

fn get(store: &TaskStore, id: &str) -> Task {
    store.get(1, &id.to_string()).unwrap().expect("task")
}

fn state(store: &TaskStore, id: &str) -> TaskState {
    get(store, id).state
}

fn skip(store: &TaskStore, id: &str) -> Option<SkipReason> {
    get(store, id).skip
}

fn not_selected(store: &TaskStore, id: &str) -> bool {
    state(store, id) == TaskState::Skipped && skip(store, id) == Some(SkipReason::BranchNotSelected)
}

/// Ready 인 task 를 실행해 출력과 함께 성공으로 보고한다.
fn run(store: &mut TaskStore, id: &str, output: Value) -> Task {
    let id = id.to_string();
    store.set_state(1, &id, TaskState::Running, 1).unwrap();
    let result = TaskResult {
        exit_code: None,
        output: Some(output),
        error: None,
    };
    store
        .complete(1, &id, Completion::succeeded(None, result), 2)
        .expect("complete")
        .task
}

fn run_fail(store: &mut TaskStore, id: &str) {
    let id = id.to_string();
    store.set_state(1, &id, TaskState::Running, 1).unwrap();
    store
        .complete(1, &id, Completion::failed(None, "boom".into()), 2)
        .expect("complete");
}

fn contract_failure(e: AgentError) -> contract::TaskFailure {
    match e {
        AgentError::TypeContract(f) => *f,
        other => panic!("expected a contract error, got {other:?}"),
    }
}

fn review_types() -> Value {
    json!({"ReviewResult": {"type": "object", "fields": {
        "verdict": {"type": "enum", "values": ["pass", "revise", "review"]},
        "confidence": {"type": "float64", "min": 0, "max": 1}}}})
}

/// review 의 판정에 따라 ship·fix·human 중 하나를 고른다.
fn review_graph(transitions: Value) -> Value {
    json!({"contract_version": 2, "types": review_types(), "tasks": [
        {"id": "review", "command": custom(), "output_schema": {"ref": "ReviewResult"},
         "transitions": transitions},
        {"id": "ship", "command": custom()},
        {"id": "fix", "command": custom()},
        {"id": "human", "command": custom()}]})
}

fn verdict_is(v: &str) -> Value {
    json!({"compare": {"path": "/verdict", "op": "eq", "value": v}})
}

fn three_way() -> Value {
    json!({"cases": [
        {"when": verdict_is("pass"), "to": ["ship"]},
        {"when": {"in": {"path": "/verdict", "values": ["revise"]}}, "to": ["fix"]}],
        "otherwise": ["human"]})
}

#[test]
fn each_verdict_runs_only_its_own_branch() {
    for (verdict, chosen) in [("pass", "ship"), ("revise", "fix"), ("review", "human")] {
        let (_td, mut mem, seq) = fresh();
        let mut store = TaskStore::new(&mut mem, "_host", &seq);
        submit(&mut store, review_graph(three_way())).expect("submit");
        for t in ["ship", "fix", "human"] {
            assert_eq!(state(&store, t), TaskState::Waiting, "{t} before review");
        }
        let review = run(
            &mut store,
            "review",
            json!({"verdict": verdict, "confidence": 0.95}),
        );
        assert_eq!(review.state, TaskState::Succeeded);
        let route = review.route.expect("route");
        assert_eq!(route.selected, vec![chosen.to_string()]);
        assert_eq!(route.attempt_id.as_deref(), Some("review#1"));
        assert_eq!(route.otherwise, verdict == "review");
        for t in ["ship", "fix", "human"] {
            if t == chosen {
                assert_eq!(state(&store, t), TaskState::Ready, "{verdict}: {t}");
            } else {
                assert!(
                    not_selected(&store, t),
                    "{verdict}: {t} {:?}",
                    get(&store, t)
                );
            }
        }
    }
}

#[test]
fn exclusive_cases_that_both_hold_fail_the_route_and_hide_no_branch() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    // 제출 시 겹침을 알 수 없는 두 조건(서로 다른 위치).
    submit(
        &mut store,
        review_graph(json!({"cases": [
            {"when": verdict_is("pass"), "to": ["ship"]},
            {"when": {"compare": {"path": "/confidence", "op": "ge", "value": 0.5}}, "to": ["fix"]}],
            "no_match": "finish"})),
    )
    .expect("submit");
    let review = run(
        &mut store,
        "review",
        json!({"verdict": "pass", "confidence": 0.9}),
    );
    let TaskState::Failed { error } = &review.state else {
        panic!("route failure expected, got {:?}", review.state);
    };
    assert!(error.contains("exclusive"), "{error}");
    let typed = review.typed_result.expect("typed");
    assert_eq!(typed.error.expect("error").stage, FailureStage::Route);
    // 출력은 진단용으로 남는다.
    assert!(typed.has_output);
    assert!(review.route.is_none());
    for t in ["ship", "fix"] {
        assert_eq!(state(&store, t), TaskState::Skipped);
        assert_eq!(
            skip(&store, t),
            Some(SkipReason::UpstreamUnavailable {
                source: "review".into(),
                source_state: "failed".into()
            })
        );
    }
}

#[test]
fn exclusive_cases_known_to_overlap_are_refused_at_submission() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    for transitions in [
        json!({"cases": [{"when": verdict_is("pass"), "to": ["ship"]},
                         {"when": verdict_is("pass"), "to": ["fix"]}], "no_match": "finish"}),
        json!({"cases": [{"when": {"in": {"path": "/verdict", "values": ["pass", "review"]}}, "to": ["ship"]},
                         {"when": verdict_is("review"), "to": ["fix"]}], "no_match": "finish"}),
    ] {
        let f = contract_failure(submit(&mut store, review_graph(transitions)).unwrap_err());
        assert!(f.message.contains("can both hold"), "{}", f.message);
        assert_eq!(
            f.location.as_deref(),
            Some("/tasks/0/transitions/cases/1/when")
        );
    }
    assert!(store.list(1).unwrap().is_empty());
}

#[test]
fn all_matches_selects_every_true_case_and_the_join_waits_for_both() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let mut graph = review_graph(json!({"mode": "all_matches", "cases": [
        {"when": verdict_is("pass"), "to": ["ship"]},
        {"when": {"compare": {"path": "/confidence", "op": "ge", "value": 0.5}}, "to": ["fix"]}],
        "no_match": "finish"}));
    graph["tasks"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id": "join", "command": custom(), "depends_on": ["ship", "fix"]}));
    submit(&mut store, graph).expect("submit");
    run(
        &mut store,
        "review",
        json!({"verdict": "pass", "confidence": 0.9}),
    );
    assert_eq!(state(&store, "ship"), TaskState::Ready);
    assert_eq!(state(&store, "fix"), TaskState::Ready);
    run(&mut store, "ship", json!({}));
    assert_eq!(
        state(&store, "join"),
        TaskState::Waiting,
        "one of two selected paths"
    );
    run(&mut store, "fix", json!({}));
    assert_eq!(state(&store, "join"), TaskState::Ready);
}

#[test]
fn no_match_finish_selects_nothing_and_the_dag_still_succeeds() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    submit(
        &mut store,
        review_graph(
            json!({"cases": [{"when": verdict_is("pass"), "to": ["ship"]}],
                            "no_match": "finish"}),
        ),
    )
    .expect("submit");
    let review = run(
        &mut store,
        "review",
        json!({"verdict": "revise", "confidence": 0.2}),
    );
    assert_eq!(review.state, TaskState::Succeeded);
    assert!(review.route.as_ref().unwrap().selected.is_empty());
    assert!(not_selected(&store, "ship"));
    // fix·human 은 전이 대상이 아니라 그대로 실행 가능하다.
    run(&mut store, "fix", json!({}));
    run(&mut store, "human", json!({}));
    let dags = group_tasks_into_dags(&store.list(1).unwrap());
    assert_eq!(dags.len(), 1);
    assert_eq!(dags[0].state_counts.not_selected, 1);
    assert_eq!(dags[0].rollup_state, "succeeded");
    // 흐름의 시작은 경로를 고르는 쪽이다. 전이 대상이 아닌 fix·human 도 시작점이다.
    assert_eq!(
        dags[0].root_task_ids,
        vec!["fix".to_string(), "human".to_string(), "review".to_string()]
    );
}

/// a 가 b 또는 c 를 고른다. c 아래로 c2 → c3 가 이어지고 d 는 b 와 c3 에서 합류한다.
fn diamond(mode: &str) -> Value {
    json!({"contract_version": 2, "types": review_types(), "tasks": [
        {"id": "a", "command": custom(), "output_schema": {"ref": "ReviewResult"},
         "transitions": {"mode": mode, "cases": [
            {"when": verdict_is("pass"), "to": ["b"]},
            {"when": {"compare": {"path": "/confidence", "op": "lt", "value": 0.5}}, "to": ["c"]}],
            "no_match": "finish"}},
        {"id": "b", "command": custom()},
        {"id": "c", "command": custom()},
        {"id": "c2", "command": custom(), "depends_on": ["c"]},
        {"id": "c3", "command": custom(), "depends_on": ["c2"]},
        {"id": "d", "command": custom(), "depends_on": ["b", "c3"]}]})
}

#[test]
fn a_join_after_an_unselected_multistage_branch_waits_only_for_the_selected_one() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    submit(&mut store, diamond("exclusive")).expect("submit");
    run(
        &mut store,
        "a",
        json!({"verdict": "pass", "confidence": 0.9}),
    );
    for t in ["c", "c2", "c3"] {
        assert!(not_selected(&store, t), "{t}: {:?}", get(&store, t));
    }
    assert_eq!(state(&store, "b"), TaskState::Ready);
    store
        .set_state(1, &"b".into(), TaskState::Running, 3)
        .unwrap();
    assert_eq!(state(&store, "d"), TaskState::Waiting, "b still running");
    store
        .complete(
            1,
            &"b".into(),
            Completion::succeeded(
                None,
                TaskResult {
                    exit_code: None,
                    output: Some(json!({})),
                    error: None,
                },
            ),
            4,
        )
        .unwrap();
    assert_eq!(state(&store, "d"), TaskState::Ready);
}

#[test]
fn a_selected_branch_that_fails_is_not_hidden_by_the_join() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    submit(&mut store, diamond("exclusive")).expect("submit");
    run(
        &mut store,
        "a",
        json!({"verdict": "pass", "confidence": 0.9}),
    );
    run_fail(&mut store, "b");
    assert_eq!(state(&store, "d"), TaskState::Skipped);
    assert_eq!(
        skip(&store, "d"),
        Some(SkipReason::UpstreamUnavailable {
            source: "b".into(),
            source_state: "failed".into()
        })
    );
    // 선택된 갈래의 실패는 합류에서 가려지지 않는다. 끝 d 가 건너뛰어졌으니 앞선 a 가
    // 성공했어도 끝까지 성공한 갈래가 없어 실패다.
    let dags = group_tasks_into_dags(&store.list(1).unwrap());
    assert_eq!(dags[0].state_counts.succeeded_ends, 0);
    assert_eq!(dags[0].rollup_state, "failed");
}

#[test]
fn two_selected_branches_join_only_after_both_finish() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    submit(&mut store, diamond("all_matches")).expect("submit");
    // pass 이면서 confidence < 0.5 → 두 경로 모두 선택.
    run(
        &mut store,
        "a",
        json!({"verdict": "pass", "confidence": 0.1}),
    );
    assert_eq!(state(&store, "b"), TaskState::Ready);
    assert_eq!(state(&store, "c"), TaskState::Ready);
    run(&mut store, "b", json!({}));
    assert_eq!(state(&store, "d"), TaskState::Waiting);
    run(&mut store, "c", json!({}));
    run(&mut store, "c2", json!({}));
    assert_eq!(state(&store, "d"), TaskState::Waiting);
    run(&mut store, "c3", json!({}));
    assert_eq!(state(&store, "d"), TaskState::Ready);
}

#[test]
fn a_task_with_no_selected_incoming_path_is_not_selected_either() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    submit(&mut store, diamond("exclusive")).expect("submit");
    // 둘 다 아니다: 아무 경로도 고르지 않는다.
    run(
        &mut store,
        "a",
        json!({"verdict": "revise", "confidence": 0.9}),
    );
    for t in ["b", "c", "c2", "c3", "d"] {
        assert!(not_selected(&store, t), "{t}: {:?}", get(&store, t));
    }
    let dags = group_tasks_into_dags(&store.list(1).unwrap());
    assert_eq!(dags[0].rollup_state, "succeeded");
}

/// 선택 엣지가 확정되기 전에 다른 선행이 먼저 끝나도 합류하지 않는다. 순서를 바꿔 두 번 본다.
#[test]
fn a_target_waits_for_its_route_even_when_its_other_dependency_finishes_first() {
    for route_first in [false, true] {
        let (_td, mut mem, seq) = fresh();
        let mut store = TaskStore::new(&mut mem, "_host", &seq);
        let mut graph = review_graph(three_way());
        graph["tasks"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id": "prep", "command": custom()}));
        graph["tasks"][1]["depends_on"] = json!(["prep"]);
        submit(&mut store, graph).expect("submit");
        if route_first {
            run(
                &mut store,
                "review",
                json!({"verdict": "pass", "confidence": 1}),
            );
            assert_eq!(state(&store, "ship"), TaskState::Waiting, "prep pending");
            run(&mut store, "prep", json!({}));
        } else {
            run(&mut store, "prep", json!({}));
            assert_eq!(state(&store, "ship"), TaskState::Waiting, "route pending");
            run(
                &mut store,
                "review",
                json!({"verdict": "pass", "confidence": 1}),
            );
        }
        assert_eq!(
            state(&store, "ship"),
            TaskState::Ready,
            "route_first={route_first}"
        );
    }
}

fn data_graph(binding: Value, field: Value) -> Value {
    let mut g = diamond("exclusive");
    g["tasks"][1]["output_schema"] = json!({"type": "object", "fields": {"v": {"type": "int64"}}});
    g["tasks"][2]["output_schema"] = json!({"type": "object", "fields": {"v": {"type": "int64"}}});
    g["tasks"][5] = json!({"id": "d", "command": custom(), "depends_on": ["b", "c3"],
        "input_schema": {"type": "object", "fields": {"v": field}},
        "bindings": {"v": binding}});
    g
}

#[test]
fn a_required_input_from_a_branch_that_may_not_run_is_refused() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let f = contract_failure(
        submit(
            &mut store,
            data_graph(
                json!({"from_task": "b", "pointer": "/v"}),
                json!({"type": "int64"}),
            ),
        )
        .unwrap_err(),
    );
    assert!(f.message.contains("one_of"), "{}", f.message);
    assert_eq!(f.location.as_deref(), Some("/tasks/5/bindings/v"));
    assert!(store.list(1).unwrap().is_empty());
}

#[test]
fn a_one_of_binding_takes_the_value_of_whichever_branch_ran() {
    for (verdict, confidence, ran, value) in [("pass", 0.9, "b", 7), ("revise", 0.1, "c", 9)] {
        let (_td, mut mem, seq) = fresh();
        let mut store = TaskStore::new(&mut mem, "_host", &seq);
        submit(
            &mut store,
            data_graph(
                json!({"one_of": [{"from_task": "b", "pointer": "/v"},
                                  {"from_task": "c", "pointer": "/v"}]}),
                json!({"type": "int64"}),
            ),
        )
        .expect("submit");
        run(
            &mut store,
            "a",
            json!({"verdict": verdict, "confidence": confidence}),
        );
        run(&mut store, ran, json!({"v": value}));
        if ran == "c" {
            run(&mut store, "c2", json!({}));
            run(&mut store, "c3", json!({}));
        }
        let d = get(&store, "d");
        assert_eq!(d.state, TaskState::Ready, "{verdict}");
        let lookup = |id: &TaskId| store.get(1, id).unwrap();
        let snap = resolve_inputs(&d, d.contract.as_ref().unwrap(), None, 5, &lookup);
        assert!(snap.failure.is_none(), "{:?}", snap.failure);
        assert_eq!(snap.value.to_internal(), json!({"v": value}));
        assert_eq!(snap.sources.len(), 1);
        assert_eq!(snap.sources[0].from_task, ran);
        assert_eq!(
            snap.sources[0].producer_attempt.as_deref(),
            Some(format!("{ran}#1").as_str())
        );
    }
}

#[test]
fn an_optional_input_from_an_unselected_branch_is_left_out() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    submit(
        &mut store,
        data_graph(
            json!({"from_task": "c", "pointer": "/v"}),
            json!({"type": "int64", "default": 0}),
        ),
    )
    .expect("submit");
    run(
        &mut store,
        "a",
        json!({"verdict": "pass", "confidence": 0.9}),
    );
    run(&mut store, "b", json!({"v": 7}));
    let d = get(&store, "d");
    assert_eq!(d.state, TaskState::Ready);
    let lookup = |id: &TaskId| store.get(1, id).unwrap();
    let snap = resolve_inputs(&d, d.contract.as_ref().unwrap(), None, 5, &lookup);
    assert!(snap.failure.is_none(), "{:?}", snap.failure);
    assert_eq!(snap.value.to_internal(), json!({"v": 0}));
}

#[test]
fn a_chain_inside_one_branch_may_read_its_own_producer() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let mut g = diamond("exclusive");
    g["tasks"][2]["output_schema"] = json!({"type": "object", "fields": {"v": {"type": "int64"}}});
    g["tasks"][3] = json!({"id": "c2", "command": custom(),
        "input_schema": {"type": "object", "fields": {"v": {"type": "int64"}}},
        "bindings": {"v": {"from_task": "c", "pointer": "/v"}}});
    submit(&mut store, g).expect("a consumer reachable only through c may require c");
    run(
        &mut store,
        "a",
        json!({"verdict": "revise", "confidence": 0.1}),
    );
    run(&mut store, "c", json!({"v": 3}));
    assert_eq!(state(&store, "c2"), TaskState::Ready);
}

/// main 이 실패하면 fallback 이 돈다. main 의 전이 대상은 경로가 정해지지 않았으므로 실패
/// 전파로 끝나고, 값은 one_of 로 실제로 성공한 쪽에서 받는다.
#[test]
fn a_failed_main_routes_nothing_and_its_fallback_supplies_the_value() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let out = json!({"type": "object", "fields": {"v": {"type": "int64"}}});
    submit(
        &mut store,
        json!({"contract_version": 2, "tasks": [
            {"id": "main", "command": custom(), "output_schema": out,
             "on_failure": {"kind": "fallback", "task": "recover"},
             "transitions": {"cases": [{"when": {"compare": {"path": "/v", "op": "gt", "value": 0}},
                                        "to": ["next"]}], "no_match": "finish"}},
            {"id": "recover", "command": custom(), "output_schema": out},
            {"id": "next", "command": custom()},
            {"id": "use", "command": custom(), "depends_on": ["main"],
             "input_schema": {"type": "object", "fields": {"v": {"type": "int64"}}},
             "bindings": {"v": {"one_of": [{"from_task": "main", "pointer": "/v"},
                                           {"from_task": "recover", "pointer": "/v"}]}}}]}),
    )
    .expect("submit");
    run_fail(&mut store, "main");
    assert_eq!(
        skip(&store, "next"),
        Some(SkipReason::UpstreamUnavailable {
            source: "main".into(),
            source_state: "failed".into()
        })
    );
    assert_eq!(state(&store, "recover"), TaskState::Ready);
    run(&mut store, "recover", json!({"v": 5}));
    let u = get(&store, "use");
    assert_eq!(u.state, TaskState::Ready);
    let lookup = |id: &TaskId| store.get(1, id).unwrap();
    let snap = resolve_inputs(&u, u.contract.as_ref().unwrap(), None, 5, &lookup);
    assert_eq!(snap.value.to_internal(), json!({"v": 5}));
    assert_eq!(snap.sources[0].from_task, "recover");
}

#[test]
fn a_fallback_whose_main_was_not_selected_is_not_selected_either() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let mut g = review_graph(
        json!({"cases": [{"when": verdict_is("pass"), "to": ["ship"]}],
                                    "no_match": "finish"}),
    );
    g["tasks"][1]["on_failure"] = json!({"kind": "fallback", "task": "fix"});
    submit(&mut store, g).expect("submit");
    run(
        &mut store,
        "review",
        json!({"verdict": "revise", "confidence": 0.5}),
    );
    assert!(not_selected(&store, "ship"));
    assert!(not_selected(&store, "fix"), "{:?}", get(&store, "fix"));
}

#[test]
fn transitions_are_checked_against_the_declared_output_and_targets() {
    let cases: Vec<(Value, &str)> = vec![
        (
            json!({"cases": [{"when": {"compare": {"path": "/confidence", "op": "eq", "value": 0.5}},
                              "to": ["ship"]}], "no_match": "finish"}),
            "lt/le/gt/ge",
        ),
        (
            json!({"cases": [{"when": {"compare": {"path": "/verdict", "op": "gt", "value": "pass"}},
                              "to": ["ship"]}], "no_match": "finish"}),
            "eq/ne only",
        ),
        (
            json!({"cases": [{"when": verdict_is("maybe"), "to": ["ship"]}], "no_match": "finish"}),
            "unknown enum value",
        ),
        (
            json!({"cases": [{"when": {"compare": {"path": "/nope", "op": "eq", "value": 1}},
                              "to": ["ship"]}], "no_match": "finish"}),
            "missing field",
        ),
        (
            json!({"cases": [{"when": {"compare": {"path": "", "op": "eq", "value": 1}},
                              "to": ["ship"]}], "no_match": "finish"}),
            "a condition compares",
        ),
        (
            json!({"cases": [{"when": verdict_is("pass"), "to": ["elsewhere"]}], "no_match": "finish"}),
            "not a task of this graph",
        ),
        (
            json!({"cases": [{"when": verdict_is("pass"), "to": ["review"]}], "no_match": "finish"}),
            "cannot select itself",
        ),
        (
            json!({"cases": [{"when": verdict_is("pass"), "to": ["ship"]}]}),
            "no_match",
        ),
        (
            json!({"cases": [{"when": verdict_is("pass"), "to": ["ship"]}],
                   "otherwise": ["fix"], "no_match": "finish"}),
            "not both",
        ),
        (
            json!({"cases": [], "no_match": "finish"}),
            "at least one case",
        ),
        (
            json!({"cases": [{"when": {"all": []}, "to": ["ship"]}], "no_match": "finish"}),
            "at least one condition",
        ),
    ];
    for (transitions, expect) in cases {
        let (_td, mut mem, seq) = fresh();
        let mut store = TaskStore::new(&mut mem, "_host", &seq);
        let f =
            contract_failure(submit(&mut store, review_graph(transitions.clone())).unwrap_err());
        assert!(
            f.message.contains(expect),
            "{transitions}: expected '{expect}' in '{}'",
            f.message
        );
        assert!(
            f.location
                .as_deref()
                .unwrap_or("")
                .starts_with("/tasks/0/transitions"),
            "{:?}",
            f.location
        );
        assert!(store.list(1).unwrap().is_empty());
    }
}

#[test]
fn a_target_cannot_revive_itself_through_its_failure_policy() {
    // continue_downstream 은 경로가 정해지지 않았을 때 대상을 실행시킨다.
    {
        let (_td, mut mem, seq) = fresh();
        let mut store = TaskStore::new(&mut mem, "_host", &seq);
        let mut g = review_graph(three_way());
        g["tasks"][1]["on_failure"] = json!({"kind": "continue_downstream"});
        let f = contract_failure(submit(&mut store, g).unwrap_err());
        assert!(f.message.contains("continue_downstream"), "{}", f.message);
    }
    // fallback 은 main 이 실패할 때만 돈다. 전이 대상이 될 수 없다.
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let mut g = review_graph(three_way());
    g["tasks"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id": "main", "command": custom(), "on_failure": {"kind": "fallback", "task": "fix"}}));
    let f = contract_failure(submit(&mut store, g).unwrap_err());
    assert!(f.message.contains("fallback of main"), "{}", f.message);
}

#[test]
fn a_condition_that_reads_a_missing_optional_value_fails_the_route() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    submit(
        &mut store,
        json!({"contract_version": 2, "tasks": [
            {"id": "p", "command": custom(), "output_schema": {"type": "object", "fields": {
                "score": {"type": "int64", "optional": true}}},
             "transitions": {"cases": [{"when": {"compare": {"path": "/score", "op": "ge", "value": 3}},
                                        "to": ["q"]}], "no_match": "finish"}},
            {"id": "q", "command": custom()}]}),
    )
    .expect("submit");
    let p = run(&mut store, "p", json!({}));
    let TaskState::Failed { error } = &p.state else {
        panic!("{:?}", p.state)
    };
    assert!(error.contains("no value at '/score'"), "{error}");
    assert_eq!(
        p.typed_result.unwrap().error.unwrap().stage,
        FailureStage::Route
    );
    assert_eq!(state(&store, "q"), TaskState::Skipped);
}

#[test]
fn a_reduce_all_condition_reads_an_input_record_with_that_input_type() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let count = json!({"type": "object", "fields": {"n": {"type": "int64"}}});
    let graph = |op: &str, input: &str| {
        json!({"contract_version": 2, "tasks": [
            {"id": "x", "command": custom(), "output_schema": count},
            {"id": "y", "command": custom()},
            {"id": "all", "command": {"kind": "reduce", "inputs": ["x", "y"], "strategy": {"kind": "all"}},
             "transitions": {"cases": [{"when": {"compare": {"input": input, "path": "/n", "op": op,
                                                              "value": "9007199254740993"}},
                                        "to": ["big"]}], "no_match": "finish"}},
            {"id": "big", "command": custom()}]})
    };
    // input 이 아닌 task, 그 입력의 타입에 없는 위치는 제출 시 거절한다.
    let f = contract_failure(submit(&mut store, graph("ge", "big")).unwrap_err());
    assert!(f.message.contains("not an input"), "{}", f.message);
    // y 는 출력 타입을 선언하지 않아 json 이다. 조건은 타입을 아는 값만 읽는다.
    let f = contract_failure(submit(&mut store, graph("ge", "y")).unwrap_err());
    assert!(f.message.contains("is json"), "{}", f.message);

    submit(&mut store, graph("ge", "x")).expect("submit");
    run(&mut store, "x", json!({"n": "9007199254740993"}));
    run(&mut store, "y", json!({}));
    let all = get(&store, "all");
    assert_eq!(all.state, TaskState::Ready);
    // reduce 결과를 실행기가 만든 것처럼 기록한다.
    let records = crate::reduce_typed(
        &ReducerStrategy::All,
        &[
            crate::TypedReducerInput::from_task(&get(&store, "x")),
            crate::TypedReducerInput::from_task(&get(&store, "y")),
        ],
        contract::MergeConflict::Error,
        |_, _| unreachable!("all does not run a program"),
    );
    let all = run(&mut store, "all", records.expect("reduce"));
    assert_eq!(all.state, TaskState::Succeeded, "{:?}", all.typed_result);
    assert_eq!(all.route.unwrap().selected, vec!["big".to_string()]);
    assert_eq!(state(&store, "big"), TaskState::Ready);
}

#[test]
fn reset_downstream_is_refused_for_typed_tasks_and_a_new_attempt_clears_the_route() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    submit(&mut store, review_graph(three_way())).expect("submit");
    let review = "review".to_string();
    run_fail(&mut store, "review");
    let e = store.retry(1, &review, true, 3).unwrap_err();
    assert!(matches!(e, AgentError::InvalidArgument(ref m) if m.contains("reset_downstream")));
    store.retry(1, &review, false, 3).expect("retry");
    // 실패를 이미 받은 대상은 그대로다.
    for t in ["ship", "fix", "human"] {
        assert!(
            matches!(
                skip(&store, t),
                Some(SkipReason::UpstreamUnavailable { .. })
            ),
            "{t}"
        );
    }
    let again = run(
        &mut store,
        "review",
        json!({"verdict": "pass", "confidence": 1}),
    );
    assert_eq!(again.route.unwrap().attempt_id.as_deref(), Some("review#2"));
    assert_eq!(state(&store, "ship"), TaskState::Skipped);
}

#[test]
fn retrying_a_task_whose_branch_was_not_selected_is_refused() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    submit(&mut store, review_graph(three_way())).expect("submit");
    run(
        &mut store,
        "review",
        json!({"verdict": "revise", "confidence": 1}),
    );
    assert!(not_selected(&store, "ship"));
    let e = store.retry(1, &"ship".to_string(), false, 3).unwrap_err();
    assert!(
        matches!(e, AgentError::InvalidArgument(ref m) if m.contains("not selected")),
        "{e:?}"
    );
    // 거절은 레코드를 바꾸지 않는다.
    assert!(not_selected(&store, "ship"));
    assert_eq!(get(&store, "ship").finished_at, Some(2));
}

#[test]
fn transitions_are_only_accepted_through_a_graph_submission() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let contract: TaskContract = serde_json::from_value(json!({
        "contract_version": 2,
        "transitions": {"cases": [{"when": {"compare": {"path": "", "op": "eq", "value": 1}}, "to": ["x"]}],
                        "no_match": "finish"}}))
    .unwrap();
    let e = store
        .create_typed(
            TaskCreateOpts {
                workspace_id: 1,
                name: "t".into(),
                command: serde_json::from_value(custom()).unwrap(),
                depends_on: vec![],
                on_failure: OnFailure::Abort,
                metadata: Value::Null,
                now_ms: 0,
            },
            contract,
        )
        .unwrap_err();
    assert!(e.to_string().contains("graph submission"), "{e}");
}

#[test]
fn a_stored_route_and_skip_reason_survive_a_reload() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    submit(&mut store, review_graph(three_way())).expect("submit");
    run(
        &mut store,
        "review",
        json!({"verdict": "revise", "confidence": 1}),
    );
    let listed = store.list(1).unwrap();
    let review = listed.iter().find(|t| t.id == "review").unwrap();
    assert_eq!(
        review.route.as_ref().unwrap().selected,
        vec!["fix".to_string()]
    );
    let ship = listed.iter().find(|t| t.id == "ship").unwrap();
    assert_eq!(ship.skip, Some(SkipReason::BranchNotSelected));
    let wire = serde_json::to_value(ship).unwrap();
    assert_eq!(wire["skip"], json!({"reason": "branch_not_selected"}));
}

/// 전이와 fallback 이 섞인 그래프: 선택된 a 가 실패하고 fallback 이 대신 성공하면, 선택되지 않은
/// b 와 함께 DAG 는 성공이다.
#[test]
fn a_selected_branch_recovered_by_its_fallback_rolls_up_as_succeeded() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    submit(
        &mut store,
        json!({"contract_version": 2, "types": review_types(), "tasks": [
            {"id": "p", "command": custom(), "output_schema": {"ref": "ReviewResult"},
             "transitions": {"cases": [{"when": verdict_is("pass"), "to": ["a"]}],
                             "otherwise": ["b"]}},
            {"id": "a", "command": custom(), "on_failure": {"kind": "fallback", "task": "fb"}},
            {"id": "fb", "command": custom()},
            {"id": "b", "command": custom()},
            {"id": "after", "command": custom(), "depends_on": ["a"]}]}),
    )
    .expect("submit");
    run(&mut store, "p", json!({"verdict": "pass", "confidence": 1}));
    run_fail(&mut store, "a");
    run(&mut store, "fb", json!({}));
    run(&mut store, "after", json!({}));
    let d = &group_tasks_into_dags(&store.list(1).unwrap())[0];
    assert_eq!(d.state_counts.failed, 1);
    assert_eq!(d.state_counts.recovered, 1);
    assert_eq!(d.state_counts.not_selected, 1);
    assert_eq!(d.rollup_state, "succeeded");
}

/// 경로를 고르지 않고 끝난 성공 task 는 끝까지 성공한 갈래다. 선택되지 않아 건너뛴 하류는
/// 하류로 보지 않으므로, 무관한 갈래가 실패해도 DAG 는 실패가 아니라 부분 오류다.
#[test]
fn a_success_whose_downstream_was_not_selected_counts_as_a_succeeded_end() {
    let finish = json!({"cases": [{"when": verdict_is("pass"), "to": ["ship"]}],
                        "no_match": "finish"});

    // 반례 3: review 가 아무 경로도 고르지 않고 끝나고, 독립 z 가 실패한다.
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    submit(
        &mut store,
        json!({"contract_version": 2, "types": review_types(), "tasks": [
            {"id": "review", "command": custom(), "output_schema": {"ref": "ReviewResult"},
             "transitions": finish},
            {"id": "ship", "command": custom()},
            {"id": "z", "command": custom()}]}),
    )
    .expect("submit");
    run(
        &mut store,
        "review",
        json!({"verdict": "revise", "confidence": 0.2}),
    );
    run_fail(&mut store, "z");
    assert!(not_selected(&store, "ship"));
    let dags = group_tasks_into_dags(&store.list(1).unwrap());
    assert_eq!(dags[0].state_counts.succeeded_ends, 1);
    assert_eq!(dags[0].rollup_state, "partially_failed");

    // 반례 4: x 가 성공했고 그 하류 y 는 r 의 전이 대상인데 선택되지 않았다. 독립 z 가 실패한다.
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    submit(
        &mut store,
        json!({"contract_version": 2, "types": review_types(), "tasks": [
            {"id": "r", "command": custom(), "output_schema": {"ref": "ReviewResult"},
             "transitions": {"cases": [{"when": verdict_is("pass"), "to": ["y"]}],
                             "no_match": "finish"}},
            {"id": "x", "command": custom()},
            {"id": "y", "command": custom(), "depends_on": ["x"]},
            {"id": "z", "command": custom()}]}),
    )
    .expect("submit");
    run(&mut store, "x", json!({}));
    run(
        &mut store,
        "r",
        json!({"verdict": "revise", "confidence": 0.2}),
    );
    run_fail(&mut store, "z");
    assert!(not_selected(&store, "y"));
    let dags = group_tasks_into_dags(&store.list(1).unwrap());
    // 끝은 r(하류 y 미선택)과 x(하류 y 미선택)다.
    assert_eq!(dags[0].state_counts.succeeded_ends, 2);
    assert_eq!(dags[0].rollup_state, "partially_failed");
}
