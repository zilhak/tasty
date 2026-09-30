//! TaskService의 작업 API. 원본은 memory의 TaskStore이며 engine별 순번·허브는 TaskScope로 받는다.

use tasty_agent::task::{
    TaskCreateOpts, TaskDeleteOpts, TaskDeleteReport, TaskPurgeFilter, TaskSweepPlan,
};
use tasty_agent::{
    AgentError, DagSummary, ReducerInput, Task, TaskId, TaskResult, TaskState, TaskStore,
    group_tasks_into_dags,
};
use tasty_memory::HOST_OWNER;

use crate::core::CoreState;
use crate::core::agent::runner_host::evict_task_side_keys;
use crate::core::task_service::{TaskScope, TaskService};

impl TaskService {
    /// fallback 예약 작업은 참조할 본 작업이 등록되기 전에 Ready가 되지 않게 만든다.
    pub(crate) fn task_create(
        &self,
        scope: &TaskScope,
        opts: TaskCreateOpts,
        reserved_for_fallback: bool,
    ) -> Result<Task, AgentError> {
        let seq = scope.agent_seq().clone();
        self.with_memory(|mem| {
            let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            if reserved_for_fallback {
                store.create_reserved_for_fallback(opts)
            } else {
                store.create(opts)
            }
        })
    }

    pub(crate) fn task_list(
        &self,
        scope: &TaskScope,
        workspace_id: u32,
    ) -> Result<Vec<Task>, AgentError> {
        task_list_from_state(self.memory(), scope, workspace_id)
    }

    /// 받은 workspace만 순회한다. 화면의 DAG 목록과 같은 구현을 쓴다.
    pub(crate) fn dag_list(
        &self,
        scope: &TaskScope,
        workspace_ids: &[u32],
    ) -> Result<Vec<DagSummary>, AgentError> {
        dag_list_from_state(self.memory(), scope, workspace_ids)
    }

    /// 받은 순서의 첫 일치를 반환한다. dag_scan_workspaces는 ID 오름차순으로 넘긴다.
    /// 사용자가 정한 DAG 키가 여러 workspace에 같을 수 있어 구별하려면 workspace_id도 지정한다.
    pub(crate) fn dag_get(
        &self,
        scope: &TaskScope,
        workspace_ids: &[u32],
        dag_id: &str,
    ) -> Result<Option<(DagSummary, Vec<Task>)>, AgentError> {
        for wid in workspace_ids.iter().copied() {
            let tasks = self.task_list(scope, wid)?;
            let Some(dag) = group_tasks_into_dags(&tasks)
                .into_iter()
                .find(|d| d.id == dag_id)
            else {
                continue;
            };
            let subset = tasks
                .into_iter()
                .filter(|t| dag.task_ids.contains(&t.id))
                .collect();
            return Ok(Some((dag, subset)));
        }
        Ok(None)
    }

    pub(crate) fn task_get(
        &self,
        scope: &TaskScope,
        workspace_id: u32,
        task_id: &TaskId,
    ) -> Result<Option<Task>, AgentError> {
        let seq = scope.agent_seq().clone();
        self.with_memory(|mem| {
            let store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            store.get(workspace_id, task_id)
        })
    }

    pub(crate) fn task_cancel(
        &self,
        scope: &TaskScope,
        workspace_id: u32,
        task_id: &TaskId,
        now_ms: u64,
    ) -> Result<(Task, Vec<Task>), AgentError> {
        let seq = scope.agent_seq().clone();
        let result = self.with_memory(|mem| {
            let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            store.cancel(workspace_id, task_id, now_ms)
        });
        if let Ok((ref task, ref downstream)) = result {
            self.fire_waker_if_terminal(scope, workspace_id, task);
            for d in downstream {
                self.fire_waker_if_terminal(scope, workspace_id, d);
            }
        }
        result
    }

    /// 종결 상태 전이 경로는 대기자를 깨우고 사건 피드를 기록하도록 이 hub를 호출해야 한다.
    fn fire_waker_if_terminal(&self, scope: &TaskScope, workspace_id: u32, task: &Task) {
        if !task.state.is_terminal() {
            return;
        }
        scope.waker_hub().fire(
            workspace_id,
            &task.id,
            crate::core::agent::task_waker::TerminalSnapshot {
                state: task.state.clone(),
                result: task.result.clone(),
            },
        );
    }

