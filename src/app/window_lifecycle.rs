//! 창 생성·등록과 engine 초기화·복원을 담당한다.
//! 첫 창과 추가 창 모두 journal 완료를 기다리는 동안 이벤트 루프에 제어를 돌려준다.

mod pending;

use std::sync::Arc;

use winit::window::Window;

use crate::app::App;
use crate::app::event::WindowRequestOrigin;
use crate::gpu::GpuState;
use crate::{plugin, window};
pub(crate) use pending::PendingWindow;

fn warn_on_theme_err<T, E: std::fmt::Display>(step: &str, result: Result<T, E>) {
    if let Err(e) = result {
        tracing::warn!("{step} failed: {e}");
    }
}

/// 적용 과정에서 테마 ID가 바뀌면 사용자 안내를 위해 원래 요청한 ID를 반환한다.
pub(super) fn boot_apply_theme(settings: &mut tasty_settings::Settings) -> Option<String> {
    let appearance = &mut settings.appearance;
    warn_on_theme_err("themes first_run_init", tasty_themes::first_run_init());
    warn_on_theme_err("sync_builtin_themes", tasty_themes::sync_builtin_themes());
    warn_on_theme_err("themes rescan", tasty_themes::rescan());
    let requested = appearance.theme.clone();
    tasty_themes::apply_theme(appearance, &requested);
    tasty_themes::install_global_with_runtime(&settings.appearance, settings.theme_runtime());
    if settings.appearance.theme != requested {
        Some(requested)
    } else {
        None
    }
}

/// GPU의 cell 크기가 필요하므로 메인 스레드에서 계산한 뒤 부팅 워커에 정수 크기를 넘긴다.
pub(super) fn boot_grid_size(
    gpu: &GpuState,
    sidebar_width: tasty_type_geometry::length::LogicalPx,
) -> (usize, usize) {
    let sf = gpu.scale_factor();
    let size = gpu.size();
    let sidebar_w = sidebar_width.to_physical(sf);
    let terminal_rect = crate::model::PhysicalRect {
        x: sidebar_w,
        y: tasty_type_geometry::length::PhysicalPx(0.0),
        width: (tasty_type_geometry::length::PhysicalPx(size.width as f32) - sidebar_w)
            .max(tasty_type_geometry::length::PhysicalPx(1.0)),
        height: tasty_type_geometry::length::PhysicalPx(size.height as f32),
    };
    gpu.grid_size_for_rect(&terminal_rect)
}

/// App 없이 첫 engine과 플러그인 매니저를 만들어 부팅 워커에서도 사용할 수 있다.
/// engine 생성 오류는 호출자에게 전달하며 첫 부팅과 새 창의 실패 처리는 호출자가 정한다.
pub(super) fn build_engine_and_plugins(
    cols: usize,
    rows: usize,
    factory: crate::waker::SharedWakerFactory,
    proxy: winit::event_loop::EventLoopProxy<crate::AppEvent>,
    memory: std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>>,
    runner_registry: Arc<tasty_task_runtime::RunnerRegistry>,
    layout_slot: crate::core::layout_persistence::LayoutSlotId,
    gauges: crate::app::services::PluginGauges,
    #[cfg(debug_assertions)] input_simulation_enabled: bool,
) -> anyhow::Result<(
    crate::runtime::engine_session::EngineSession,
    plugin::PluginManager,
)> {
    let engine = build_core_state_first_boot(
        cols,
        rows,
        factory.clone(),
        proxy,
        memory,
        runner_registry,
        layout_slot,
        #[cfg(debug_assertions)]
        input_simulation_enabled,
    )?;
    let mgr = build_plugin_manager(factory, &engine.runtime, gauges);
    Ok((engine, mgr))
}

/// 새 창의 engine은 task ID 순번을 기존 engine과 공유하고 runner 등록부는 TaskService의 것을 쓴다.
fn additional_window_task_scope(
    src: &tasty_task_runtime::TaskScope,
    tasks: &tasty_task_runtime::TaskService,
) -> tasty_task_runtime::TaskScope {
    tasty_task_runtime::TaskScope::with_seq(
        Arc::clone(src.agent_seq()),
        Arc::clone(tasks.runner_registry()),
    )
}

