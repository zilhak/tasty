//! 창 생성·등록과 engine 초기화·복원을 담당한다.
//! 첫 창은 boot_machine이 단계를 나눠 실행하고 새 창 생성은 동기 경로로 같은 하위 함수를 사용한다.

use std::sync::Arc;

use winit::window::Window;

use crate::app::App;
use crate::app::event::WindowRequestOrigin;
use crate::gpu::GpuState;
use crate::{plugin, window};

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
    layout_slot: crate::core::layout_persistence::LayoutSlotId,
    gauges: crate::core::PluginGauges,
    #[cfg(debug_assertions)] input_simulation_enabled: bool,
) -> anyhow::Result<(crate::core::CoreState, plugin::PluginManager)> {
    let engine = build_core_state_first_boot(
        cols,
        rows,
        factory.clone(),
        proxy,
        memory,
        layout_slot,
        #[cfg(debug_assertions)]
        input_simulation_enabled,
    )?;
    let mgr = build_plugin_manager(factory, &engine, gauges);
    Ok((engine, mgr))
}

fn build_core_state_first_boot(
    cols: usize,
    rows: usize,
    factory: crate::waker::SharedWakerFactory,
    proxy: winit::event_loop::EventLoopProxy<crate::AppEvent>,
    memory: std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>>,
    layout_slot: crate::core::layout_persistence::LayoutSlotId,
    #[cfg(debug_assertions)] input_simulation_enabled: bool,
) -> anyhow::Result<crate::core::CoreState> {
    // 슬롯 로드 시간도 포함한다. scrollback GC는 창마다 하지 않고 부팅 때 전체 슬롯을 대상으로 한다.
    let t_engine = std::time::Instant::now();
    let waker: crate::terminal::Waker = factory.make_default_waker();
    let mut engine =
        crate::core::CoreState::new_with_ids(cols, rows, waker, None, Some(layout_slot), memory)?;
    engine.waker_factory = Some(factory);
    engine.identify_worker = Some(Arc::new(crate::identify_worker::IdentifyWorker::new(
        engine.file_format.clone(),
        proxy,
    )));
    #[cfg(debug_assertions)]
    {
        engine.input_simulation_enabled = input_simulation_enabled;
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
    engine: &crate::core::CoreState,
    gauges: crate::core::PluginGauges,
) -> plugin::PluginManager {
    let mut mgr = plugin::PluginManager::with_registries(
        factory,
        engine.file_format.clone(),
        engine.file_handler.clone(),
    );
    // 호스트와 같은 게이지를 써야 플러그인 대기 시간도 원래 요청의 pressure 기록에 연결된다.
    mgr.set_plugin_wait(gauges.plugin_wait);
    mgr.set_slow_requests(gauges.slow_requests);
    mgr.set_surface_registry(engine.surface_registry.clone());
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
    /// 추가 창의 동기 초기화. 첫 부팅은 같은 단계를 boot_machine에서 나눠 실행한다.
    pub(crate) fn create_app_state(
        &mut self,
        gpu: &GpuState,
        sidebar_width: tasty_type_geometry::length::LogicalPx,
    ) -> anyhow::Result<crate::state::AppState> {
        self.ensure_engine_and_plugins(gpu, sidebar_width)?;

        // main loop 진입 전이라 Intent 큐를 기다리지 않고 복원을 직접 적용한다.
        let restored_idx_after_layout = if self.core_state().pending_layout_restore.is_some() {
            self.boot_wait_for_required_plugin_kinds();
            let restored = self.boot_apply_pending_layout_restore();
            self.boot_wait_for_remote_surface_restores();
            restored
        } else {
            None
        };

        // 복원을 예정한 engine에는 기본 workspace가 없을 수 있어 복원 실패 뒤 보충한다.
        let bootstrapped = match self.core_state.as_mut() {
            Some(engine) => Self::bootstrap_workspace_if_empty(&mut self.core, engine),
            None => None,
        };

        Ok(self.assemble_app_state(bootstrapped.or(restored_idx_after_layout)))
    }

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

        if self.core_state.is_none() {
            // 플러그인이 등록한 kind·파일 처리기와 ID 발급기를 창마다 새로 만들지 않는다.
            let shared = self.any_main_engine().map(|src| {
                (
                    src.surface_registry.clone(),
                    src.file_format.clone(),
                    src.file_handler.clone(),
                    src.identify_worker.clone(),
                    src.approval_store.clone(),
                    src.telemetry_seq.clone(),
                    src.anomaly_detector.clone(),
                    src.agent_seq.clone(),
                    src.next_ids.clone(),
                )
            });

            let engine = if let Some((
                surface_registry,
                file_format,
                file_handler,
                identify_worker,
                approval_store,
                telemetry_seq,
                anomaly_detector,
                agent_seq,
                next_ids,
            )) = shared
            {
                // 전체 슬롯 scrollback GC는 부팅 때만 실행하며 여기서는 새 슬롯을 읽는다.
                let t_engine = std::time::Instant::now();
                // 기본 workspace 생성부터 ID를 발급하므로 기존 발급기를 생성 전에 주입한다.
                let waker: crate::terminal::Waker = factory.make_default_waker();
                let mut engine = crate::core::CoreState::new_with_ids(
                    cols,
                    rows,
                    waker,
                    Some(next_ids),
                    Some(layout_slot),
                    self.core.memory_arc(),
                )?;
                engine.waker_factory = Some(factory.clone());
                engine.surface_registry = surface_registry;
                engine.file_format = file_format;
                engine.file_handler = file_handler;
                engine.identify_worker = identify_worker;
                engine.approval_store = approval_store;
                engine.telemetry_seq = telemetry_seq;
                engine.anomaly_detector = anomaly_detector;
                engine.agent_seq = agent_seq;
                #[cfg(debug_assertions)]
                {
                    engine.input_simulation_enabled = self.input_simulation_enabled;
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
                    self.core.memory_arc(),
                    layout_slot,
                    #[cfg(debug_assertions)]
                    self.input_simulation_enabled,
                )?
            };
            self.core_state = Some(engine);
        }

        if self.plugin_manager.is_none() {
            let gauges = self.core.plugin_gauges();
            let mgr = build_plugin_manager(factory, self.core_state(), gauges);
            self.plugin_manager = Some(mgr);
        }
        Ok(())
    }

    pub(super) fn boot_apply_pending_layout_restore(&mut self) -> Option<usize> {
        let t5 = std::time::Instant::now();
        let engine = self
            .core_state
            .as_mut()
            .expect("core_state must be initialized before layout restore");
        let restored = match self.core.apply(
            engine,
            crate::core::intent::DomainIntent::ApplyPendingLayoutRestore,
        ) {
            Ok(events) => events.into_iter().find_map(|e| {
                if let crate::core::intent::CoreEvent::LayoutRestored {
                    restored: true,
                    active_workspace,
                } = e
                {
                    tracing::info!("Layout restored from slot file (deferred)");
                    active_workspace
                } else {
                    None
                }
            }),
            Err(e) => {
                tracing::warn!("ApplyPendingLayoutRestore failed: {e}");
                None
            }
        };
        tracing::info!(
            target: "tasty::boot",
            ms = t5.elapsed().as_secs_f64() * 1000.0,
            "T5 layout_apply (ApplyPendingLayoutRestore)"
        );
        restored
    }

    pub(super) fn assemble_app_state(
        &mut self,
        restored_idx_after_layout: Option<usize>,
    ) -> crate::state::AppState {
        let preset_store = self.core.preset_store.clone();
        let memory = self.core.memory_arc();
        let mut state = crate::state::AppState::new(self.core_state_mut(), preset_store, memory);
        if let Some(restored_idx) = restored_idx_after_layout {
            state.switch_workspace(self.core_state_mut(), restored_idx);
        }
        if let Some(mgr) = self.plugin_manager.as_ref() {
            state
                .tool_registry
                .set_plugin_items(mgr.plugin_tool_items());
            // 이후 목록 갱신을 기다리지 않고 첫 화면부터 플러그인 명령을 표시한다.
            state.palette_plugin_commands = mgr.plugin_palette_commands();
        }
        state
    }

    /// 필요한 플러그인 kind가 등록되거나 대기 기한이 지날 때까지 pump한다.
    /// 복원 데이터는 여기서 가져오지 않고 실제 복원 Intent에 남겨 둔다.
    fn boot_wait_for_required_plugin_kinds(&mut self) {
        use crate::app::boot_machine::PLUGIN_WAIT_DEADLINE;
        use std::time::{Duration, Instant};
        let needed = self.boot_required_plugin_kinds();
        let t4 = Instant::now();
        let deadline = t4 + PLUGIN_WAIT_DEADLINE;
        let mut t4_reason = "deadline";
        while Instant::now() < deadline {
            if self.boot_pump_step_plugins_registered(&needed) {
                t4_reason = "satisfied";
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        tracing::info!(
            target: "tasty::boot",
            ms = t4.elapsed().as_secs_f64() * 1000.0,
            reason = t4_reason,
            deadline_ms = PLUGIN_WAIT_DEADLINE.as_millis() as u64,
            "T4 layout_wait_plugins"
        );
    }

    pub(super) fn boot_required_plugin_kinds(&self) -> Vec<String> {
        self.core_state()
            .pending_layout_restore
            .as_ref()
            .map(|s| s.required_plugin_kinds())
            .unwrap_or_default()
    }

    pub(super) fn boot_pump_step_plugins_registered(&mut self, needed: &[String]) -> bool {
        let hello_pairs = if let Some(mgr) = self.plugin_manager.as_mut() {
            mgr.pump(std::time::Instant::now())
        } else {
            Vec::new()
        };
        self.finalize_plugin_hello(hello_pairs);
        let engine = self.core_state();
        needed
            .iter()
            .all(|k| engine.surface_registry.get_live(k).is_some())
    }

    /// 복원 요청을 pump해 응답을 기다리되 기한이 지나면 계속 부팅한다.
    /// 복원 응답이 첫 사용자 조작을 뒤늦게 덮는 경우를 줄이려는 대기이며 완료를 보장하지 않는다.
    /// 응답 전에는 RemoteSurface가 보존한 carry 상태를 사용한다.
    fn boot_wait_for_remote_surface_restores(&mut self) {
        use std::time::{Duration, Instant};
        let t6 = Instant::now();
        let deadline = t6 + Duration::from_millis(500);
        let mut t6_reason = "deadline";
        while Instant::now() < deadline {
            if self.boot_pump_step_remote_restores_done() {
                t6_reason = "satisfied";
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        tracing::info!(
            target: "tasty::boot",
            ms = t6.elapsed().as_secs_f64() * 1000.0,
            reason = t6_reason,
            "T6 remote_surface_wait (deadline 500ms)"
        );
    }

    /// pending 여부를 보기 전에 pump해 송수신을 진행한다. 매니저가 없으면 완료로 본다.
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

    /// 사용자 요청 창은 포커스를 옮기고, 에이전트 요청은 기존 포커스를 유지한다.
    pub(crate) fn register_window(
        &mut self,
        gpu: GpuState,
        state: crate::state::AppState,
        core_state: crate::core::CoreState,
        window: Arc<Window>,
        origin: WindowRequestOrigin,
    ) {
        let window_id = window.id();
        let main =
            window::main::MainView::new(gpu, state, core_state, window, self.view.proxy.clone());
        self.view.views.insert(window_id, Box::new(main));
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

    fn focused_main_winit(&self) -> Option<Arc<Window>> {
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

        let (mut state, mut core_state) =
            match self.acquire_app_state_and_engine(&gpu, settings.appearance.sidebar_width) {
                Ok(pair) => pair,
                Err(e) => {
                    return Err(self.notify_window_creation_failed(
                        WindowCreationTarget::NewWindow,
                        origin,
                        "failed to create engine for new window",
                        e,
                    ));
                }
            };
        self.ensure_at_least_one_workspace(&mut core_state, &mut state);

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
        self.register_window(gpu, state, core_state, window, origin);
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
            };
            crate::adapters::ui::info_modal::show_info_modal(&mut view.state, modal);
        } else {
            tracing::error!(
                "no main window to surface the window-creation failure notice ({context})"
            );
        }
        body
    }

    fn acquire_app_state_and_engine(
        &mut self,
        gpu: &GpuState,
        sidebar_width: tasty_type_geometry::length::LogicalPx,
    ) -> anyhow::Result<(crate::state::AppState, crate::core::CoreState)> {
        let (state, parked_engine) = if !self.parked_states.is_empty() {
            let parked = self.parked_states.remove(0);
            tracing::info!(
                "restoring parked state, {} remaining",
                self.parked_states.len()
            );
            let (st, eng) = parked;
            (st, Some(eng))
        } else {
            let st = self.create_app_state(gpu, sidebar_width)?;
            (st, None)
        };

        // parked engine의 슬롯은 그대로 유지한다. 새 슬롯을 주면 다른 창의 복원 파일을 덮을 수 있다.
        let core_state = match parked_engine {
            Some(e) => e,
            None => self
                .core_state
                .take()
                .expect("App.core_state must be present to register a main window"),
        };
        Ok((state, core_state))
    }

    fn ensure_at_least_one_workspace(
        &mut self,
        core_state: &mut crate::core::CoreState,
        state: &mut crate::state::AppState,
    ) {
        if let Some(idx) = Self::bootstrap_workspace_if_empty(&mut self.core, core_state) {
            state.active_workspace = idx;
        }
    }

    /// 복원 예정 engine은 기본 workspace 없이 시작할 수 있어 복원 뒤 비어 있으면 하나 만든다.
    /// 부팅·추가 창 경로가 함께 사용한다. 생성 실패는 로그를 남기고 None을 반환한다.
    pub(super) fn bootstrap_workspace_if_empty(
        core: &mut crate::core::Core,
        engine: &mut crate::core::CoreState,
    ) -> Option<usize> {
        if !engine.workspaces.is_empty() {
            return None;
        }
        match core.create_default_workspace(engine) {
            Ok(idx) => Some(idx),
            Err(e) => {
                tracing::error!("bootstrap workspace failed: {e}");
                None
            }
        }
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

fn build_db_init_error_modal(
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
    }
}

fn build_theme_fallback_modal(
    invalid_theme_name: &str,
) -> crate::adapters::ui::info_modal::InfoModal {
    crate::adapters::ui::info_modal::InfoModal {
        title: crate::i18n::t("theme_error.title").to_string(),
        body: crate::i18n::t_fmt("theme_error.body", invalid_theme_name),
        on_close: crate::adapters::ui::info_modal::InfoModalAction::Continue,
        extra_buttons: Vec::new(),
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
