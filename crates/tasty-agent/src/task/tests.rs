//! `task_tests` 단위 테스트.

use super::*;
use crate::AgentError;
use std::sync::atomic::AtomicU64;
use tasty_memory::MemoryStore;
use tempfile::TempDir;

fn fresh_store() -> (TempDir, MemoryStore, AtomicU64) {
    let td = tempfile::tempdir().expect("tempdir");
    let mem = MemoryStore::open(&td.path().join("mem.db")).expect("mem");
    (td, mem, AtomicU64::new(0))
}

fn run_cmd() -> TaskCommand {
    TaskCommand::Run {
        command: vec!["echo".into(), "hi".into()],
        workspace_id: 1,
        cwd: None,
    }
}

fn reduce_cmd(inputs: Vec<TaskId>) -> TaskCommand {
    TaskCommand::Reduce {
        inputs,
        strategy: ReducerStrategy::All,
    }
}

#[test]
fn create_single_task_starts_ready() {
    let (_td, mut mem, seq) = fresh_store();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let t = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "a".to_string(),
            command: run_cmd(),
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1000,
        })
        .unwrap();
    assert_eq!(t.state, TaskState::Ready);
    assert_eq!(t.workspace_id, 1);
}

#[test]
fn linear_dag_transitions_downstream() {
    let (_td, mut mem, seq) = fresh_store();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let a = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "A".to_string(),
            command: run_cmd(),
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1000,
        })
        .unwrap();
    let b = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "B".to_string(),
            command: run_cmd(),
            depends_on: vec![a.id.clone()],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1001,
        })
        .unwrap();
    let c = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "C".to_string(),
            command: run_cmd(),
            depends_on: vec![b.id.clone()],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1002,
        })
        .unwrap();
    assert_eq!(a.state, TaskState::Ready);
    assert_eq!(b.state, TaskState::Waiting);
    assert_eq!(c.state, TaskState::Waiting);

    store.set_state(1, &a.id, TaskState::Running, 2000).unwrap();
    let (_, cascaded) = store
        .set_state(1, &a.id, TaskState::Succeeded, 3000)
        .unwrap();
    // B should be Ready now
    let b_after = store.get(1, &b.id).unwrap().unwrap();
    assert_eq!(b_after.state, TaskState::Ready);
    // C still Waiting
    let c_after = store.get(1, &c.id).unwrap().unwrap();
    assert_eq!(c_after.state, TaskState::Waiting);
    assert!(cascaded.iter().any(|t| t.id == b.id));
}

#[test]
fn diamond_dag_parallel_then_join() {
    let (_td, mut mem, seq) = fresh_store();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let a = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "A".to_string(),
            command: run_cmd(),
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1000,
        })
        .unwrap();
    let b = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "B".to_string(),
            command: run_cmd(),
            depends_on: vec![a.id.clone()],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1001,
        })
        .unwrap();
    let c = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "C".to_string(),
            command: run_cmd(),
            depends_on: vec![a.id.clone()],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1002,
        })
        .unwrap();
    let d = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "D".to_string(),
            command: run_cmd(),
            depends_on: vec![b.id.clone(), c.id.clone()],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1003,
        })
        .unwrap();

    store.set_state(1, &a.id, TaskState::Running, 2000).unwrap();
    store
        .set_state(1, &a.id, TaskState::Succeeded, 3000)
        .unwrap();
    assert_eq!(
        store.get(1, &b.id).unwrap().unwrap().state,
        TaskState::Ready
    );
    assert_eq!(
        store.get(1, &c.id).unwrap().unwrap().state,
        TaskState::Ready
    );
    assert_eq!(
        store.get(1, &d.id).unwrap().unwrap().state,
        TaskState::Waiting
    );

    // B done
    store.set_state(1, &b.id, TaskState::Running, 4000).unwrap();
    store
        .set_state(1, &b.id, TaskState::Succeeded, 5000)
        .unwrap();
    assert_eq!(
        store.get(1, &d.id).unwrap().unwrap().state,
        TaskState::Waiting
    );

    // C done → D ready
    store.set_state(1, &c.id, TaskState::Running, 6000).unwrap();
    store
        .set_state(1, &c.id, TaskState::Succeeded, 7000)
        .unwrap();
    assert_eq!(
        store.get(1, &d.id).unwrap().unwrap().state,
        TaskState::Ready
    );
}

#[test]
fn abort_propagates_skip() {
    let (_td, mut mem, seq) = fresh_store();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let a = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "A".to_string(),
            command: run_cmd(),
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1000,
        })
        .unwrap();
    let b = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "B".to_string(),
            command: run_cmd(),
            depends_on: vec![a.id.clone()],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1001,
        })
        .unwrap();
    store.set_state(1, &a.id, TaskState::Running, 2000).unwrap();
    store
        .set_state(
            1,
            &a.id,
            TaskState::Failed {
                error: "boom".into(),
            },
            3000,
        )
        .unwrap();
    let b_after = store.get(1, &b.id).unwrap().unwrap();
    assert_eq!(b_after.state, TaskState::Skipped);
}

