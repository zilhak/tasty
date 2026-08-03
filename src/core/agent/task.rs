//! Task store wrapper. handler 의 `core.with_memory + TaskStore::new` 조립을
//! 본 모듈로 흡수. `agent_seq` 의 시퀀스 공유는 그대로 유지.

use tasty_agent::task::TaskCreateOpts;
use tasty_agent::{AgentError, ReducerInput, Task, TaskId, TaskResult, TaskState, TaskStore};
use tasty_memory::HOST_OWNER;

use crate::core::Core;
use crate::core::CoreState;

impl Core {
    /// Task 생성 — `TaskStore::create` wrapper.
    pub(crate) fn task_create(
        &self,
        engine: &CoreState,
        opts: TaskCreateOpts,
    ) -> Result<Task, AgentError> {
        let seq = engine.agent_seq.clone();
        self.with_memory(|mem| {
            let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            store.create(opts)
        })
    }

    /// Task 목록.
    pub(crate) fn task_list(
        &self,
        engine: &CoreState,
        workspace_id: u32,
    ) -> Result<Vec<Task>, AgentError> {
        let seq = engine.agent_seq.clone();
        self.with_memory(|mem| {
            let store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            store.list(workspace_id)
        })
    }

    /// Task 단건 조회.
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

    /// Task 취소 — downstream cascade 포함.
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

    /// S5: task state 가 종결 (Succeeded/Failed/Cancelled/Skipped) 이면 waker hub 에
    /// fire. set_state / cancel / runner thread 등 *모든* terminal 진입 경로에서
    /// 호출되어야 누락 없음 (R-5 회피).
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

    /// Task retry — 옵션에 따라 downstream reset.
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

    /// Task state 강제 전이 — runner 가 dispatch / poll 결과에 따라 호출.
    /// downstream cascade 도 함께 수행. 반환: (갱신된 자기, 자동 전이된 downstream).
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

    /// Task result 영속. set_state(Succeeded/Failed) 직전에 호출.
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

    /// `hook_id` 에 매핑된 대기 중 task 가 있으면 완료
    /// 처리한다. 없으면 no-op. `engine` 은 `task_set_state` 의 waker 발화
    /// (`task_waker_hub`)에 필요하다 — 호출자는 이 훅이 발화한 그 window/state
    /// 의 engine 을 넘겨야 `agent.task_await` 대기자가 정확히 깨어난다(다른
    /// window 의 engine 을 넘기면 waker 가 엉뚱한 hub 에 발화한다).
    ///
    /// `exit_code` — 실제 관측된 값(`HookEvent::CommandCompleted` 발화만 보유,
    /// 그 외 push 신호는 `None`). `Some(0)` 또는 `None` 이면 Succeeded,
    /// `Some(비0)` 이면 그 코드를 실은 Failed — 결정 7 "exit 0 은 succeeded,
    /// 비-0 은 failed" 를 여기서 강제한다. exit code 개념이 없는 임의 push
    /// 완료 신호(예: 향후 claude/codex 전략)는 `None` 이라 기존처럼 Succeeded.
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

    /// Reducer 단계 1: 입력 task 들의 결과를 `ReducerInput` 형태로 수집.
    /// 실제 reducer / shell I/O 는 handler 가 *memory lock 바깥에서* 실행.
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
                out.push(ReducerInput { succeeded, output });
            }
            Ok(out)
        })
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
            .with_process(Arc::new(MockProcessSpawner::default()))
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
        core.task_create(engine, opts).expect("task_create").id
    }

    /// hook_task_wait 전체 생애주기: register → (해당 hook 발화 시뮬레이션인)
    /// resolve 호출 → task 가 Succeeded 로 마감. `agent.task_set_result` 외부
    /// 호출과 동형의 결과를 훅 경유로 재현한다.
    #[test]
    fn register_then_resolve_completes_the_waiting_task() {
        let (core, _home) = core();
        let engine = engine();
        let ws = 1;
        let task_id = mk_ready_task(&core, &engine, ws);
        // dispatch 가 됐다고 가정 — Ready → Running (실제 러너의 0단계 전이와 동형).
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

    /// 등록되지 않은 hook_id 는 no-op — 대부분의 훅 발화(§B 미구현이라 오늘은
    /// 전부)가 여기 해당하므로 반드시 안전해야 한다.
    #[test]
    fn resolve_unregistered_hook_id_does_not_touch_any_task() {
        let (core, _home) = core();
        let engine = engine();
        let ws = 1;
        let task_id = mk_ready_task(&core, &engine, ws);
        core.task_set_state(&engine, ws, &task_id, TaskState::Running, 2)
            .expect("Ready -> Running");

        // 아무것도 등록 안 한 채 임의 hook_id resolve — task 는 Running 그대로.
        core.resolve_hook_task_wait(&engine, 999, None);

        let task = core
            .task_reduce_collect(&engine, ws, std::slice::from_ref(&task_id))
            .expect("collect")
            .remove(0);
        assert!(!task.succeeded);
    }

    /// resolve 는 1회성 소비 — 같은 hook_id 가 두 번 발화해도(예: 재등록 없이
    /// 중복 이벤트) 두 번째는 이미 소비된 매핑이라 no-op(에러 없이 조용히 무시).
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
        // 두 번째 발화 — 매핑이 이미 소비돼 no-op. 패닉/에러 없이 조용히 지나간다.
        core.resolve_hook_task_wait(&engine, 7, None);

        let task = core
            .task_reduce_collect(&engine, ws, std::slice::from_ref(&task_id))
            .expect("collect")
            .remove(0);
        assert!(task.succeeded);
    }

    /// 결정 7 — 비-0 exit code 로 발화하면 task 는 Failed 로 마감된다(Succeeded
    /// 아님). `command-completed` 내장 push 전략의 핵심 계약.
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
}