    pub(crate) fn task_retry(
        &self,
        scope: &TaskScope,
        workspace_id: u32,
        task_id: &TaskId,
        reset_downstream: bool,
        now_ms: u64,
    ) -> Result<Task, AgentError> {
        let seq = scope.agent_seq().clone();
        self.with_memory(|mem| {
            let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            store.retry(workspace_id, task_id, reset_downstream, now_ms)
        })
    }

    /// 상태 변경과 자동 전이된 후속 작업을 반환한다.
    pub(crate) fn task_set_state(
        &self,
        scope: &TaskScope,
        workspace_id: u32,
        task_id: &TaskId,
        new_state: TaskState,
        now_ms: u64,
    ) -> Result<(Task, Vec<Task>), AgentError> {
        let seq = scope.agent_seq().clone();
        let result = self.with_memory(|mem| {
            let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            store.set_state(workspace_id, task_id, new_state, now_ms)
        });
        if let Ok((ref task, ref downstream)) = result {
            self.fire_waker_if_terminal(scope, workspace_id, task);
            for d in downstream {
                self.fire_waker_if_terminal(scope, workspace_id, d);
            }
        }
        result
    }

    pub(crate) fn task_set_result(
        &self,
        scope: &TaskScope,
        workspace_id: u32,
        task_id: &TaskId,
        result: TaskResult,
    ) -> Result<Task, AgentError> {
        let seq = scope.agent_seq().clone();
        self.with_memory(|mem| {
            let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            store.set_result(workspace_id, task_id, result)
        })
    }

    /// 훅 매핑을 소비해 exit code가 0 또는 없으면 성공, 나머지는 실패로 처리한다.
    /// 대기자를 깨울 hub가 engine별이므로 훅이 발생한 engine의 범위를 전달해야 한다.
    /// 매핑은 저장 전에 제거하며 저장 실패 때 다시 등록하지 않는다.
    pub(crate) fn resolve_hook_task_wait(
        &self,
        scope: &TaskScope,
        hook_id: u64,
        exit_code: Option<i32>,
        now_ms: u64,
    ) {
        let Some((workspace_id, task_id)) = self.hook_task_waits().resolve(hook_id) else {
            return;
        };
        let result = TaskResult {
            exit_code,
            output: None,
            error: None,
        };
        if let Err(e) = self.task_set_result(scope, workspace_id, &task_id, result) {
            tracing::warn!("resolve_hook_task_wait: set_result {task_id} failed: {e}");
            return;
        }
        let new_state = match exit_code {
            Some(code) if code != 0 => TaskState::Failed {
                error: format!("command exited with code {code}"),
            },
            _ => TaskState::Succeeded,
        };
        if let Err(e) = self.task_set_state(scope, workspace_id, &task_id, new_state, now_ms) {
            tracing::warn!("resolve_hook_task_wait: set_state {task_id} failed: {e}");
        }
    }

    /// 저장소 락 안에서는 입력 결과만 모으고 실제 reducer 실행은 호출자가 락 밖에서 한다.
    pub(crate) fn task_reduce_collect(
        &self,
        scope: &TaskScope,
        workspace_id: u32,
        inputs: &[TaskId],
    ) -> Result<Vec<ReducerInput>, AgentError> {
        let seq = scope.agent_seq().clone();
        self.with_memory(|mem| {
            let store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            let mut out: Vec<ReducerInput> = Vec::with_capacity(inputs.len());
            for tid in inputs {
                let task = match store.get(workspace_id, tid)? {
                    Some(t) => t,
                    None => return Err(AgentError::TaskNotFound(tid.clone())),
                };
                let succeeded = matches!(task.state, TaskState::Succeeded);
                let output = task
                    .result
                    .and_then(|r| r.output)
                    .unwrap_or(serde_json::Value::Null);
                out.push(ReducerInput {
                    succeeded,
                    task_id: tid.clone(),
                    output,
                });
            }
            Ok(out)
        })
    }