#[test]
fn continue_downstream_keeps_going() {
    let (_td, mut mem, seq) = fresh_store();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let a = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "A".to_string(),
            command: run_cmd(),
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1000,
        })
        .unwrap();
    let b = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "B".to_string(),
            command: run_cmd(),
            depends_on: vec![a.id.clone()],
            on_failure: OnFailure::ContinueDownstream,
            metadata: serde_json::Value::Null,
            now_ms: 1001,
        })
        .unwrap();
    store.set_state(1, &a.id, TaskState::Running, 2000).unwrap();
    store
        .set_state(1, &a.id, TaskState::Failed { error: "x".into() }, 3000)
        .unwrap();
    let b_after = store.get(1, &b.id).unwrap().unwrap();
    assert_eq!(b_after.state, TaskState::Ready);
}

#[test]
fn cycle_detected() {
    let (_td, mut mem, seq) = fresh_store();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    // 직접 사이클을 만들 수 없으니, 내부 함수로 시도.
    // A -> B (created with dep A) ; 만약 A를 update해서 dep=B 추가하면 cycle.
    let a = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "A".to_string(),
            command: run_cmd(),
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1000,
        })
        .unwrap();
    let b = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "B".to_string(),
            command: run_cmd(),
            depends_on: vec![a.id.clone()],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1001,
        })
        .unwrap();
    // create 시 unknown dep는 거부
    let err = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "X".to_string(),
            command: run_cmd(),
            depends_on: vec!["nonexistent".into()],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1002,
        })
        .unwrap_err();
    assert!(matches!(err, AgentError::UnknownDependency(_)));

    // 영속을 우회해 강제로 cycle 만든 뒤 detect
    let mut a_mut = a.clone();
    a_mut.depends_on = vec![b.id.clone()];
    store.put(&a_mut).unwrap();
    let all = store.list(1).unwrap();
    let graph = TaskGraph::build(&all);
    let err = graph.detect_cycles().unwrap_err();
    assert!(matches!(err, AgentError::DependencyCycle(_)));
}

#[test]
fn cancel_pending_task() {
    let (_td, mut mem, seq) = fresh_store();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let a = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "A".to_string(),
            command: run_cmd(),
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1000,
        })
        .unwrap();
    let (cancelled, _) = store.cancel(1, &a.id, 1500).unwrap();
    assert_eq!(cancelled.state, TaskState::Cancelled);
    assert_eq!(cancelled.finished_at, Some(1500));
}

#[test]
fn retry_resets_state() {
    let (_td, mut mem, seq) = fresh_store();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let a = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "A".to_string(),
            command: run_cmd(),
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1000,
        })
        .unwrap();
    store.set_state(1, &a.id, TaskState::Running, 2000).unwrap();
    store
        .set_state(1, &a.id, TaskState::Failed { error: "e".into() }, 3000)
        .unwrap();
    let retried = store.retry(1, &a.id, false, 4000).unwrap();
    assert_eq!(retried.state, TaskState::Ready);
    assert!(retried.finished_at.is_none());
}

#[test]
fn retry_with_reset_downstream() {
    let (_td, mut mem, seq) = fresh_store();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let a = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "A".to_string(),
            command: run_cmd(),
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1000,
        })
        .unwrap();
    let b = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "B".to_string(),
            command: run_cmd(),
            depends_on: vec![a.id.clone()],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1001,
        })
        .unwrap();
    store.set_state(1, &a.id, TaskState::Running, 2000).unwrap();
    store
        .set_state(1, &a.id, TaskState::Failed { error: "e".into() }, 3000)
        .unwrap();
    // B는 Skipped
    assert_eq!(
        store.get(1, &b.id).unwrap().unwrap().state,
        TaskState::Skipped
    );

    store.retry(1, &a.id, true, 4000).unwrap();
    // B는 Waiting으로 돌아오고 cascade 후 A가 Ready라서 B는 Waiting 유지 (A 미완료)
    let b_after = store.get(1, &b.id).unwrap().unwrap();
    assert_eq!(b_after.state, TaskState::Waiting);
}

