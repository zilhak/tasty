//! DAG 요약의 막힌 대기. 선행 실패 뒤 대기로 남은 v1 task 와, 그 task 때문에 함께 영원히
//! 기다리는 task 가 진행 중으로 보이지 않는지를 저장소의 실제 상태 전이로 확인한다.

use std::sync::atomic::AtomicU64;

use tasty_memory::MemoryStore;
use tempfile::TempDir;

use super::*;

fn fresh() -> (TempDir, MemoryStore, AtomicU64) {
    let td = tempfile::tempdir().expect("tempdir");
    let mem = MemoryStore::open(&td.path().join("mem.db")).expect("mem");
    (td, mem, AtomicU64::new(0))
}

fn run_cmd() -> TaskCommand {
    TaskCommand::Run {
        command: vec!["true".into()],
        workspace_id: 1,
        cwd: None,
    }
}

fn opts(
    name: &str,
    command: TaskCommand,
    depends_on: &[&TaskId],
    on_failure: OnFailure,
) -> TaskCreateOpts {
    TaskCreateOpts {
        workspace_id: 1,
        name: name.to_string(),
        command,
        depends_on: depends_on.iter().map(|id| (*id).clone()).collect(),
        on_failure,
        metadata: serde_json::json!({"dag": "g"}),
        now_ms: 1000,
    }
}

fn finish(store: &mut TaskStore, id: &TaskId, state: TaskState) {
    store.set_state(1, id, TaskState::Running, 2000).unwrap();
    store.set_state(1, id, state, 3000).unwrap();
}

fn failed() -> TaskState {
    TaskState::Failed {
        error: "boom".into(),
    }
}

fn state(store: &TaskStore, id: &TaskId) -> TaskState {
    store.get(1, id).unwrap().expect("task").state
}

fn summary(store: &TaskStore) -> DagSummary {
    let dags = group_tasks_into_dags(&store.list(1).unwrap());
    assert_eq!(dags.len(), 1);
    dags[0].clone()
}

/// main·other 와, main 이 실패하면 대기로 남는 `use`(자기 fallback 을 둔 v1 task)를 만든다.
fn stuck_after_main_failure(
    store: &mut TaskStore,
    use_fallback: OnFailure,
) -> (TaskId, TaskId, TaskId) {
    let main = store
        .create(opts("main", run_cmd(), &[], OnFailure::Abort))
        .unwrap();
    let other = store
        .create(opts("other", run_cmd(), &[], OnFailure::Abort))
        .unwrap();
    let use_ = store
        .create(opts("use", run_cmd(), &[&main.id], use_fallback))
        .unwrap();
    (main.id, other.id, use_.id)
}

/// 반례 1: `use` 의 fallback task `use_fb` 가 실제로 있다. use 가 실패하지 않으니 use_fb 도
/// 깨어나지 않는다. 둘 다 막힌 대기다.
#[test]
fn the_fallback_of_a_stuck_task_is_blocked_too() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let use_fb = store
        .create_reserved_for_fallback(opts("use_fb", run_cmd(), &[], OnFailure::Abort))
        .unwrap();
    let (main, other, use_) = stuck_after_main_failure(
        &mut store,
        OnFailure::Fallback {
            task: Some(use_fb.id.clone()),
            inline: None,
        },
    );
    finish(&mut store, &main, failed());
    finish(&mut store, &other, TaskState::Succeeded);
    assert_eq!(state(&store, &use_), TaskState::Waiting);
    assert_eq!(state(&store, &use_fb.id), TaskState::Waiting);

    let d = summary(&store);
    assert_eq!((d.state_counts.waiting, d.state_counts.blocked), (2, 2));
    assert_eq!(d.rollup_state, "partially_failed");
}

/// 반례 2: 막힌 `use` 의 하류 `down` 과, 막힌 입력을 기다리는 v1 Reduce 도 막힌 대기다.
#[test]
fn tasks_waiting_on_a_stuck_task_are_blocked_too() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let inline = InlineFallbackSpec {
        name: "use_fb".into(),
        command: run_cmd(),
        depends_on_override: None,
        on_failure: OnFailure::Abort,
        metadata: serde_json::Value::Null,
    };
    let (main, other, use_) = stuck_after_main_failure(
        &mut store,
        OnFailure::Fallback {
            task: None,
            inline: Some(Box::new(inline)),
        },
    );
    let down = store
        .create(opts("down", run_cmd(), &[&use_], OnFailure::Abort))
        .unwrap();
    let reduce = store
        .create(opts(
            "reduce",
            TaskCommand::Reduce {
                inputs: vec![use_.clone()],
                strategy: ReducerStrategy::All,
            },
            &[],
            OnFailure::Abort,
        ))
        .unwrap();
    finish(&mut store, &main, failed());

    // other 가 아직 실행될 수 있으니 진행 중이다.
    assert_eq!(summary(&store).rollup_state, "ready");

    finish(&mut store, &other, TaskState::Succeeded);
    for id in [&use_, &down.id, &reduce.id] {
        assert_eq!(state(&store, id), TaskState::Waiting, "{id}");
    }
    let d = summary(&store);
    assert_eq!((d.state_counts.waiting, d.state_counts.blocked), (3, 3));
    assert_eq!(d.rollup_state, "partially_failed");
}

/// `unknown` 은 사람이 개입해야 진행되므로 진행 가능한 것으로 보지 않는다. 그 task 를
/// 기다리는 대기도 막힌 대기다. 실패가 있으면 실패 판정으로 넘어가고, 실패가 없으면 개입이
/// 필요한 task 를 가리지 않도록 대기로 남는다.
#[test]
fn an_unknown_task_does_not_keep_a_failed_dag_in_progress() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let lost = store
        .create(opts("lost", run_cmd(), &[], OnFailure::Abort))
        .unwrap();
    let after = store
        .create(opts("after", run_cmd(), &[&lost.id], OnFailure::Abort))
        .unwrap();
    // 재시작 뒤 결과를 알 수 없게 된 task. 호스트가 상태를 직접 기록한다.
    store
        .set_state(1, &lost.id, TaskState::Running, 2000)
        .unwrap();
    let mut lost = store.get(1, &lost.id).unwrap().expect("task");
    lost.state = TaskState::Unknown { reason: None };
    store.put(&lost).unwrap();
    assert_eq!(state(&store, &after.id), TaskState::Waiting);

    let d = summary(&store);
    assert_eq!((d.state_counts.unknown, d.state_counts.blocked), (1, 1));
    assert_eq!(d.rollup_state, "waiting");

    let bad = store
        .create(opts("bad", run_cmd(), &[], OnFailure::Abort))
        .unwrap();
    finish(&mut store, &bad.id, failed());
    assert_eq!(summary(&store).rollup_state, "failed");
}