fn build_core_state_first_boot(
    cols: usize,
    rows: usize,
    factory: crate::waker::SharedWakerFactory,
    proxy: winit::event_loop::EventLoopProxy<crate::AppEvent>,
    memory: std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>>,
    runner_registry: Arc<tasty_task_runtime::RunnerRegistry>,
    layout_slot: crate::core::layout_persistence::LayoutSlotId,
    #[cfg(debug_assertions)] input_simulation_enabled: bool,
) -> anyhow::Result<crate::runtime::engine_session::EngineSession> {
    // 슬롯 로드 시간도 포함한다. scrollback GC는 창마다 하지 않고 부팅 때 전체 슬롯을 대상으로 한다.
    let t_engine = std::time::Instant::now();
    let waker: crate::terminal::Waker = factory.make_default_waker();
    let mut engine = crate::runtime::engine_session::EngineSession::new_with_ids(
        cols,
        rows,
        waker,
        None,
        Some(layout_slot),
        memory,
        runner_registry,
    )?;
    engine.runtime.waker_factory = Some(factory);
    engine.runtime.identify_worker = Some(Arc::new(
        crate::identify_worker::IdentifyWorker::new(engine.runtime.file_format.clone(), proxy),
    ));
    #[cfg(debug_assertions)]
    {
        engine.runtime.input_simulation_enabled = input_simulation_enabled;
    }
    tracing::info!(
        target: "tasty::boot",
        ms = t_engine.elapsed().as_secs_f64() * 1000.0,
        "T2.6 engine_init (CoreState::new_with_ids + layout slot load)"
    );
    Ok(engine)
}

fn build_plugin_manager(
    factory: crate::waker::SharedWakerFactory,
    runtime: &crate::runtime::engine_runtime::EngineRuntime,
    gauges: crate::app::services::PluginGauges,
) -> plugin::PluginManager {
    let mut mgr = plugin::PluginManager::with_registries(
        factory,
        runtime.file_format.clone(),
        runtime.file_handler.clone(),
    );
    // 호스트와 같은 게이지를 써야 플러그인 대기 시간도 원래 요청의 pressure 기록에 연결된다.
    mgr.set_plugin_wait(gauges.plugin_wait);
    mgr.set_slow_requests(gauges.slow_requests);
    mgr.set_surface_registry(runtime.surface_registry.clone());
    mgr.set_i18n_registrar(std::sync::Arc::new(crate::i18n::BinI18nRegistrar));
    mgr.set_hook_handler_registry(std::sync::Arc::new(
        crate::hook_handler::HostHookHandlerPort,
    ));
    mgr.set_completion_strategy_registry(std::sync::Arc::new(
        crate::completion_strategy::HostCompletionStrategyPort,
    ));
    let t3 = std::time::Instant::now();
    plugin::install_builtins_if_needed(&mut mgr);
    // namespace 해석에 같은 공유 표를 넘긴다. 이후 refresh_packages가 이 표를 갱신한다.
    mgr.install_namespace_table_once();
    mgr.refresh_packages();
    tracing::info!(
        target: "tasty::boot",
        ms = t3.elapsed().as_secs_f64() * 1000.0,
        "T3a plugin_discovery (install_builtins + refresh_packages)"
    );
    let t3b = std::time::Instant::now();
    mgr.discover_and_start();
    tracing::info!(
        target: "tasty::boot",
        ms = t3b.elapsed().as_secs_f64() * 1000.0,
        total_ms = t3.elapsed().as_secs_f64() * 1000.0,
        "T3b plugin_spawn (discover_and_start; total_ms = T3 전체)"
    );
    mgr
}

/// 실패한 창 종류에 맞게 안내한다. 종료 확인 창의 실패는 별도로 종료를 계속한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WindowCreationTarget {
    NewWindow,
    Settings,
    Plugins,
}

impl WindowCreationTarget {
    fn title_key(self) -> &'static str {
        match self {
            Self::NewWindow => "window_error.new_window.title",
            Self::Settings => "window_error.settings.title",
            Self::Plugins => "window_error.plugins.title",
        }
    }

    fn body_key(self) -> &'static str {
        match self {
            Self::NewWindow => "window_error.new_window.body",
            Self::Settings => "window_error.settings.body",
            Self::Plugins => "window_error.plugins.body",
        }
    }
}

