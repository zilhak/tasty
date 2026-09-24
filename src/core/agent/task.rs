//! Core의 저장소와 engine의 공유 시퀀스로 작업을 관리한다.

use tasty_agent::task::{
    TaskCreateOpts, TaskDeleteOpts, TaskDeleteReport, TaskPurgeFilter, TaskSweepPlan,
};
use tasty_agent::{
    AgentError, DagSummary, ReducerInput, Task, TaskId, TaskResult, TaskState, TaskStore,
    group_tasks_into_dags,
};
use tasty_memory::HOST_OWNER;

use crate::core::Core;
use crate::core::CoreState;
use crate::core::agent::runner_host::evict_task_side_keys;

impl Core {
    /// fallback 예약 작업은 참조할 본 작업이 등록되기 전에 Ready가 되지 않게 만든다.
    pub(crate) fn task_create(
        &self,
        engine: &CoreState,
        opts: TaskCreateOpts,
        reserved_for_fallback: bool,
    ) -> Result<Task, AgentError> {
        let seq = engine.agent_seq.clone();
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
        engine: &CoreState,
        workspace_id: u32,
    ) -> Result<Vec<Task>, AgentError> {
        // 렌더 경로도 Core 없이 같은 저장소와 목록 구현을 사용할 수 있게 위임한다.
        task_list_from_state(engine, workspace_id)
    }

    /// ID를 지정하지 않으면 이 engine에 살아 있는 workspace만 순회한다. 삭제된 workspace의 고아 scope는 조회하지 않는다.
    pub(crate) fn dag_list(
        &self,
        engine: &CoreState,
        workspace_id: Option<u32>,
    ) -> Result<Vec<DagSummary>, AgentError> {
        dag_list_from_state(engine, workspace_id)
    }