    /// 참조·Running 검사를 통과해 삭제된 작업의 handle·실행 결과도 정리한다.
    /// 부속 키 정리는 저장소 락을 다시 사용하므로 첫 락을 놓은 뒤 호출해야 한다.
    pub(crate) fn task_delete(
        &self,
        scope: &TaskScope,
        workspace_id: u32,
        task_id: &TaskId,
        opts: TaskDeleteOpts,
    ) -> Result<TaskDeleteReport, AgentError> {
        let seq = scope.agent_seq().clone();
        let report = self.with_memory(|mem| {
            let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            store.delete_checked(workspace_id, task_id, opts)
        })?;
        let ctx = self.runner_context(scope);
        for id in &report.deleted {
            evict_task_side_keys(&ctx, workspace_id, id);
        }
        Ok(report)
    }

    /// 상태나 경과시간 조건 중 하나는 있어야 한다. dry_run과 실제 삭제가 같은 계획을 사용한다.
    /// 계획 조회와 적용은 별도 락 구간이다.
    pub(crate) fn task_purge(
        &self,
        scope: &TaskScope,
        workspace_id: u32,
        filter: TaskPurgeFilter,
        dry_run: bool,
    ) -> Result<TaskSweepPlan, AgentError> {
        if filter.states.is_none() && filter.older_than_ms.is_none() {
            return Err(AgentError::InvalidArgument(
                "task_purge requires at least one of 'states'/'older_than_ms'".into(),
            ));
        }
        let seq = scope.agent_seq().clone();
        let plan = self.with_memory(|mem| {
            let store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            store.plan_sweep(workspace_id, &filter)
        })?;
        if dry_run {
            return Ok(plan);
        }
        self.with_memory(|mem| {
            let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            store.apply_sweep_plan(workspace_id, &plan)
        })?;
        let ctx = self.runner_context(scope);
        for id in &plan.deleted {
            evict_task_side_keys(&ctx, workspace_id, id);
        }
        Ok(plan)
    }
}

#[cfg(test)]
mod hook_wait_tests {
    use std::sync::{Arc, Mutex};

    use tasty_agent::task::{OnFailure, TaskCommand};
    use tasty_memory::MemoryStorage;
    use tasty_themes::{ThemeStorage, ThemeStore};

    use crate::adapters::test::{
        fake_clock::FakeClock, mem_fs::MemFileSystem, mock_clipboard::MockClipboard,
        mock_process::MockProcessSpawner, tmp_home::TmpHome,
    };
    use crate::core::Core;
    use crate::core::CoreState;
    use crate::core::builder::CoreBuilder;
    use crate::ports::notification_sound::NoopPlayer;

    use super::*;

    fn engine() -> CoreState {
        let waker: tasty_terminal::Waker = Arc::new(|| {});
        CoreState::new(80, 24, waker).expect("engine")
    }

    fn core() -> (Core, tempfile::TempDir) {
        let preset_store: Arc<Mutex<tasty_presets::PresetStore>> =
            Arc::new(Mutex::new(tasty_presets::PresetStore::load_default()));
        let memory: Arc<Mutex<dyn MemoryStorage>> =
            Arc::new(Mutex::new(tasty_memory::testing::InMemoryStorage::new()));
        let themes: Arc<dyn ThemeStorage> = Arc::new(ThemeStore::new());
        let home_tmp = tempfile::tempdir().expect("test tempdir");
        let home = TmpHome::new(home_tmp.path().to_path_buf());

        let core = CoreBuilder::new()
            .with_fs(Arc::new(MemFileSystem::new()))
            .with_clock(Arc::new(FakeClock::default()))
            .with_clipboard(Arc::new(MockClipboard::default()))
            .with_process(Arc::new(MockProcessSpawner))
            .with_home(Arc::new(home))
            .with_sound_player(Arc::new(NoopPlayer))
            .with_memory(memory)
            .with_themes(themes)
            .with_preset_store(preset_store)
            .with_settings_storage(Arc::new(tasty_settings::FileSettingsStorage))
            .build()
            .expect("test Core build");
        (core, home_tmp)
    }

    fn mk_ready_task(core: &Core, engine: &CoreState, workspace_id: u32) -> TaskId {
        let opts = TaskCreateOpts {
            workspace_id,
            name: "t".to_string(),
            command: TaskCommand::Run {
                command: vec!["true".into()],
                workspace_id,
                cwd: None,
            },
            depends_on: Vec::new(),
            on_failure: OnFailure::default(),
            metadata: serde_json::Value::Null,
            now_ms: 1,
        };
        core.tasks
            .task_create(&engine.task_scope, opts, false)
            .expect("task_create")
            .id
    }