#[test]
fn fallback_triggers_when_main_fails() {
    // A -> C (dep). A.on_failure = Fallback{A'}. A 실패 시 A' 가 자동 Ready.
    let (_td, mut mem, seq) = fresh_store();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let a_prime = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "A_prime".to_string(),
            command: run_cmd(),
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1000,
        })
        .unwrap();
    let a = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "A".to_string(),
            command: run_cmd(),
            depends_on: vec![],
            on_failure: OnFailure::Fallback {
                task: Some(a_prime.id.clone()),
                inline: None,
            },
            metadata: serde_json::Value::Null,
            now_ms: 1001,
        })
        .unwrap();
    let c = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "C".to_string(),
            command: run_cmd(),
            depends_on: vec![a.id.clone()],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1002,
        })
        .unwrap();
    // A 실패 시 A_prime 도 이미 Ready 였으므로 변화 없음, C 는 Waiting 유지 (fallback 대기).
    store.set_state(1, &a.id, TaskState::Running, 2000).unwrap();
    store
        .set_state(
            1,
            &a.id,
            TaskState::Failed {
                error: "boom".into(),
            },
            3000,
        )
        .unwrap();
    let c_after = store.get(1, &c.id).unwrap().unwrap();
    assert_eq!(c_after.state, TaskState::Waiting, "C 는 fallback 대기");
    let a_prime_after = store.get(1, &a_prime.id).unwrap().unwrap();
    assert_eq!(a_prime_after.state, TaskState::Ready);
}

#[test]
fn fallback_success_propagates_to_main_downstream() {
    // A.on_failure=Fallback{A'}, C depends on A. A 실패 → A' Succeed → C Ready.
    let (_td, mut mem, seq) = fresh_store();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let a_prime = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "A_prime".to_string(),
            command: run_cmd(),
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1000,
        })
        .unwrap();
    let a = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "A".to_string(),
            command: run_cmd(),
            depends_on: vec![],
            on_failure: OnFailure::Fallback {
                task: Some(a_prime.id.clone()),
                inline: None,
            },
            metadata: serde_json::Value::Null,
            now_ms: 1001,
        })
        .unwrap();
    let c = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "C".to_string(),
            command: run_cmd(),
            depends_on: vec![a.id.clone()],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1002,
        })
        .unwrap();
    store.set_state(1, &a.id, TaskState::Running, 2000).unwrap();
    store
        .set_state(
            1,
            &a.id,
            TaskState::Failed {
                error: "boom".into(),
            },
            3000,
        )
        .unwrap();
    store
        .set_state(1, &a_prime.id, TaskState::Running, 3500)
        .unwrap();
    store
        .set_state(1, &a_prime.id, TaskState::Succeeded, 4000)
        .unwrap();
    let c_after = store.get(1, &c.id).unwrap().unwrap();
    assert_eq!(c_after.state, TaskState::Ready, "fallback 성공으로 C 진행");
}

#[test]
fn fallback_failure_also_skips_main_downstream() {
    // A.on_failure=Fallback{A'}, C depends on A. A 실패 → A' 도 실패 → C Skipped.
    let (_td, mut mem, seq) = fresh_store();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let a_prime = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "A_prime".to_string(),
            command: run_cmd(),
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1000,
        })
        .unwrap();
    let a = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "A".to_string(),
            command: run_cmd(),
            depends_on: vec![],
            on_failure: OnFailure::Fallback {
                task: Some(a_prime.id.clone()),
                inline: None,
            },
            metadata: serde_json::Value::Null,
            now_ms: 1001,
        })
        .unwrap();
    let c = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "C".to_string(),
            command: run_cmd(),
            depends_on: vec![a.id.clone()],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1002,
        })
        .unwrap();
    store.set_state(1, &a.id, TaskState::Running, 2000).unwrap();
    store
        .set_state(
            1,
            &a.id,
            TaskState::Failed {
                error: "boom".into(),
            },
            3000,
        )
        .unwrap();
    store
        .set_state(1, &a_prime.id, TaskState::Running, 3500)
        .unwrap();
    store
        .set_state(
            1,
            &a_prime.id,
            TaskState::Failed {
                error: "boom2".into(),
            },
            4000,
        )
        .unwrap();
    let c_after = store.get(1, &c.id).unwrap().unwrap();
    assert_eq!(
        c_after.state,
        TaskState::Skipped,
        "fallback 도 실패면 C 도 Skip"
    );
}

#[test]
fn list_returns_all_tasks() {
    let (_td, mut mem, seq) = fresh_store();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "A".to_string(),
            command: run_cmd(),
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1000,
        })
        .unwrap();
    store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "B".to_string(),
            command: run_cmd(),
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1001,
        })
        .unwrap();
    store
        .create(TaskCreateOpts {
            workspace_id: 2,
            name: "C".to_string(),
            command: run_cmd(),
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1002,
        })
        .unwrap();
    let ws1 = store.list(1).unwrap();
    assert_eq!(ws1.len(), 2);
    let ws2 = store.list(2).unwrap();
    assert_eq!(ws2.len(), 1);
}

