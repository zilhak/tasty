use super::{EngineId, EngineSession};
use crate::core::CoreState;
use crate::core::state::{IdGenerator, ShellConfig};
use crate::model::Workspace;
use crate::settings::Settings;
use std::sync::Arc;
use tasty_terminal::Waker;

impl EngineSession {
    /// 기본 Settings와 in-memory 저장소로 생성한다. 사용자 config.toml의 설정을 읽지 않는다.
    #[cfg(test)]
    pub fn new(cols: usize, rows: usize, waker: Waker) -> anyhow::Result<Self> {
        let memory: std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>> =
            std::sync::Arc::new(std::sync::Mutex::new(
                tasty_memory::MemoryStore::open_in_memory()?,
            ));
        let runner_registry =
            std::sync::Arc::new(crate::core::agent::runner_thread::RunnerRegistry::new());
        Self::new_with_ids_and_settings(
            cols,
            rows,
            waker,
            None,
            None,
            memory,
            runner_registry,
            Settings::default(),
        )
    }

    /// 다른 engine과 발급기를 공유할 수 있다. 슬롯이 있고 restore_layout이 켜져 있을 때만 읽는다.
    /// runner 등록부는 TaskService가 가진 Arc를 넘긴다.
    pub fn new_with_ids(
        cols: usize,
        rows: usize,
        waker: Waker,
        shared_ids: Option<IdGenerator>,
        layout_slot: Option<crate::core::layout_persistence::LayoutSlotId>,
        memory: std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>>,
        runner_registry: std::sync::Arc<crate::core::agent::runner_thread::RunnerRegistry>,
    ) -> anyhow::Result<Self> {
        let state = Self::for_journal(
            cols,
            rows,
            waker,
            shared_ids,
            layout_slot,
            memory,
            runner_registry,
            Settings::load(),
        )?;
        Ok(state)
    }

    /// Allocate services and the resource owner without creating a local structure or PTY.
    pub(crate) fn for_journal(
        cols: usize,
        rows: usize,
        waker: Waker,
        shared_ids: Option<IdGenerator>,
        layout_slot: Option<crate::core::layout_persistence::LayoutSlotId>,
        memory: Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>>,
        runner_registry: Arc<crate::core::agent::runner_thread::RunnerRegistry>,
        settings: Settings,
    ) -> anyhow::Result<Self> {
        Self::assemble(
            cols,
            rows,
            waker,
            shared_ids,
            layout_slot,
            memory,
            runner_registry,
            settings,
            false,
        )
    }

    #[cfg(test)]
    pub(crate) fn new_with_ids_and_settings(
        cols: usize,
        rows: usize,
        waker: Waker,
        shared_ids: Option<IdGenerator>,
        layout_slot: Option<crate::core::layout_persistence::LayoutSlotId>,
        memory: Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>>,
        runner_registry: Arc<crate::core::agent::runner_thread::RunnerRegistry>,
        settings: Settings,
    ) -> anyhow::Result<Self> {
        Self::assemble(
            cols,
            rows,
            waker,
            shared_ids,
            layout_slot,
            memory,
            runner_registry,
            settings,
            true,
        )
    }

    fn assemble(
        cols: usize,
        rows: usize,
        waker: Waker,
        shared_ids: Option<IdGenerator>,
        layout_slot: Option<crate::core::layout_persistence::LayoutSlotId>,
        memory: Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>>,
        runner_registry: Arc<crate::core::agent::runner_thread::RunnerRegistry>,
        settings: Settings,
        materialize_default: bool,
    ) -> anyhow::Result<Self> {
        // CoreState와 실행 자원이 파일을 읽기 전에 검사 홈을 설정한다.
        #[cfg(test)]
        let isolated_home = Some(crate::test_support::IsolatedHome::new());
        let next_ids = shared_ids.unwrap_or_default();
        let mut session = Self {
            id: EngineId::issue(),
            journal_binding: None,
            pending_materializations: Default::default(),
            core_state: CoreState::new_base(
                cols,
                rows,
                waker.clone(),
                next_ids.clone(),
                layout_slot,
                memory,
                settings,
            ),
            hooks: crate::hook_runtime::HookRuntimeState::with_counters(
                next_ids.hook_counter(),
                next_ids.global_hook_counter(),
            ),
            task_scope: crate::core::task_service::TaskScope::new(runner_registry),
            observer_router: crate::output_observer::ObserverRouter::with_counter(
                next_ids.observer_counter(),
            ),
            runtime: crate::core::engine_runtime::EngineRuntime::new(next_ids.pty_counter()),
            #[cfg(test)]
            _isolated_home: isolated_home,
            #[cfg(all(test, feature = "gui"))]
            test_host_commands: None,
        };
        #[cfg(test)]
        if materialize_default
            && session.core_state.settings.general.restore_layout
            && let Some(slot) = layout_slot
        {
            session
                .core_state
                .accept_slot_load(crate::core::layout_persistence::load_slot(slot), slot);
        }
        let mut engine = session.borrow_mut();
        // 복원할 레이아웃이 있으면 기본 PTY를 먼저 만들지 않는다. 복원이 트리를 교체해도 별도 store의 PTY는 남기 때문이다.
        if materialize_default && engine.pending_layout_restore.is_none() {
            let ws_id = engine.next_ids.next_workspace();
            let pane_id = engine.next_ids.next_pane();
            let tab_id = engine.next_ids.next_tab();
            let surface_id = engine.next_ids.next_surface();
            let sh = ShellConfig::from_settings(&engine.settings);
            let (terminal, pty) = crate::core::terminal_spawn::spawn_shell_terminal(
                surface_id,
                crate::core::terminal_spawn::ShellSpawnOpts {
                    cols,
                    rows,
                    shell: sh.shell_ref(),
                    shell_args: &sh.args_ref(),
                    extra_env: &sh.envs_ref(),
                    waker,
                    working_dir: None,
                },
            )?;
            engine
                .runtime
                .terminals
                .insert(surface_id, terminal, Some(pty));
            let ws = Workspace::new_with_terminal_marker(
                ws_id,
                "Workspace 1".to_string(),
                pane_id,
                tab_id,
                surface_id,
            );
            engine.replace_local_workspaces(vec![ws]);
            engine.send_fast_init(surface_id);
        }

        Ok(session)
    }
}