    #[test]
    fn register_then_resolve_completes_the_waiting_task() {
        let (core, _home) = core();
        let engine = engine();
        let ws = 1;
        let task_id = mk_ready_task(&core, &engine, ws);
        core.tasks
            .task_set_state(&engine.task_scope, ws, &task_id, TaskState::Running, 2)
            .expect("Ready -> Running");

        core.tasks
            .hook_task_waits()
            .register(42, ws, task_id.clone(), u64::MAX);
        core.tasks.resolve_hook_task_wait(
            &engine.task_scope,
            42,
            Some(0),
            core.now_unix_millis() as u64,
        );

        let task = core
            .tasks
            .task_reduce_collect(&engine.task_scope, ws, std::slice::from_ref(&task_id))
            .expect("collect")
            .remove(0);
        assert!(
            task.succeeded,
            "task should be Succeeded after hook resolve"
        );
    }

    #[test]
    fn resolve_unregistered_hook_id_does_not_touch_any_task() {
        let (core, _home) = core();
        let engine = engine();
        let ws = 1;
        let task_id = mk_ready_task(&core, &engine, ws);
        core.tasks
            .task_set_state(&engine.task_scope, ws, &task_id, TaskState::Running, 2)
            .expect("Ready -> Running");

        core.tasks.resolve_hook_task_wait(
            &engine.task_scope,
            999,
            None,
            core.now_unix_millis() as u64,
        );

        let task = core
            .tasks
            .task_reduce_collect(&engine.task_scope, ws, std::slice::from_ref(&task_id))
            .expect("collect")
            .remove(0);
        assert!(!task.succeeded);
    }

    #[test]
    fn resolve_is_one_shot() {
        let (core, _home) = core();
        let engine = engine();
        let ws = 1;
        let task_id = mk_ready_task(&core, &engine, ws);
        core.tasks
            .task_set_state(&engine.task_scope, ws, &task_id, TaskState::Running, 2)
            .expect("Ready -> Running");

        core.tasks
            .hook_task_waits()
            .register(7, ws, task_id.clone(), u64::MAX);
        core.tasks.resolve_hook_task_wait(
            &engine.task_scope,
            7,
            None,
            core.now_unix_millis() as u64,
        );
        core.tasks.resolve_hook_task_wait(
            &engine.task_scope,
            7,
            None,
            core.now_unix_millis() as u64,
        );

        let task = core
            .tasks
            .task_reduce_collect(&engine.task_scope, ws, std::slice::from_ref(&task_id))
            .expect("collect")
            .remove(0);
        assert!(task.succeeded);
    }

    #[test]
    fn resolve_with_nonzero_exit_code_fails_the_task() {
        let (core, _home) = core();
        let engine = engine();
        let ws = 1;
        let task_id = mk_ready_task(&core, &engine, ws);
        core.tasks
            .task_set_state(&engine.task_scope, ws, &task_id, TaskState::Running, 2)
            .expect("Ready -> Running");

        core.tasks
            .hook_task_waits()
            .register(1, ws, task_id.clone(), u64::MAX);
        core.tasks.resolve_hook_task_wait(
            &engine.task_scope,
            1,
            Some(1),
            core.now_unix_millis() as u64,
        );

        let task = core
            .tasks
            .task_get(&engine.task_scope, ws, &task_id)
            .expect("task_get")
            .expect("task exists");
        assert!(matches!(task.state, TaskState::Failed { .. }));
        assert_eq!(task.result.and_then(|r| r.exit_code), Some(1));
    }

    /// Core의 reserved_for_fallback 인자가 Store의 Waiting 생성으로 이어지는지 확인한다. IPC 인자 파서는 실행하지 않는다.
    #[test]
    fn task_create_reserved_for_fallback_wires_through_core_to_waiting_state() {
        let (core, _home) = core();
        let engine = engine();
        let ws = 1;
        let opts = TaskCreateOpts {
            workspace_id: ws,
            name: "fallback".to_string(),
            command: TaskCommand::Run {
                command: vec!["true".into()],
                workspace_id: ws,
                cwd: None,
            },
            depends_on: Vec::new(),
            on_failure: OnFailure::default(),
            metadata: serde_json::Value::Null,
            now_ms: 1,
        };
        let task = core
            .tasks
            .task_create(&engine.task_scope, opts, true)
            .expect("task_create reserved");
        assert_eq!(
            task.state,
            TaskState::Waiting,
            "reserved_for_fallback=true 는 의존성이 없어도 Ready 를 거치지 않아야 한다"
        );
    }
}