impl App {
    /// 없는 engine·매니저만 초기화하며 새 engine은 기존 engine의 공용 상태를 공유한다.
    pub(super) fn ensure_engine_and_plugins(
        &mut self,
        gpu: &GpuState,
        sidebar_width: tasty_type_geometry::length::LogicalPx,
    ) -> anyhow::Result<()> {
        let (cols, rows) = boot_grid_size(gpu, sidebar_width);
        let factory: crate::waker::SharedWakerFactory = Arc::new(
            crate::waker_factory_winit::WinitWakerFactory::new(self.view.proxy.clone()),
        );
        let layout_slot = self.claim_free_layout_slot();

        if self.engines.pending_id().is_none() {
            // 플러그인이 등록한 kind·파일 처리기와 ID 발급기를 창마다 새로 만들지 않는다.
            let shared = self.any_main_engine().map(|src| {
                (
                    src.runtime.surface_registry.clone(),
                    src.runtime.file_format.clone(),
                    src.runtime.file_handler.clone(),
                    src.runtime.identify_worker.clone(),
                    additional_window_task_scope(src.task_scope, &self.services.tasks),
                    src.runtime.counters.clone(),
                )
            });

            let engine = if let Some((
                surface_registry,
                file_format,
                file_handler,
                identify_worker,
                task_scope,
                next_ids,
            )) = shared
            {
                // 전체 슬롯 scrollback GC는 부팅 때만 실행하며 여기서는 새 슬롯을 읽는다.
                let t_engine = std::time::Instant::now();
                // 기본 workspace 생성부터 ID를 발급하므로 기존 발급기를 생성 전에 주입한다.
                let waker: crate::terminal::Waker = factory.make_default_waker();
                let mut engine = crate::runtime::engine_session::EngineSession::new_with_ids(
                    cols,
                    rows,
                    waker,
                    Some(next_ids),
                    Some(layout_slot),
                    self.services.memory_arc(),
                    Arc::clone(self.services.tasks.runner_registry()),
                )?;
                engine.runtime.waker_factory = Some(factory.clone());
                engine.runtime.surface_registry = surface_registry;
                engine.runtime.file_format = file_format;
                engine.runtime.file_handler = file_handler;
                engine.runtime.identify_worker = identify_worker;
                engine.task_scope = task_scope;
                #[cfg(debug_assertions)]
                {
                    engine.runtime.input_simulation_enabled = self.state.input_simulation_enabled;
                }
                tracing::info!(
                    target: "tasty::boot",
                    ms = t_engine.elapsed().as_secs_f64() * 1000.0,
                    "T2.6 engine_init (CoreState::new_with_ids + layout slot load)"
                );
                engine
            } else {
                build_core_state_first_boot(
                    cols,
                    rows,
                    factory.clone(),
                    self.view.proxy.clone(),
                    self.services.memory_arc(),
                    Arc::clone(self.services.tasks.runner_registry()),
                    layout_slot,
                    #[cfg(debug_assertions)]
                    self.state.input_simulation_enabled,
                )?
            };
            self.install_pending_engine(engine);
        }

        if self.plugin_manager.is_none() {
            let gauges = self.services.plugin_gauges();
            let engine=self.engines.pending().ok_or_else(||anyhow::anyhow!("pending engine missing for plugin initialization"))?;
            let mgr = build_plugin_manager(factory, engine.runtime, gauges);
            self.plugin_manager = Some(mgr);
        }
        Ok(())
    }

    pub(super) fn assemble_app_state(
        &mut self,
        restored_idx_after_layout: Option<crate::model::RestoredPresentation>,
    ) -> Result<crate::state::MainViewState, String> {
        let preset_store = self.services.preset_store.clone();
        let engine = self
            .engines
            .pending_mut()
            .ok_or("no pending engine is available to assemble the View")?;
        let mut state = crate::state::MainViewState::new(&engine.read(), preset_store);
        if let Some(restored_idx) = restored_idx_after_layout {
            state
                .navigation
                .restore(&engine.workspaces(), &restored_idx);
        }
        if let Some(mgr) = self.plugin_manager.as_ref() {
            state
                .tool_registry
                .set_plugin_items(mgr.plugin_tool_items());
            // 이후 목록 갱신을 기다리지 않고 첫 화면부터 플러그인 명령을 표시한다.
            state.palette_plugin_commands = mgr.plugin_palette_commands();
        }
        Ok(state)
    }