#[test]
fn fallback_inline_materializes_on_failed_transition() {
    // A.on_failure=Fallback{inline:{A_prime}}, C depends on A.
    // A 실패 → A_prime task 자동 생성 (metadata.fallback_of=A.id) + Ready.
    let (_td, mut mem, seq) = fresh_store();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let a = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "A".to_string(),
            command: run_cmd(),
            depends_on: vec![],
            on_failure: OnFailure::Fallback {
                task: None,
                inline: Some(Box::new(InlineFallbackSpec {
                    name: "A_prime".into(),
                    command: run_cmd(),
                    depends_on_override: None,
                    on_failure: OnFailure::Abort,
                    metadata: serde_json::json!({}),
                })),
            },
            metadata: serde_json::Value::Null,
            now_ms: 1000,
        })
        .unwrap();
    let c = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "C".to_string(),
            command: run_cmd(),
            depends_on: vec![a.id.clone()],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1001,
        })
        .unwrap();

    store.set_state(1, &a.id, TaskState::Running, 2000).unwrap();
    store
        .set_state(
            1,
            &a.id,
            TaskState::Failed {
                error: "boom".into(),
            },
            3000,
        )
        .unwrap();

    // A_prime 가 생성되었고 Ready 인지 확인.
    let all = store.list(1).unwrap();
    let a_prime = all
        .iter()
        .find(|t| t.metadata.get("fallback_of").and_then(|v| v.as_str()) == Some(a.id.as_str()))
        .expect("inline fallback materialized");
    assert_eq!(a_prime.name, "A_prime");
    assert_eq!(a_prime.state, TaskState::Ready);
    // C 는 여전히 Waiting — A_prime 의 결과 대기.
    let c_after = store.get(1, &c.id).unwrap().unwrap();
    assert_eq!(c_after.state, TaskState::Waiting);

    // A_prime Succeed → C Ready.
    let a_prime_id = a_prime.id.clone();
    store
        .set_state(1, &a_prime_id, TaskState::Running, 3500)
        .unwrap();
    store
        .set_state(1, &a_prime_id, TaskState::Succeeded, 4000)
        .unwrap();
    let c_after = store.get(1, &c.id).unwrap().unwrap();
    assert_eq!(c_after.state, TaskState::Ready, "fallback 성공 → C Ready");
}

#[test]
fn fallback_inline_idempotent_on_repeated_failed_calls() {
    // 같은 main 의 Failed 분기를 *여러 번* 호출해도 inline fallback task 는 1개만.
    let (_td, mut mem, seq) = fresh_store();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let a = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "A".to_string(),
            command: run_cmd(),
            depends_on: vec![],
            on_failure: OnFailure::Fallback {
                task: None,
                inline: Some(Box::new(InlineFallbackSpec {
                    name: "A_prime".into(),
                    command: run_cmd(),
                    depends_on_override: None,
                    on_failure: OnFailure::Abort,
                    metadata: serde_json::json!({}),
                })),
            },
            metadata: serde_json::Value::Null,
            now_ms: 1000,
        })
        .unwrap();

    store.set_state(1, &a.id, TaskState::Running, 2000).unwrap();
    store
        .set_state(
            1,
            &a.id,
            TaskState::Failed {
                error: "boom".into(),
            },
            3000,
        )
        .unwrap();

    let count_first = store
        .list(1)
        .unwrap()
        .iter()
        .filter(|t| t.metadata.get("fallback_of").and_then(|v| v.as_str()) == Some(a.id.as_str()))
        .count();
    assert_eq!(count_first, 1);

    // retry 후 다시 Failed — fallback 이 중복 생성되면 안 됨.
    // 단 retry 는 task state 를 Waiting → Ready 로 보내므로, Failed 후 retry → Failed
    // 사이클을 시뮬레이션.
    store.retry(1, &a.id, false, 3100).unwrap();
    store.set_state(1, &a.id, TaskState::Running, 3200).unwrap();
    store
        .set_state(
            1,
            &a.id,
            TaskState::Failed {
                error: "boom2".into(),
            },
            3300,
        )
        .unwrap();
    let count_second = store
        .list(1)
        .unwrap()
        .iter()
        .filter(|t| t.metadata.get("fallback_of").and_then(|v| v.as_str()) == Some(a.id.as_str()))
        .count();
    assert_eq!(count_second, 1, "inline fallback must not duplicate");
}

#[test]
fn fallback_validation_rejects_both_task_and_inline() {
    let (_td, mut mem, seq) = fresh_store();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    // 먼저 어떤 task 라도 만들어 task id 확보.
    let helper = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "helper".into(),
            command: run_cmd(),
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1000,
        })
        .unwrap();
    let err = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "bad".into(),
            command: run_cmd(),
            depends_on: vec![],
            on_failure: OnFailure::Fallback {
                task: Some(helper.id),
                inline: Some(Box::new(InlineFallbackSpec {
                    name: "x".into(),
                    command: run_cmd(),
                    depends_on_override: None,
                    on_failure: OnFailure::Abort,
                    metadata: serde_json::json!({}),
                })),
            },
            metadata: serde_json::Value::Null,
            now_ms: 1001,
        })
        .unwrap_err();
    assert!(
        matches!(err, AgentError::InvalidArgument(_)),
        "expected InvalidArgument, got {err:?}"
    );
}