/// 작업 삭제의 부속 키 정리와 Running 삭제 거절 시 permit 보존을 검사한다.
#[cfg(test)]
mod task_delete_tests {
    use std::sync::{Arc, Mutex};

    use tasty_agent::task::{OnFailure, TaskCommand};
    use tasty_agent::{AgentError, SemaphoreStore};
    use tasty_memory::{HOST_OWNER, MemoryStorage, MemoryValue, PutOpts, Scope};
    use tasty_themes::{ThemeStorage, ThemeStore};

    use crate::adapters::test::{
        fake_clock::FakeClock, mem_fs::MemFileSystem, mock_clipboard::MockClipboard,
        mock_process::MockProcessSpawner, tmp_home::TmpHome,
    };
    use crate::core::Core;
    use crate::core::CoreState;
    use crate::core::agent::runner_host::{handle_key, run_result_key};
    use crate::core::builder::CoreBuilder;
    use crate::ports::notification_sound::NoopPlayer;

    use super::*;

    fn engine() -> CoreState {
        let waker: tasty_terminal::Waker = Arc::new(|| {});
        CoreState::new(80, 24, waker).expect("engine")
    }

    fn core() -> (Core, tempfile::TempDir) {
        let preset_store: Arc<Mutex<tasty_presets::PresetStore>> =
            Arc::new(Mutex::new(tasty_presets::PresetStore::load_default()));
        let memory: Arc<Mutex<dyn MemoryStorage>> =
            Arc::new(Mutex::new(tasty_memory::testing::InMemoryStorage::new()));
        let themes: Arc<dyn ThemeStorage> = Arc::new(ThemeStore::new());
        let home_tmp = tempfile::tempdir().expect("test tempdir");
        let home = TmpHome::new(home_tmp.path().to_path_buf());

        let core = CoreBuilder::new()
            .with_fs(Arc::new(MemFileSystem::new()))
            .with_clock(Arc::new(FakeClock::default()))
            .with_clipboard(Arc::new(MockClipboard::default()))
            .with_process(Arc::new(MockProcessSpawner))
            .with_home(Arc::new(home))
            .with_sound_player(Arc::new(NoopPlayer))
            .with_memory(memory)
            .with_themes(themes)
            .with_preset_store(preset_store)
            .with_settings_storage(Arc::new(tasty_settings::FileSettingsStorage))
            .build()
            .expect("test Core build");
        (core, home_tmp)
    }

    fn mk_ready_task(core: &Core, engine: &CoreState, workspace_id: u32) -> TaskId {
        let opts = TaskCreateOpts {
            workspace_id,
            name: "t".to_string(),
            command: TaskCommand::Run {
                command: vec!["true".into()],
                workspace_id,
                cwd: None,
            },
            depends_on: Vec::new(),
            on_failure: OnFailure::default(),
            metadata: serde_json::Value::Null,
            now_ms: 1,
        };
        core.tasks
            .task_create(&engine.task_scope, opts, false)
            .expect("task_create")
            .id
    }