    pub(super) fn boot_required_plugin_kinds(&self) -> Vec<String> {
        let Some(engine) = self.engines.pending() else {
            return Vec::new();
        };
        engine.local_workspaces().iter().flat_map(|workspace|workspace.all_surface_ids()).filter_map(|id|
            engine.find_surface_by_id(id).and_then(|surface|surface.as_any().downcast_ref::<crate::runtime::surface_restorer::JournalPlaceholder>())
                .filter(|surface|surface.kind!="terminal" && engine.runtime.surface_registry.get_live(&surface.kind).is_none())
                .map(|surface|surface.kind.clone())
        ).collect()
    }

    pub(super) fn boot_pump_step_plugins_registered(&mut self, needed: &[String]) -> bool {
        let hello_pairs = if let Some(mgr) = self.plugin_manager.as_mut() {
            mgr.pump(std::time::Instant::now())
        } else {
            Vec::new()
        };
        self.finalize_plugin_hello(hello_pairs);
        let Some(engine) = self.engines.pending() else {return false;};
        needed
            .iter()
            .all(|k| engine.runtime.surface_registry.get_live(k).is_some())
    }

    pub(super) fn boot_pump_step_remote_restores_done(&mut self) -> bool {
        let still_pending = if let Some(mgr) = self.plugin_manager.as_mut() {
            let hello_pairs = mgr.pump(std::time::Instant::now());
            if !hello_pairs.is_empty() {
                self.finalize_plugin_hello(hello_pairs);
            }
            self.plugin_manager
                .as_ref()
                .is_some_and(|m| m.has_pending_surface_restores())
        } else {
            false
        };
        !still_pending
    }

    /// 새 engine을 창에 배정하기 전 임시 관계로 둔다. 임시 engine은 하나뿐이다.
    pub(crate) fn install_pending_engine(
        &mut self,
        engine: crate::runtime::engine_session::EngineSession,
    ) {
        if self.engines.insert_pending(engine).is_err() {
            tracing::error!("pending engine already present; dropping the new engine");
        }
    }

    /// 창 관계를 parked로 바꾸고 View 복원 자료만 남긴다. engine은 registry에 그대로 있다.
    pub(crate) fn park_main_window(
        &mut self,
        wid: winit::window::WindowId,
        main: Box<crate::view::main::MainView>,
    ) {
        let crate::view::main::MainView { state, .. } = *main;
        if self.engines_mut().park(wid, state).is_none() {
            tracing::error!("parking window {wid:?} without an engine relation");
        }
    }

    /// 창 관계를 끊고 레이아웃 복원 설정에 따라 저장하거나 슬롯 파일을 지운 뒤 View, engine 순서로 버린다.
    pub(crate) fn retire_main_window(
        &mut self,
        wid: winit::window::WindowId,
        main: Box<crate::view::main::MainView>,
    ) {
        if let Some(id)=self.engines.of_window(wid) && let Some(session)=self.engines.session_mut(id) {
            crate::app::attach_activation::cancel_engine(&mut self.pending_server_attaches,id,&mut session.borrow_mut(),&self.stream_hub);
        }
        if !self.journal.is_halted() {
            let retiring = self.engines.of_window(wid).and_then(|id| {
                let session = self.engines.session_mut(id)?;
                (!session.runtime.settings.general.restore_layout)
                    .then(|| (id, session.journal_binding.clone()))
                    .and_then(|(id, binding)| binding.map(|binding| (id, binding)))
            });
            if let Some((id, binding)) = retiring {
                self.engines.begin_retiring_window(wid);
                self.journal.retire_engine(id, binding, true);
                drop(main);
                return;
            }
        }
        if !self.journal.is_halted()
            && let Some(id)=self.engines.of_window(wid)
            && let Some(engine)=self.engines.get(id)
            && engine.runtime.settings.general.restore_layout {
            self.engines.preserve_closed_view(wid,main.state.navigation.clone());
            drop(main);
            self.poll_preserved_window_closes();
            return;
        }
        let Some(mut session) = self.engines.retire_window(wid) else {
            tracing::error!("retiring window {wid:?} without an engine relation");
            return;
        };
        // A halted batch may have changed only part of the live tree. Closing remains available,
        // but neither capture nor slot deletion may turn that partial projection into restore input.
        if !self.journal.is_halted() {
            if session.runtime.settings.general.restore_layout
                && let Some(binding) = session.journal_binding.as_ref()
            {
                let active = session
                    .core_state
                    .workspace_at(main.state.active_workspace_index(&session.core_state))
                    .map(|workspace| workspace.id);
                self.journal.queue_view(
                    crate::runtime::journal_product::view_record::StoredView::capture(
                        binding.clone(),
                        &session.core_state,
                        active,
                        &main.state.navigation,
                    ),
                );
            }

        }
        drop(main);
        drop(session);
    }

