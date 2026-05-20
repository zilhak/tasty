//! `task_tests` 단위 테스트.

#![cfg(test)]

    use super::*;
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

    #[test]
    fn create_single_task_starts_ready() {
        let (_td, mut mem, seq) = fresh_store();
        let mut store = TaskStore::new(&mut mem, "_host", &seq);
        let t = store
            .create(1, "a", run_cmd(), vec![], OnFailure::Abort, serde_json::Value::Null, 1000)
            .unwrap();
        assert_eq!(t.state, TaskState::Ready);
        assert_eq!(t.workspace_id, 1);
    }

    #[test]
    fn linear_dag_transitions_downstream() {
        let (_td, mut mem, seq) = fresh_store();
        let mut store = TaskStore::new(&mut mem, "_host", &seq);
        let a = store
            .create(1, "A", run_cmd(), vec![], OnFailure::Abort, serde_json::Value::Null, 1000)
            .unwrap();
        let b = store
            .create(1, "B", run_cmd(), vec![a.id.clone()], OnFailure::Abort, serde_json::Value::Null, 1001)
            .unwrap();
        let c = store
            .create(1, "C", run_cmd(), vec![b.id.clone()], OnFailure::Abort, serde_json::Value::Null, 1002)
            .unwrap();
        assert_eq!(a.state, TaskState::Ready);
        assert_eq!(b.state, TaskState::Waiting);
        assert_eq!(c.state, TaskState::Waiting);

        store
            .set_state(1, &a.id, TaskState::Running, 2000)
            .unwrap();
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
            .create(1, "A", run_cmd(), vec![], OnFailure::Abort, serde_json::Value::Null, 1000)
            .unwrap();
        let b = store
            .create(1, "B", run_cmd(), vec![a.id.clone()], OnFailure::Abort, serde_json::Value::Null, 1001)
            .unwrap();
        let c = store
            .create(1, "C", run_cmd(), vec![a.id.clone()], OnFailure::Abort, serde_json::Value::Null, 1002)
            .unwrap();
        let d = store
            .create(1, "D", run_cmd(), vec![b.id.clone(), c.id.clone()], OnFailure::Abort, serde_json::Value::Null, 1003)
            .unwrap();

        store.set_state(1, &a.id, TaskState::Running, 2000).unwrap();
        store.set_state(1, &a.id, TaskState::Succeeded, 3000).unwrap();
        assert_eq!(store.get(1, &b.id).unwrap().unwrap().state, TaskState::Ready);
        assert_eq!(store.get(1, &c.id).unwrap().unwrap().state, TaskState::Ready);
        assert_eq!(store.get(1, &d.id).unwrap().unwrap().state, TaskState::Waiting);

        // B done
        store.set_state(1, &b.id, TaskState::Running, 4000).unwrap();
        store.set_state(1, &b.id, TaskState::Succeeded, 5000).unwrap();
        assert_eq!(store.get(1, &d.id).unwrap().unwrap().state, TaskState::Waiting);

        // C done → D ready
        store.set_state(1, &c.id, TaskState::Running, 6000).unwrap();
        store.set_state(1, &c.id, TaskState::Succeeded, 7000).unwrap();
        assert_eq!(store.get(1, &d.id).unwrap().unwrap().state, TaskState::Ready);
    }

    #[test]
    fn abort_propagates_skip() {
        let (_td, mut mem, seq) = fresh_store();
        let mut store = TaskStore::new(&mut mem, "_host", &seq);
        let a = store
            .create(1, "A", run_cmd(), vec![], OnFailure::Abort, serde_json::Value::Null, 1000)
            .unwrap();
        let b = store
            .create(1, "B", run_cmd(), vec![a.id.clone()], OnFailure::Abort, serde_json::Value::Null, 1001)
            .unwrap();
        store.set_state(1, &a.id, TaskState::Running, 2000).unwrap();
        store
            .set_state(
                1,
                &a.id,
                TaskState::Failed { error: "boom".into() },
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
            .create(1, "A", run_cmd(), vec![], OnFailure::Abort, serde_json::Value::Null, 1000)
            .unwrap();
        let b = store
            .create(
                1,
                "B",
                run_cmd(),
                vec![a.id.clone()],
                OnFailure::ContinueDownstream,
                serde_json::Value::Null,
                1001,
            )
            .unwrap();
        store.set_state(1, &a.id, TaskState::Running, 2000).unwrap();
        store
            .set_state(
                1,
                &a.id,
                TaskState::Failed { error: "x".into() },
                3000,
            )
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
            .create(1, "A", run_cmd(), vec![], OnFailure::Abort, serde_json::Value::Null, 1000)
            .unwrap();
        let b = store
            .create(1, "B", run_cmd(), vec![a.id.clone()], OnFailure::Abort, serde_json::Value::Null, 1001)
            .unwrap();
        // create 시 unknown dep는 거부
        let err = store
            .create(1, "X", run_cmd(), vec!["nonexistent".into()], OnFailure::Abort, serde_json::Value::Null, 1002)
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
            .create(1, "A", run_cmd(), vec![], OnFailure::Abort, serde_json::Value::Null, 1000)
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
            .create(1, "A", run_cmd(), vec![], OnFailure::Abort, serde_json::Value::Null, 1000)
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
            .create(1, "A", run_cmd(), vec![], OnFailure::Abort, serde_json::Value::Null, 1000)
            .unwrap();
        let b = store
            .create(1, "B", run_cmd(), vec![a.id.clone()], OnFailure::Abort, serde_json::Value::Null, 1001)
            .unwrap();
        store.set_state(1, &a.id, TaskState::Running, 2000).unwrap();
        store
            .set_state(1, &a.id, TaskState::Failed { error: "e".into() }, 3000)
            .unwrap();
        // B는 Skipped
        assert_eq!(store.get(1, &b.id).unwrap().unwrap().state, TaskState::Skipped);

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
            .create(1, "A_prime", run_cmd(), vec![], OnFailure::Abort, serde_json::Value::Null, 1000)
            .unwrap();
        let a = store
            .create(
                1,
                "A",
                run_cmd(),
                vec![],
                OnFailure::Fallback { task: a_prime.id.clone() },
                serde_json::Value::Null,
                1001,
            )
            .unwrap();
        let c = store
            .create(1, "C", run_cmd(), vec![a.id.clone()], OnFailure::Abort, serde_json::Value::Null, 1002)
            .unwrap();
        // A 실패 시 A_prime 도 이미 Ready 였으므로 변화 없음, C 는 Waiting 유지 (fallback 대기).
        store.set_state(1, &a.id, TaskState::Running, 2000).unwrap();
        store
            .set_state(1, &a.id, TaskState::Failed { error: "boom".into() }, 3000)
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
            .create(1, "A_prime", run_cmd(), vec![], OnFailure::Abort, serde_json::Value::Null, 1000)
            .unwrap();
        let a = store
            .create(
                1,
                "A",
                run_cmd(),
                vec![],
                OnFailure::Fallback { task: a_prime.id.clone() },
                serde_json::Value::Null,
                1001,
            )
            .unwrap();
        let c = store
            .create(1, "C", run_cmd(), vec![a.id.clone()], OnFailure::Abort, serde_json::Value::Null, 1002)
            .unwrap();
        store.set_state(1, &a.id, TaskState::Running, 2000).unwrap();
        store
            .set_state(1, &a.id, TaskState::Failed { error: "boom".into() }, 3000)
            .unwrap();
        store.set_state(1, &a_prime.id, TaskState::Running, 3500).unwrap();
        store.set_state(1, &a_prime.id, TaskState::Succeeded, 4000).unwrap();
        let c_after = store.get(1, &c.id).unwrap().unwrap();
        assert_eq!(c_after.state, TaskState::Ready, "fallback 성공으로 C 진행");
    }

    #[test]
    fn fallback_failure_also_skips_main_downstream() {
        // A.on_failure=Fallback{A'}, C depends on A. A 실패 → A' 도 실패 → C Skipped.
        let (_td, mut mem, seq) = fresh_store();
        let mut store = TaskStore::new(&mut mem, "_host", &seq);
        let a_prime = store
            .create(1, "A_prime", run_cmd(), vec![], OnFailure::Abort, serde_json::Value::Null, 1000)
            .unwrap();
        let a = store
            .create(
                1,
                "A",
                run_cmd(),
                vec![],
                OnFailure::Fallback { task: a_prime.id.clone() },
                serde_json::Value::Null,
                1001,
            )
            .unwrap();
        let c = store
            .create(1, "C", run_cmd(), vec![a.id.clone()], OnFailure::Abort, serde_json::Value::Null, 1002)
            .unwrap();
        store.set_state(1, &a.id, TaskState::Running, 2000).unwrap();
        store
            .set_state(1, &a.id, TaskState::Failed { error: "boom".into() }, 3000)
            .unwrap();
        store.set_state(1, &a_prime.id, TaskState::Running, 3500).unwrap();
        store
            .set_state(1, &a_prime.id, TaskState::Failed { error: "boom2".into() }, 4000)
            .unwrap();
        let c_after = store.get(1, &c.id).unwrap().unwrap();
        assert_eq!(c_after.state, TaskState::Skipped, "fallback 도 실패면 C 도 Skip");
    }

    #[test]
    fn list_returns_all_tasks() {
        let (_td, mut mem, seq) = fresh_store();
        let mut store = TaskStore::new(&mut mem, "_host", &seq);
        store
            .create(1, "A", run_cmd(), vec![], OnFailure::Abort, serde_json::Value::Null, 1000)
            .unwrap();
        store
            .create(1, "B", run_cmd(), vec![], OnFailure::Abort, serde_json::Value::Null, 1001)
            .unwrap();
        store
            .create(2, "C", run_cmd(), vec![], OnFailure::Abort, serde_json::Value::Null, 1002)
            .unwrap();
        let ws1 = store.list(1).unwrap();
        assert_eq!(ws1.len(), 2);
        let ws2 = store.list(2).unwrap();
        assert_eq!(ws2.len(), 1);
    }