    #[test]
    fn task_delete_evicts_handle_and_run_result_side_keys() {
        let (core, _home) = core();
        let engine = engine();
        let ws = 1;
        let task_id = mk_ready_task(&core, &engine, ws);

        core.with_memory(|mem| {
            mem.put(
                HOST_OWNER,
                &Scope::Workspace(ws),
                &handle_key(&task_id),
                &MemoryValue::Json(serde_json::json!({"kind": "shell_process", "pid": 123})),
                &PutOpts::default(),
            )
        })
        .expect("persist handle");
        core.with_memory(|mem| {
            mem.put(
                HOST_OWNER,
                &Scope::Workspace(ws),
                &run_result_key(&task_id),
                &MemoryValue::Json(serde_json::json!({"kind": "done", "exit_code": 0})),
                &PutOpts::default(),
            )
        })
        .expect("persist run_result");

        core.tasks
            .task_set_state(&engine.task_scope, ws, &task_id, TaskState::Running, 2)
            .expect("Ready -> Running");
        core.tasks
            .task_set_state(&engine.task_scope, ws, &task_id, TaskState::Succeeded, 3)
            .expect("Running -> Succeeded");

        core.tasks
            .task_delete(&engine.task_scope, ws, &task_id, TaskDeleteOpts::default())
            .expect("delete succeeded task");

        let handle_gone = core
            .with_memory(|mem| mem.get(&Scope::Workspace(ws), &handle_key(&task_id)))
            .expect("get handle");
        let run_result_gone = core
            .with_memory(|mem| mem.get(&Scope::Workspace(ws), &run_result_key(&task_id)))
            .expect("get run_result");
        assert!(
            handle_gone.is_none(),
            "handle side-key must be evicted on delete"
        );
        assert!(
            run_result_gone.is_none(),
            "run_result side-key must be evicted on delete"
        );
    }

    #[test]
    fn task_delete_rejects_running_task_without_touching_held_semaphore_permit() {
        let (core, _home) = core();
        let engine = engine();
        let ws = 1;
        let task_id = mk_ready_task(&core, &engine, ws);

        core.with_memory(|mem| {
            let mut sem = SemaphoreStore::new(mem, HOST_OWNER);
            sem.create(ws, "gate", 1, 1)?;
            sem.acquire(ws, "gate", &task_id, None, 1000)?;
            Ok::<_, AgentError>(())
        })
        .expect("acquire permit");

        core.tasks
            .task_set_state(&engine.task_scope, ws, &task_id, TaskState::Running, 2)
            .expect("Ready -> Running");

        let err = core
            .tasks
            .task_delete(&engine.task_scope, ws, &task_id, TaskDeleteOpts::default())
            .expect_err("Running task delete must be rejected");
        assert!(matches!(err, AgentError::TaskRunning(_)));

        let sem_after = core
            .with_memory(|mem| SemaphoreStore::new(mem, HOST_OWNER).get(ws, "gate"))
            .expect("get semaphore")
            .expect("semaphore exists");
        assert_eq!(
            sem_after
                .holders
                .iter()
                .map(|h| h.id.clone())
                .collect::<Vec<_>>(),
            vec![task_id.clone()],
            "rejected delete must not touch the held permit"
        );
    }
}

/// 서비스와 서비스를 받지 못하는 화면이 같은 목록 조회를 쓴다. 화면은 engine의 저장소를 넘긴다.
pub(crate) fn task_list_from_state(
    memory: &std::sync::Mutex<dyn tasty_memory::MemoryStorage>,
    scope: &TaskScope,
    workspace_id: u32,
) -> Result<Vec<Task>, AgentError> {
    let mut guard = crate::poison::recover_mutex(
        memory.lock(),
        crate::core::MEMORY_WHAT,
        &crate::core::MEMORY_POISONED,
    );
    let store = TaskStore::new(&mut *guard, HOST_OWNER, scope.agent_seq().as_ref());
    store.list(workspace_id)
}

/// ID 미지정 시 이 engine의 live workspace를 오름차순으로 순회한다. 명시한 ID는 그대로 사용한다.
pub(crate) fn dag_scan_workspaces(engine: &CoreState, workspace_id: Option<u32>) -> Vec<u32> {
    match workspace_id {
        Some(w) => vec![w],
        None => {
            let mut ids: Vec<u32> = engine.workspaces.iter().map(|w| w.id).collect();
            ids.sort_unstable();
            ids
        }
    }
}

/// 받은 workspace만 순회한다. 호출자는 dag_scan_workspaces로 engine의 live workspace를 넘겨
/// 삭제된 workspace의 고아 scope를 조회하지 않게 한다.
pub(crate) fn dag_list_from_state(
    memory: &std::sync::Mutex<dyn tasty_memory::MemoryStorage>,
    scope: &TaskScope,
    workspace_ids: &[u32],
) -> Result<Vec<DagSummary>, AgentError> {
    let mut out = Vec::new();
    for wid in workspace_ids {
        out.extend(group_tasks_into_dags(&task_list_from_state(
            memory, scope, *wid,
        )?));
    }
    Ok(out)
}