    /// 사용자 요청 창은 포커스를 옮기고, 에이전트 요청은 기존 포커스를 유지한다.
    pub(crate) fn register_window(
        &mut self,
        gpu: GpuState,
        state: crate::state::MainViewState,
        engine: crate::runtime::engine_session::EngineId,
        window: Arc<Window>,
        origin: WindowRequestOrigin,
    ) {
        let window_id = window.id();
        let main = window::main::MainView::new(gpu, state, window, self.view.proxy.clone());
        self.view.views.insert(window_id, Box::new(main));
        self.engines.attach_window(window_id, engine);
        self.view.focused_view_id =
            focus_after_register(self.view.focused_view_id, window_id, origin);
        let scripts = self.autofire_scripts();
        if let Some(mgr) = self.plugin_manager.as_mut() {
            use tasty_plugin_protocol::EventScope;
            use tasty_plugin_protocol::events::payloads::{WindowCreated, WindowModality};
            let payload = WindowCreated {
                window_id: u64::from(window_id),
                kind: "main".to_string(),
                modality: WindowModality::Modeless,
            };
            mgr.emit_host_event("window.created", &payload, EventScope::System);
            crate::hooks::lua::fire(
                self.lua_engine.as_ref(),
                crate::hooks::lua::AutofireCtx {
                    scripts: &scripts,
                    guard: &mut self.lua_autofire,
                },
                "window.create.post",
                &payload,
            );
        }
    }

    pub(super) fn focused_main_winit(&self) -> Option<Arc<Window>> {
        let id = self.view.focused_view_id?;
        let view = self.view.views.get(&id)?;
        view.as_main().map(|_| view.base().winit.clone())
    }