#[test]
fn fallback_validation_rejects_neither_task_nor_inline() {
    let (_td, mut mem, seq) = fresh_store();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let err = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "bad".into(),
            command: run_cmd(),
            depends_on: vec![],
            on_failure: OnFailure::Fallback {
                task: None,
                inline: None,
            },
            metadata: serde_json::Value::Null,
            now_ms: 1000,
        })
        .unwrap_err();
    assert!(
        matches!(err, AgentError::InvalidArgument(_)),
        "expected InvalidArgument, got {err:?}"
    );
}

// ============================================================
// TODO14 회귀: Fallback.task / Reduce.inputs 참조 검증
// ============================================================

#[test]
fn create_rejects_unknown_reduce_input() {
    let (_td, mut mem, seq) = fresh_store();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let err = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "R".into(),
            command: reduce_cmd(vec!["t-does-not-exist".into()]),
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1000,
        })
        .unwrap_err();
    assert!(
        matches!(err, AgentError::UnknownDependency(ref id) if id == "t-does-not-exist"),
        "expected UnknownDependency, got {err:?}"
    );
}

#[test]
fn create_rejects_unknown_fallback_task() {
    let (_td, mut mem, seq) = fresh_store();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let err = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "A".into(),
            command: run_cmd(),
            depends_on: vec![],
            on_failure: OnFailure::Fallback {
                task: Some("t-does-not-exist".into()),
                inline: None,
            },
            metadata: serde_json::Value::Null,
            now_ms: 1000,
        })
        .unwrap_err();
    assert!(
        matches!(err, AgentError::UnknownDependency(ref id) if id == "t-does-not-exist"),
        "expected UnknownDependency, got {err:?}"
    );
}

#[test]
fn fallback_inline_is_not_rejected_as_unknown_target() {
    // inline fallback 은 생성 시점엔 아직 존재하지 않는 게 정상 — task 존재
    // 검증 대상이 아니어야 한다 (fallback_validation_rejects_* 와는 다른 축).
    let (_td, mut mem, seq) = fresh_store();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let t = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "A".into(),
            command: run_cmd(),
            depends_on: vec![],
            on_failure: OnFailure::Fallback {
                task: None,
                inline: Some(Box::new(InlineFallbackSpec {
                    name: "A_prime".into(),
                    command: run_cmd(),
                    depends_on_override: None,
                    on_failure: OnFailure::Abort,
                    metadata: serde_json::json!({}),
                })),
            },
            metadata: serde_json::Value::Null,
            now_ms: 1000,
        })
        .unwrap();
    assert_eq!(t.state, TaskState::Ready);
}

/// 결정 1 (가장 중요): `Reduce.inputs` 가 암묵적 의존성으로 승격되어, 입력이
/// 전부 종결되기 전에는 Reduce 가 `Ready` 로 올라가지 않는다. 승격 이전에는
/// `depends_on` 없는 Reduce 가 생성 즉시 `Ready` → dispatch 되어 미완 입력을
/// `Null` 로 조용히 수집하고 `Succeeded` 로 마감했다 (현상 §3, 조용한 오답).
#[test]
fn reduce_waits_for_inputs_to_terminate_before_ready() {
    let (_td, mut mem, seq) = fresh_store();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let a = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "A (slow)".into(),
            command: run_cmd(),
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1000,
        })
        .unwrap();
    // depends_on 을 일부러 비워둔다 — 승격 전에는 이 경로가 즉시 Ready 였다.
    let r = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "R".into(),
            command: reduce_cmd(vec![a.id.clone()]),
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1001,
        })
        .unwrap();
    assert_eq!(
        r.state,
        TaskState::Waiting,
        "A 가 아직 미완이므로 R 은 생성 즉시 Ready 가 되면 안 된다"
    );

    // A 가 여전히 실행 중이어도 R 은 계속 Waiting.
    store.set_state(1, &a.id, TaskState::Running, 2000).unwrap();
    let r_mid = store.get(1, &r.id).unwrap().unwrap();
    assert_eq!(r_mid.state, TaskState::Waiting);

    // A 가 종결(Succeeded)되어야 R 이 Ready.
    let (_, cascaded) = store
        .set_state(1, &a.id, TaskState::Succeeded, 3000)
        .unwrap();
    let r_after = store.get(1, &r.id).unwrap().unwrap();
    assert_eq!(r_after.state, TaskState::Ready);
    assert!(cascaded.iter().any(|t| t.id == r.id));
}

/// Reducer(특히 `all`)는 실패한 입력도 의도적으로 수집하는 계약이므로, 입력이
/// 실패했다고 Reduce 를 `depends_on` 처럼 `Skipped` 로 몰면 안 된다 — 종결
/// 되었으면 (성공이든 실패든) `Ready` 로 진행되어야 dispatch 가 실제로
/// 그 실패 결과를 합성할 수 있다.
#[test]
fn reduce_becomes_ready_after_failed_input_not_skipped() {
    let (_td, mut mem, seq) = fresh_store();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let a = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "A".into(),
            command: run_cmd(),
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1000,
        })
        .unwrap();
    let r = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "R".into(),
            command: reduce_cmd(vec![a.id.clone()]),
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1001,
        })
        .unwrap();
    store.set_state(1, &a.id, TaskState::Running, 2000).unwrap();
    store
        .set_state(
            1,
            &a.id,
            TaskState::Failed {
                error: "boom".into(),
            },
            3000,
        )
        .unwrap();
    let r_after = store.get(1, &r.id).unwrap().unwrap();
    assert_eq!(
        r_after.state,
        TaskState::Ready,
        "실패한 입력도 종결이므로 Reduce 는 Ready — Skipped 로 몰지 않는다"
    );
}