    /// workspace를 지정하지 않으면 ID 오름차순의 첫 일치를 반환한다.
    /// 사용자가 정한 DAG 키가 여러 workspace에 같을 수 있어 구별하려면 workspace_id도 지정한다.
    pub(crate) fn dag_get(
        &self,
        engine: &CoreState,
        workspace_id: Option<u32>,
        dag_id: &str,
    ) -> Result<Option<(DagSummary, Vec<Task>)>, AgentError> {
        for wid in dag_scan_workspaces(engine, workspace_id) {
            let tasks = self.task_list(engine, wid)?;
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
        engine: &CoreState,
        workspace_id: u32,
        task_id: &TaskId,
    ) -> Result<Option<Task>, AgentError> {
        let seq = engine.agent_seq.clone();
        self.with_memory(|mem| {
            let store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            store.get(workspace_id, task_id)
        })
    }

    pub(crate) fn task_cancel(
        &self,
        engine: &CoreState,
        workspace_id: u32,
        task_id: &TaskId,
        now_ms: u64,
    ) -> Result<(Task, Vec<Task>), AgentError> {
        let seq = engine.agent_seq.clone();
        let result = self.with_memory(|mem| {
            let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            store.cancel(workspace_id, task_id, now_ms)
        });
        if let Ok((ref task, ref downstream)) = result {
            self.fire_waker_if_terminal(engine, workspace_id, task);
            for d in downstream {
                self.fire_waker_if_terminal(engine, workspace_id, d);
            }
        }
        result
    }

    /// 종결 상태 전이 경로는 대기자를 깨우고 사건 피드를 기록하도록 이 hub를 호출해야 한다.
    fn fire_waker_if_terminal(&self, engine: &CoreState, workspace_id: u32, task: &Task) {
        if !task.state.is_terminal() {
            return;
        }
        engine.task_waker_hub.fire(
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
        engine: &CoreState,
        workspace_id: u32,
        task_id: &TaskId,
        reset_downstream: bool,
        now_ms: u64,
    ) -> Result<Task, AgentError> {
        let seq = engine.agent_seq.clone();
        self.with_memory(|mem| {
            let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            store.retry(workspace_id, task_id, reset_downstream, now_ms)
        })
    }

    /// 상태 변경과 자동 전이된 후속 작업을 반환한다.
    pub(crate) fn task_set_state(
        &self,
        engine: &CoreState,
        workspace_id: u32,
        task_id: &TaskId,
        new_state: TaskState,
        now_ms: u64,
    ) -> Result<(Task, Vec<Task>), AgentError> {
        let seq = engine.agent_seq.clone();
        let result = self.with_memory(|mem| {
            let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            store.set_state(workspace_id, task_id, new_state, now_ms)
        });
        if let Ok((ref task, ref downstream)) = result {
            self.fire_waker_if_terminal(engine, workspace_id, task);
            for d in downstream {
                self.fire_waker_if_terminal(engine, workspace_id, d);
            }
        }
        result
    }

    pub(crate) fn task_set_result(
        &self,
        engine: &CoreState,
        workspace_id: u32,
        task_id: &TaskId,
        result: TaskResult,
    ) -> Result<Task, AgentError> {
        let seq = engine.agent_seq.clone();
        self.with_memory(|mem| {
            let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            store.set_result(workspace_id, task_id, result)
        })
    }

    /// 훅 매핑을 소비해 exit code가 0 또는 없으면 성공, 나머지는 실패로 처리한다.
    /// 대기자를 깨울 hub가 engine에 있으므로 훅이 발생한 engine을 전달해야 한다.
    /// 매핑은 저장 전에 제거하며 저장 실패 때 다시 등록하지 않는다.
    pub(crate) fn resolve_hook_task_wait(
        &self,
        engine: &CoreState,
        hook_id: u64,
        exit_code: Option<i32>,
    ) {
        let Some((workspace_id, task_id)) = self.hook_task_waits.resolve(hook_id) else {
            return;
        };
        let result = TaskResult {
            exit_code,
            output: None,
            error: None,
        };
        if let Err(e) = self.task_set_result(engine, workspace_id, &task_id, result) {
            tracing::warn!("resolve_hook_task_wait: set_result {task_id} failed: {e}");
            return;
        }
        let now_ms = self.clock.now_unix_millis() as u64;
        let new_state = match exit_code {
            Some(code) if code != 0 => TaskState::Failed {
                error: format!("command exited with code {code}"),
            },
            _ => TaskState::Succeeded,
        };
        if let Err(e) = self.task_set_state(engine, workspace_id, &task_id, new_state, now_ms) {
            tracing::warn!("resolve_hook_task_wait: set_state {task_id} failed: {e}");
        }
    }

    /// 저장소 락 안에서는 입력 결과만 모으고 실제 reducer 실행은 호출자가 락 밖에서 한다.
    pub(crate) fn task_reduce_collect(
        &self,
        engine: &CoreState,
        workspace_id: u32,
        inputs: &[TaskId],
    ) -> Result<Vec<ReducerInput>, AgentError> {
        let seq = engine.agent_seq.clone();
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
        engine: &CoreState,
        workspace_id: u32,
        task_id: &TaskId,
        opts: TaskDeleteOpts,
    ) -> Result<TaskDeleteReport, AgentError> {
        let seq = engine.agent_seq.clone();
        let report = self.with_memory(|mem| {
            let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            store.delete_checked(workspace_id, task_id, opts)
        })?;
        let ctx = self.runner_context(engine);
        for id in &report.deleted {
            evict_task_side_keys(&ctx, workspace_id, id);
        }
        Ok(report)
    }

    /// 상태나 경과시간 조건 중 하나는 있어야 한다. dry_run과 실제 삭제가 같은 계획을 사용한다.
    /// 계획 조회와 적용은 별도 락 구간이다.
    pub(crate) fn task_purge(
        &self,
        engine: &CoreState,
        workspace_id: u32,
        filter: TaskPurgeFilter,
        dry_run: bool,
    ) -> Result<TaskSweepPlan, AgentError> {
        if filter.states.is_none() && filter.older_than_ms.is_none() {
            return Err(AgentError::InvalidArgument(
                "task_purge requires at least one of 'states'/'older_than_ms'".into(),
            ));
        }
        let seq = engine.agent_seq.clone();
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
        let ctx = self.runner_context(engine);
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
        core.task_create(engine, opts, false)
            .expect("task_create")
            .id
    }

    #[test]
    fn register_then_resolve_completes_the_waiting_task() {
        let (core, _home) = core();
        let engine = engine();
        let ws = 1;
        let task_id = mk_ready_task(&core, &engine, ws);
        core.task_set_state(&engine, ws, &task_id, TaskState::Running, 2)
            .expect("Ready -> Running");

        core.hook_task_waits
            .register(42, ws, task_id.clone(), u64::MAX);
        core.resolve_hook_task_wait(&engine, 42, Some(0));

        let task = core
            .task_reduce_collect(&engine, ws, std::slice::from_ref(&task_id))
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
        core.task_set_state(&engine, ws, &task_id, TaskState::Running, 2)
            .expect("Ready -> Running");

        core.resolve_hook_task_wait(&engine, 999, None);

        let task = core
            .task_reduce_collect(&engine, ws, std::slice::from_ref(&task_id))
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
        core.task_set_state(&engine, ws, &task_id, TaskState::Running, 2)
            .expect("Ready -> Running");

        core.hook_task_waits
            .register(7, ws, task_id.clone(), u64::MAX);
        core.resolve_hook_task_wait(&engine, 7, None);
        core.resolve_hook_task_wait(&engine, 7, None);

        let task = core
            .task_reduce_collect(&engine, ws, std::slice::from_ref(&task_id))
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
        core.task_set_state(&engine, ws, &task_id, TaskState::Running, 2)
            .expect("Ready -> Running");

        core.hook_task_waits
            .register(1, ws, task_id.clone(), u64::MAX);
        core.resolve_hook_task_wait(&engine, 1, Some(1));

        let task = core
            .task_get(&engine, ws, &task_id)
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
            .task_create(&engine, opts, true)
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
        core.task_create(engine, opts, false)
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

        core.task_set_state(&engine, ws, &task_id, TaskState::Running, 2)
            .expect("Ready -> Running");
        core.task_set_state(&engine, ws, &task_id, TaskState::Succeeded, 3)
            .expect("Running -> Succeeded");

        core.task_delete(&engine, ws, &task_id, TaskDeleteOpts::default())
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

        core.task_set_state(&engine, ws, &task_id, TaskState::Running, 2)
            .expect("Ready -> Running");

        let err = core
            .task_delete(&engine, ws, &task_id, TaskDeleteOpts::default())
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

/// Core를 받지 못하는 화면도 같은 목록 조회를 사용할 수 있도록 engine에서 저장소를 가져온다.
pub(crate) fn task_list_from_state(
    engine: &CoreState,
    workspace_id: u32,
) -> Result<Vec<Task>, AgentError> {
    let seq = engine.agent_seq.clone();
    let mut guard = crate::poison::recover_mutex(
        engine.memory.lock(),
        crate::core::MEMORY_WHAT,
        &crate::core::MEMORY_POISONED,
    );
    let store = TaskStore::new(&mut *guard, HOST_OWNER, seq.as_ref());
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

pub(crate) fn dag_list_from_state(
    engine: &CoreState,
    workspace_id: Option<u32>,
) -> Result<Vec<DagSummary>, AgentError> {
    let mut out = Vec::new();
    for wid in dag_scan_workspaces(engine, workspace_id) {
        out.extend(group_tasks_into_dags(&task_list_from_state(engine, wid)?));
    }
    Ok(out)
}

/// 화면은 표시 중인 DAG의 task만 세므로 여기서는 러너 실행·crash 여부만 반환한다.
/// 레지스트리가 없으면 (false, false)다.
#[cfg(feature = "gui")]
pub(crate) fn runner_liveness(engine: &CoreState, workspace_id: u32) -> (bool, bool) {
    engine
        .agent_runner_registry
        .get()
        .map(|registry| registry.liveness(workspace_id))
        .unwrap_or((false, false))
}