    /// 창 생성 결과를 IPC에 돌려준다. 창·GPU·engine 생성 실패는 새 창만 취소하고 사용자 요청이면 기존 창에 알린다.
    pub(crate) fn create_new_window(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        origin: WindowRequestOrigin,
    ) -> Result<winit::window::WindowId, String> {
        use winit::window::WindowAttributes;
        if self.pending_window.is_some() {
            return Err("another window is waiting for its committed engine".into());
        }

        let title = if cfg!(debug_assertions) {
            "Tasty (Debug)"
        } else {
            "Tasty"
        };
        let mut attrs = WindowAttributes::default()
            .with_title(title)
            .with_inner_size(winit::dpi::LogicalSize::new(1280, 720))
            .with_min_inner_size(winit::dpi::LogicalSize::new(640, 480));
        if let Some(icon) = crate::app_icon::winit_window_icon() {
            attrs = attrs.with_window_icon(Some(icon));
        }
        attrs = crate::platform::window_chrome::apply_csd_attributes(attrs);
        attrs = origin_window_attributes(attrs, origin);

        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                return Err(self.notify_window_creation_failed(
                    WindowCreationTarget::NewWindow,
                    origin,
                    "failed to create new window",
                    e,
                ));
            }
        };
        window.set_ime_allowed(true);

        // Windows 절전 복귀 통지를 받을 창을 남기도록 각 창에 훅을 설치한다.
        #[cfg(windows)]
        {
            let proxy = self.view.proxy.clone();
            crate::platform::power_windows::install_resume_hook(
                &window,
                Box::new(move || {
                    crate::shortcuts::send_app_event(&proxy, crate::AppEvent::SystemResumed);
                }),
            );
        }

        // engine이 DB를 사용하기 전에 초기화한다. 오류는 아래에서 확인 후 종료하는 모달로 알린다.
        let db_init_error = crate::db::init().err();

        let (settings, invalid_theme_name) = boot_load_and_normalize_settings();
        let gpu = match self.create_gpu_state(window.clone(), &settings.appearance) {
            Ok(g) => g,
            Err(e) => {
                return Err(self.notify_window_creation_failed(
                    WindowCreationTarget::NewWindow,
                    origin,
                    "failed to initialize GPU for new window",
                    e,
                ));
            }
        };

        let (mut state, engine) = if let Some((engine, state)) = self.engines_mut().unpark_first() {
            tracing::info!(
                "restoring parked state, {} remaining",
                self.engines().parked_count()
            );
            (state, engine)
        } else {
            self.ensure_engine_and_plugins(&gpu, settings.appearance.sidebar_width)
                .map_err(|error| error.to_string())?;
            let engine = self
                .engines
                .pending_id()
                .ok_or("new pending engine is unavailable")?;
            let session = self
                .engines
                .session_mut(engine)
                .ok_or("pending engine is unavailable")?;
            self.journal.begin_engine(
                session,
                crate::runtime::journal_product::EngineSelection::Slot {
                    slot: session
                        .persistence
                        .slot
                        .ok_or("pending GUI engine has no layout slot")?,
                    resume: session.runtime.settings.general.restore_layout,
                },
            )?;
            let id = window.id();
            self.pending_window = Some(PendingWindow {
                window,
                gpu,
                engine,
                origin,
                db_init_error,
                invalid_theme_name,
                completion: None,
                plugin_deadline: None,
            });
            return Ok(id);
        };

        // DB 오류를 먼저 큐에 넣어 확인 시 종료 안내가 다른 모달보다 앞서도록 한다.
        if let Some(err) = db_init_error {
            crate::adapters::ui::info_modal::show_info_modal(
                &mut state,
                build_db_init_error_modal(&err),
            );
        }

        if let Some(invalid) = invalid_theme_name {
            crate::adapters::ui::info_modal::show_info_modal(
                &mut state,
                build_theme_fallback_modal(&invalid),
            );
        }

        let window_id = window.id();
        // 등록 전에 사용자가 보던 창을 기억해 에이전트 창을 그 뒤에 표시한다.
        let behind = matches!(origin, WindowRequestOrigin::Agent)
            .then(|| (window.clone(), self.focused_main_winit()));
        self.register_window(gpu, state, engine, window, origin);
        if let Some((window, anchor)) = behind {
            show_agent_window(&window, anchor.as_deref());
            self.pending_focus_hint_clear.insert(window_id);
        }
        tracing::info!("created new window {window_id:?} ({origin:?})");
        Ok(window_id)
    }

    /// 사용자 요청 실패는 기존 MainView에 알리고, 에이전트 요청 실패는 반환 문자열로만 돌려준다.
    /// 안내할 창이 없으면 로그만 남긴다.
    pub(super) fn notify_window_creation_failed(
        &mut self,
        target: WindowCreationTarget,
        origin: crate::app::event::WindowRequestOrigin,
        context: &str,
        err: impl std::fmt::Display,
    ) -> String {
        use crate::app::event::WindowRequestOrigin;

        tracing::error!("{context}: {err}");
        let body = crate::i18n::t_fmt(target.body_key(), &err.to_string());

        if matches!(origin, WindowRequestOrigin::Agent) {
            return body;
        }

        if let Some(view) = self.notice_window_mut() {
            let modal = crate::adapters::ui::info_modal::InfoModal {
                title: crate::i18n::t(target.title_key()).to_string(),
                body: body.clone(),
                on_close: crate::adapters::ui::info_modal::InfoModalAction::Continue,
                extra_buttons: Vec::new(),
                emphasis: false,
                dismiss_label: None,
            };
            crate::adapters::ui::info_modal::show_info_modal(&mut view.state, modal);
        } else {
            tracing::error!(
                "no main window to surface the window-creation failure notice ({context})"
            );
        }
        body
    }
}