#[test]
fn reduce_input_cycle_via_implicit_edge_is_detected() {
    // A -> R (Reduce.inputs 로 R 이 A 에 의존). 이후 A 에 depends_on=[R] 을
    // 강제로 주입(create 검증 우회, cycle_detected 테스트와 동일 기법)하면
    // A -> R -> A 사이클이 만들어진다. depends_on 만 엣지였다면 못 잡는다.
    let (_td, mut mem, seq) = fresh_store();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let a = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "A".into(),
            command: run_cmd(),
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1000,
        })
        .unwrap();
    let r = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "R".into(),
            command: reduce_cmd(vec![a.id.clone()]),
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1001,
        })
        .unwrap();

    let mut a_mut = a.clone();
    a_mut.depends_on = vec![r.id.clone()];
    store.put(&a_mut).unwrap();
    let all = store.list(1).unwrap();
    let err = TaskGraph::build(&all).detect_cycles().unwrap_err();
    assert!(matches!(err, AgentError::DependencyCycle(_)));
}

/// 결정 3: 본 검증 도입 이전에 저장된 dangling `Fallback.task` 참조는
/// 마이그레이션하지 않는다 — `create()` 우회로 그 상태를 재현하고, main 이
/// 실패해도 패닉하지 않고 downstream 이 영구 `Waiting` 으로 남는지 확인한다.
#[test]
fn legacy_dangling_fallback_target_leaves_downstream_waiting() {
    let (_td, mut mem, seq) = fresh_store();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let a = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "A".into(),
            command: run_cmd(),
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1000,
        })
        .unwrap();
    let c = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "C".into(),
            command: run_cmd(),
            depends_on: vec![a.id.clone()],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1001,
        })
        .unwrap();

    // create() 를 우회해 dangling fallback 참조를 강제로 주입 (레거시 데이터 재현).
    let mut a_mut = a.clone();
    a_mut.on_failure = OnFailure::Fallback {
        task: Some("t-does-not-exist".into()),
        inline: None,
    };
    store.put(&a_mut).unwrap();

    store.set_state(1, &a.id, TaskState::Running, 2000).unwrap();
    let (_, _cascaded) = store
        .set_state(
            1,
            &a.id,
            TaskState::Failed {
                error: "boom".into(),
            },
            3000,
        )
        .unwrap();

    let c_after = store.get(1, &c.id).unwrap().unwrap();
    assert_eq!(
        c_after.state,
        TaskState::Waiting,
        "dangling fallback 은 관측 가능한 경고만 남기고 downstream 은 영구 Waiting"
    );
}

// ============================================================
// TODO11: delete_checked / plan_sweep / apply_sweep_plan
// ============================================================

#[test]
fn delete_checked_rejects_when_referenced_by_depends_on() {
    let (_td, mut mem, seq) = fresh_store();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let a = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "A".into(),
            command: run_cmd(),
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1000,
        })
        .unwrap();
    let b = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "B".into(),
            command: run_cmd(),
            depends_on: vec![a.id.clone()],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1001,
        })
        .unwrap();

    let err = store
        .delete_checked(1, &a.id, TaskDeleteOpts::default())
        .unwrap_err();
    match err {
        AgentError::TaskReferenced {
            task,
            referenced_by,
        } => {
            assert_eq!(task, a.id);
            assert_eq!(referenced_by, vec![b.id.clone()]);
        }
        other => panic!("expected TaskReferenced, got {other:?}"),
    }
    // 거부됐으니 A 는 여전히 존재해야 한다.
    assert!(store.get(1, &a.id).unwrap().is_some());
}

#[test]
fn delete_checked_rejects_when_referenced_by_fallback_task() {
    let (_td, mut mem, seq) = fresh_store();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let fb = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "FB".into(),
            command: run_cmd(),
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1000,
        })
        .unwrap();
    let main = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "MAIN".into(),
            command: run_cmd(),
            depends_on: vec![],
            on_failure: OnFailure::Fallback {
                task: Some(fb.id.clone()),
                inline: None,
            },
            metadata: serde_json::Value::Null,
            now_ms: 1001,
        })
        .unwrap();

    let err = store
        .delete_checked(1, &fb.id, TaskDeleteOpts::default())
        .unwrap_err();
    match err {
        AgentError::TaskReferenced {
            task,
            referenced_by,
        } => {
            assert_eq!(task, fb.id);
            assert_eq!(referenced_by, vec![main.id.clone()]);
        }
        other => panic!("expected TaskReferenced, got {other:?}"),
    }
}