fn boot_load_and_normalize_settings() -> (crate::settings::Settings, Option<String>) {
    let mut settings = crate::settings::Settings::load();
    // 잘못된 설정을 정규화한 값을 저장해 다음 실행에서 같은 안내를 반복하지 않게 한다.
    let normalize_report = settings.normalize();
    if normalize_report.changed
        && let Err(e) = settings.save()
    {
        tracing::warn!("failed to persist normalized settings: {e}");
    }

    let invalid_theme_name = boot_apply_theme(&mut settings);
    if (invalid_theme_name.is_some() || normalize_report.changed)
        && let Err(e) = settings.save()
    {
        tracing::warn!("failed to persist settings after theme apply: {e}");
    }
    (settings, invalid_theme_name)
}

pub(super) fn build_db_init_error_modal(
    err: &crate::db::DbInitError,
) -> crate::adapters::ui::info_modal::InfoModal {
    tracing::error!("state.db init failed: {err}");
    let (key, args) = err.user_message_i18n();
    let body = match args.len() {
        0 => crate::i18n::t(key).to_string(),
        1 => crate::i18n::t_fmt(key, &args[0]),
        _ => crate::i18n::t_fmt2(key, &args[0], &args[1]),
    };
    crate::adapters::ui::info_modal::InfoModal {
        title: crate::i18n::t("db_error.title").to_string(),
        body,
        on_close: crate::adapters::ui::info_modal::InfoModalAction::Exit(1),
        extra_buttons: Vec::new(),
        emphasis: false,
        dismiss_label: Some(crate::i18n::t("db_error.quit").to_string()),
    }
}

pub(super) fn build_theme_fallback_modal(
    invalid_theme_name: &str,
) -> crate::adapters::ui::info_modal::InfoModal {
    crate::adapters::ui::info_modal::InfoModal {
        title: crate::i18n::t("theme_error.title").to_string(),
        body: crate::i18n::t_fmt("theme_error.body", invalid_theme_name),
        on_close: crate::adapters::ui::info_modal::InfoModalAction::Continue,
        extra_buttons: Vec::new(),
        emphasis: false,
        dismiss_label: None,
    }
}

/// 사용자 요청은 새 창으로 옮기고 에이전트 요청은 기존 포커스를 유지한다.
/// 에이전트 요청도 기존 포커스가 None이면 새 창을 사용한다.
pub(crate) fn focus_after_register(
    current: Option<winit::window::WindowId>,
    registered: winit::window::WindowId,
    origin: WindowRequestOrigin,
) -> Option<winit::window::WindowId> {
    match origin {
        WindowRequestOrigin::User => Some(registered),
        WindowRequestOrigin::Agent => current.or(Some(registered)),
    }
}

/// 에이전트 창은 활성화하지 않고 숨겨 만든 뒤 등록 후 표시한다.
pub(crate) fn origin_window_attributes(
    attrs: winit::window::WindowAttributes,
    origin: WindowRequestOrigin,
) -> winit::window::WindowAttributes {
    match origin {
        WindowRequestOrigin::User => attrs.with_active(true),
        WindowRequestOrigin::Agent => attrs.with_active(false).with_visible(false),
    }
}

/// 에이전트 창을 사용자 창 뒤에 표시한다. 네이티브 호출 실패 시 경고하고 일반 표시로 대체한다.
fn show_agent_window(window: &Window, anchor: Option<&Window>) {
    if let Err(e) = crate::platform::window_stacking::show_behind(window, anchor) {
        tracing::warn!(
            "agent window: showing behind the user's window failed ({e}) — showing it the default way"
        );
        window.set_visible(true);
    }
    window.request_redraw();
}

#[cfg(test)]
mod register_focus_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use tasty_task_runtime::{TaskScope, TaskService};

    #[test]
    fn an_additional_window_scope_shares_the_service_registry_and_the_id_sequence() {
        let memory: Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>> = Arc::new(
            std::sync::Mutex::new(tasty_memory::MemoryStore::open_in_memory().expect("memory")),
        );
        let tasks = TaskService::new(memory, Arc::new(std::sync::OnceLock::new()));
        let first = TaskScope::new(Arc::clone(tasks.runner_registry()));

        let added = additional_window_task_scope(&first, &tasks);

        assert!(Arc::ptr_eq(
            added.runner_registry(),
            tasks.runner_registry()
        ));
        assert!(Arc::ptr_eq(added.agent_seq(), first.agent_seq()));
    }
}