#[test]
fn delete_checked_rejects_when_referenced_by_reduce_inputs() {
    let (_td, mut mem, seq) = fresh_store();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let input = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "IN".into(),
            command: run_cmd(),
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1000,
        })
        .unwrap();
    let reducer = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "REDUCE".into(),
            command: reduce_cmd(vec![input.id.clone()]),
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1001,
        })
        .unwrap();

    let err = store
        .delete_checked(1, &input.id, TaskDeleteOpts::default())
        .unwrap_err();
    match err {
        AgentError::TaskReferenced {
            task,
            referenced_by,
        } => {
            assert_eq!(task, input.id);
            assert_eq!(referenced_by, vec![reducer.id.clone()]);
        }
        other => panic!("expected TaskReferenced, got {other:?}"),
    }
}

/// 가장 중요한 시나리오(TODO11 문서 "완료 확인 방법" #1): 참조 있는 task 를
/// cascade 삭제하면 참조자까지 함께 지워지고, 그 뒤 같은 workspace 에 새 task
/// 를 만드는 게 여전히 성공한다 — raw `delete()` 를 그대로 노출했다면 dangling
/// `depends_on` 이 남아 이후 모든 `create()` 가 `detect_cycles` 의
/// `UnknownDependency` 로 깨졌을 것.
#[test]
fn cascade_delete_removes_referencers_and_workspace_stays_creatable() {
    let (_td, mut mem, seq) = fresh_store();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let a = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "A".into(),
            command: run_cmd(),
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1000,
        })
        .unwrap();
    let b = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "B".into(),
            command: run_cmd(),
            depends_on: vec![a.id.clone()],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1001,
        })
        .unwrap();

    let report = store
        .delete_checked(
            1,
            &a.id,
            TaskDeleteOpts {
                cascade: true,
                force: false,
            },
        )
        .unwrap();
    assert_eq!(report.deleted.len(), 2);
    assert!(report.deleted.contains(&a.id));
    assert!(report.deleted.contains(&b.id));
    assert!(store.get(1, &a.id).unwrap().is_none());
    assert!(store.get(1, &b.id).unwrap().is_none());

    // scenario 2: B 가 영구 Waiting 으로 남지 않는다 — 아예 존재하지 않는다.
    // scenario 1 뒷부분: 이후 create() 가 여전히 정상 동작.
    let c = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "C".into(),
            command: run_cmd(),
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 2000,
        })
        .unwrap();
    assert_eq!(c.state, TaskState::Ready);
}

#[test]
fn delete_checked_force_bypasses_reference_check_but_not_running() {
    let (_td, mut mem, seq) = fresh_store();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let a = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "A".into(),
            command: run_cmd(),
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1000,
        })
        .unwrap();
    let b = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "B".into(),
            command: run_cmd(),
            depends_on: vec![a.id.clone()],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1001,
        })
        .unwrap();

    let report = store
        .delete_checked(
            1,
            &a.id,
            TaskDeleteOpts {
                cascade: false,
                force: true,
            },
        )
        .unwrap();
    assert_eq!(report.deleted, vec![a.id.clone()]);
    assert!(store.get(1, &a.id).unwrap().is_none());
    // force 는 상태 제약은 못 뚫는다 — 남은 B 는 dangling 참조 위에서 그냥
    // 영구 Waiting 으로 남는다(사용자가 명시적으로 감수한 결과).
    assert_eq!(
        store.get(1, &b.id).unwrap().unwrap().state,
        TaskState::Waiting
    );
}

#[test]
fn delete_checked_rejects_running_regardless_of_cascade_or_force() {
    let (_td, mut mem, seq) = fresh_store();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let a = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "A".into(),
            command: run_cmd(),
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1000,
        })
        .unwrap();
    store.set_state(1, &a.id, TaskState::Running, 2000).unwrap();

    for opts in [
        TaskDeleteOpts::default(),
        TaskDeleteOpts {
            cascade: false,
            force: true,
        },
        TaskDeleteOpts {
            cascade: true,
            force: false,
        },
    ] {
        let err = store.delete_checked(1, &a.id, opts).unwrap_err();
        assert!(
            matches!(err, AgentError::TaskRunning(ref id) if *id == a.id),
            "expected TaskRunning, got {err:?}"
        );
    }
    assert!(store.get(1, &a.id).unwrap().is_some());
}

#[test]
fn delete_checked_allows_waiting_ready_and_terminal_states() {
    let (_td, mut mem, seq) = fresh_store();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);

    // Ready (no deps, no referencers).
    let ready = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "READY".into(),
            command: run_cmd(),
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1000,
        })
        .unwrap();
    assert_eq!(ready.state, TaskState::Ready);
    store
        .delete_checked(1, &ready.id, TaskDeleteOpts::default())
        .unwrap();

    // Waiting (Reduce 대기 중 — 아직 input 이 terminal 아님).
    let input = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "IN".into(),
            command: run_cmd(),
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1001,
        })
        .unwrap();
    let waiting = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "WAIT".into(),
            command: reduce_cmd(vec![input.id.clone()]),
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1002,
        })
        .unwrap();
    assert_eq!(waiting.state, TaskState::Waiting);
    store
        .delete_checked(1, &waiting.id, TaskDeleteOpts::default())
        .unwrap();

    // Succeeded (terminal).
    let term = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "TERM".into(),
            command: run_cmd(),
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1003,
        })
        .unwrap();
    store
        .set_state(1, &term.id, TaskState::Running, 2000)
        .unwrap();
    store
        .set_state(1, &term.id, TaskState::Succeeded, 3000)
        .unwrap();
    store
        .delete_checked(1, &term.id, TaskDeleteOpts::default())
        .unwrap();

    assert!(store.get(1, &ready.id).unwrap().is_none());
    assert!(store.get(1, &waiting.id).unwrap().is_none());
    assert!(store.get(1, &term.id).unwrap().is_none());
}

/// TODO11 문서 "완료 확인 방법" #5: 자동 GC 가 완전히 얽힌 `Waiting` 그래프를
/// 실제로 드레인하는지 — 방치된 `Reduce` (X) 가 그 input(Y) 을 참조로 붙잡고
/// 있어도, `Waiting` 은 금지 상태가 아니므로(결정 2) 후보 집합 안에서 둘 다
/// 함께 지워져야 한다(terminal 로 제한했다면 영원히 못 지웠을 그래프).
#[test]
fn plan_sweep_drains_entangled_waiting_reduce_graph() {
    let (_td, mut mem, seq) = fresh_store();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let y = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "Y".into(),
            command: run_cmd(),
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1000,
        })
        .unwrap();
    assert_eq!(y.state, TaskState::Ready);
    let x = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "X".into(),
            command: reduce_cmd(vec![y.id.clone()]),
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1001,
        })
        .unwrap();
    assert_eq!(
        x.state,
        TaskState::Waiting,
        "Y 가 terminal 이 아니라 X 는 영원히 Waiting — 방치된 Reduce 재현"
    );

    let filter = TaskPurgeFilter {
        states: None,
        older_than_ms: Some(1000),
        now_ms: 50_000,
    };
    let plan = store.plan_sweep(1, &filter).unwrap();
    assert!(plan.retained.is_empty(), "retained: {:?}", plan.retained);
    assert!(plan.deleted.contains(&x.id));
    assert!(plan.deleted.contains(&y.id));

    store.apply_sweep_plan(1, &plan).unwrap();
    assert!(store.get(1, &x.id).unwrap().is_none());
    assert!(store.get(1, &y.id).unwrap().is_none());
}

#[test]
fn plan_sweep_retains_task_referenced_from_outside_candidate_set() {
    let (_td, mut mem, seq) = fresh_store();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let a = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "A".into(),
            command: run_cmd(),
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1000,
        })
        .unwrap();
    // B 는 A 에 의존하지만 최근에 생성됨(older_than_ms 필터에 안 걸림) → 후보
    // 집합 밖에서 A 를 참조하는 상황을 만든다.
    let _b = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "B".into(),
            command: run_cmd(),
            depends_on: vec![a.id.clone()],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 49_500,
        })
        .unwrap();

    let filter = TaskPurgeFilter {
        states: None,
        older_than_ms: Some(10_000),
        now_ms: 50_000,
    };
    let plan = store.plan_sweep(1, &filter).unwrap();
    assert_eq!(plan.deleted, Vec::<TaskId>::new());
    assert_eq!(plan.retained, vec![a.id.clone()]);

    store.apply_sweep_plan(1, &plan).unwrap();
    assert!(store.get(1, &a.id).unwrap().is_some());
}

#[test]
fn plan_sweep_filters_by_state_name() {
    let (_td, mut mem, seq) = fresh_store();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let ready = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "READY".into(),
            command: run_cmd(),
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1000,
        })
        .unwrap();
    let term = store
        .create(TaskCreateOpts {
            workspace_id: 1,
            name: "TERM".into(),
            command: run_cmd(),
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            now_ms: 1001,
        })
        .unwrap();
    store
        .set_state(1, &term.id, TaskState::Running, 2000)
        .unwrap();
    store
        .set_state(1, &term.id, TaskState::Succeeded, 3000)
        .unwrap();

    let filter = TaskPurgeFilter {
        states: Some(vec!["succeeded".to_string()]),
        older_than_ms: None,
        now_ms: 50_000,
    };
    let plan = store.plan_sweep(1, &filter).unwrap();
    assert_eq!(plan.deleted, vec![term.id.clone()]);
    assert!(!plan.deleted.contains(&ready.id));
}
