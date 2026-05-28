//! `App` 의 윈도우 라이프사이클 메서드.
//!
//! - `create_app_state`: GPU 상태 + 사이드바 폭으로부터 새 `AppState` 를 만든다.
//!   첫 호출 시 plugin manager 도 초기화하며, `pending_layout_restore` 가 있으면
//!   plugin 등록을 짧게 기다린 뒤 layout 을 복원한다.
//! - `register_window`: 만들어진 `MainWindow` 를 hash 에 등록 + focused 로 설정 +
//!   `window.created` host event / lua hook 발화.
//! - `init_app_state`: shell 확정 후 첫 윈도우의 IPC server 시작 + AppState 부착.
//! - `create_new_window`: 다중 윈도우용 — 새 winit window + GPU + AppState (parked 우선) + 모달 안내.

use std::sync::Arc;

use winit::window::Window;

use crate::app::App;
use crate::gpu::GpuState;
use crate::{plugin, window};

/// 부팅 흐름의 테마 적용. `tasty-themes` 의 디스크 초기화 + 전역 Theme 설치를 한 단계로 묶는다.
///
/// 반환: `settings.appearance.theme` 가 디스크/캐시에 없어 mocha 로 fallback 된 경우 원래 요청 id.
/// (호출자가 InfoModal 로 사용자에게 알린다.)
fn boot_apply_theme(appearance: &mut tasty_settings::AppearanceSettings) -> Option<String> {
    if let Err(e) = tasty_themes::first_run_init() {
        tracing::warn!("themes first_run_init failed: {e}");
    }
    if let Err(e) = tasty_themes::ensure_mocha_exists() {
        tracing::warn!("ensure_mocha_exists failed: {e}");
    }
    if let Err(e) = tasty_themes::rescan() {
        tracing::warn!("themes rescan failed: {e}");
    }
    let requested = appearance.theme.clone();
    tasty_themes::apply_theme(appearance, &requested);
    tasty_themes::install_global(appearance);
    if appearance.theme != requested {
        Some(requested)
    } else {
        None
    }
}

impl App {
    /// Create an AppState from a GPU state, computing grid size from the sidebar width.
    pub(crate) fn create_app_state(
        &mut self,
        gpu: &GpuState,
        sidebar_width: tasty_type_geometry::length::LogicalPx,
    ) -> crate::state::AppState {
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
        let (cols, rows) = gpu.grid_size_for_rect(&terminal_rect);

        let factory: crate::waker::SharedWakerFactory = Arc::new(
            crate::waker_factory_winit::WinitWakerFactory::new(self.view.proxy.clone()),
        );
        let waker: crate::terminal::Waker = factory.make_default_waker();

        // EngineState를 App 직속에 1회 init.
        if self.engine_state.is_none() {
            // 두 번째 main window 생성 시: 첫 engine 의 글로벌 Arc 들을 공유한다.
            // surface_registry 는 plugin_manager 가 첫 부팅 시 set 한 것과 같은
            // Arc 여야 plugin 이 register 한 surface kind 가 두 번째 윈도우에서도
            // 보임. file_format / file_handler 도 동일 — plugin contribute 한
            // file 동작이 두번째 윈도우에서 누락 안 되도록.
            //
            // 첫 부팅 시점에는 source 없음 → EngineState::new 의 기본 Arc 사용.
            let shared = self.any_main_engine().map(|src| {
                (
                    src.surface_registry.clone(),
                    src.file_format.clone(),
                    src.file_handler.clone(),
                    src.identify_worker.clone(),
                    src.preset_store.clone(),
                    src.approval_store.clone(),
                    src.telemetry_seq.clone(),
                    src.anomaly_detector.clone(),
                    src.agent_seq.clone(),
                    src.next_ids.clone(),
                )
            });

            // IdGenerator 는 EngineState::new 시점에 default workspace 만들면서
            // 첫 ID 들 발급하므로, **생성 전에** source 의 next_ids 를 주입해야
            // workspace_id/pane_id/tab_id/surface_id 충돌이 안 난다.
            let shared_ids = shared.as_ref().map(|s| s.9.clone());
            let mut engine = crate::engine_state::EngineState::new_with_ids(
                cols,
                rows,
                waker.clone(),
                shared_ids,
            )
            .expect("failed to create engine state");
            engine.waker_factory = Some(factory.clone());
            if let Some((
                surface_registry,
                file_format,
                file_handler,
                identify_worker,
                preset_store,
                approval_store,
                telemetry_seq,
                anomaly_detector,
                agent_seq,
                _next_ids,
            )) = shared
            {
                engine.surface_registry = surface_registry;
                engine.file_format = file_format;
                engine.file_handler = file_handler;
                engine.identify_worker = identify_worker;
                engine.preset_store = preset_store;
                engine.approval_store = approval_store;
                engine.telemetry_seq = telemetry_seq;
                engine.anomaly_detector = anomaly_detector;
                engine.agent_seq = agent_seq;
                // next_ids 는 위에서 이미 생성 시점에 주입됨.
            } else {
                // 첫 부팅 — identify_worker 와 preset_store 는 App proxy 가 필요.
                engine.identify_worker =
                    Some(Arc::new(crate::identify_worker::IdentifyWorker::new(
                        engine.file_format.clone(),
                        self.view.proxy.clone(),
                    )));
                engine.preset_store = Some(self.core.preset_store.clone());
            }
            #[cfg(debug_assertions)]
            {
                engine.input_simulation_enabled = self.input_simulation_enabled;
            }
            self.engine_state = Some(engine);
        }

        if self.plugin_manager.is_none() {
            let (file_format, file_handler, surface_registry) = {
                let engine = self.engine_state();
                (
                    engine.file_format.clone(),
                    engine.file_handler.clone(),
                    engine.surface_registry.clone(),
                )
            };
            let mut mgr =
                plugin::PluginManager::with_registries(factory, file_format, file_handler);
            mgr.set_surface_registry(surface_registry);
            plugin::install_builtins_if_needed(&mut mgr);
            mgr.packages = plugin::discover();
            mgr.discover_and_start();
            self.plugin_manager = Some(mgr);
        }

        let saved_layout = self.engine_state_mut().pending_layout_restore.take();
        let restored_idx_after_layout = if let Some(saved) = saved_layout {
            {
                use std::time::{Duration, Instant};
                let deadline = Instant::now() + Duration::from_millis(300);
                let needed: Vec<String> = saved.required_plugin_kinds();
                while Instant::now() < deadline {
                    if let Some(mgr) = self.plugin_manager.as_mut() {
                        mgr.pump();
                    }
                    let registered_all = {
                        let engine = self.engine_state();
                        needed
                            .iter()
                            .all(|k| engine.surface_registry.get(k).is_some())
                    };
                    if registered_all {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
            let engine = self.engine_state_mut();
            if saved.restore(engine) {
                tracing::info!("Layout restored from layout.json (deferred)");
                engine.restored_active_workspace.take()
            } else {
                None
            }
        } else {
            None
        };

        let mut state = crate::state::AppState::new(self.engine_state_mut());
        if let Some(restored_idx) = restored_idx_after_layout {
            state.switch_workspace(self.engine_state_mut(), restored_idx);
        }
        if let Some(mgr) = self.plugin_manager.as_ref() {
            state
                .tool_registry
                .set_plugin_items(mgr.plugin_tool_items());
        }
        state
    }

    /// Register a MainWindow and set it as focused.
    pub(crate) fn register_window(
        &mut self,
        gpu: GpuState,
        state: crate::state::AppState,
        engine_state: crate::engine_state::EngineState,
        window: Arc<Window>,
    ) {
        let window_id = window.id();
        let main = window::main::MainWindow::new(
            gpu,
            state,
            engine_state,
            window,
            self.view.proxy.clone(),
        );
        self.windows.insert(window_id, Box::new(main));
        self.view.focused_window_id = Some(window_id);
        if let Some(mgr) = self.plugin_manager.as_mut() {
            use tasty_plugin_protocol::EventScope;
            use tasty_plugin_protocol::events::payloads::{WindowCreated, WindowModality};
            let payload = WindowCreated {
                window_id: u64::from(window_id),
                kind: "main".to_string(),
                modality: WindowModality::Modeless,
            };
            mgr.emit_host_event("window.created", &payload, EventScope::System);
            crate::hooks::lua::fire(self.lua_engine.as_ref(), "window.create.post", &payload);
        }
    }

    /// Initialize the full app state (terminal, IPC server, etc.) after shell is confirmed.
    ///
    /// 테마 디스크 초기화·적용은 이 함수 내부에서 수행되며, 요청된 theme id 가
    /// fallback 되었으면 InfoModal 로 사용자에게 알린다.
    pub(crate) fn init_app_state(
        &mut self,
        window: Arc<Window>,
        gpu: GpuState,
        mut settings: crate::settings::Settings,
    ) {
        // state.db / memory.db 초기화는 create_app_state 이전에 반드시 호출.
        // 첫 윈도우는 `create_new_window` 를 거치지 않고 곧장 이 함수로 진입하므로
        // 여기서도 호출이 필요하다. 두 init 모두 OnceLock 기반 idempotent.
        let db_init_error = crate::db::init().err();

        let memory_config = tasty_memory::MemoryConfig {
            entry_max_bytes: settings.memory.entry_max_mb.saturating_mul(1024 * 1024),
            secret_quota_per_owner_bytes: settings
                .memory
                .secret_quota_mb_per_plugin
                .saturating_mul(1024 * 1024),
            regular_quota_total_bytes: settings
                .memory
                .regular_quota_mb_total
                .saturating_mul(1024 * 1024),
        };
        if let Err(e) = tasty_memory::init_with_config(memory_config) {
            tracing::warn!("memory.db init failed: {e}");
        }

        // Apply theme via tasty-themes (first-run init, fallback, partial accumulation, global install).
        let invalid_theme_name = boot_apply_theme(&mut settings.appearance);
        if let Err(e) = settings.save() {
            tracing::warn!("failed to persist settings after theme apply: {e}");
        }

        let mut state = self.create_app_state(&gpu, settings.appearance.sidebar_width);

        // DB 초기화 실패 알림 — create_new_window 와 동일하게 InfoModal 로 안내 후 Exit(1).
        if let Some(err) = db_init_error {
            tracing::error!("state.db init failed: {err}");
            let (key, args) = err.user_message_i18n();
            let body = match args.len() {
                0 => crate::i18n::t(key).to_string(),
                1 => crate::i18n::t_fmt(key, &args[0]),
                _ => crate::i18n::t_fmt2(key, &args[0], &args[1]),
            };
            crate::ui::info_modal::show_info_modal(
                &mut state,
                crate::ui::info_modal::InfoModal {
                    title: crate::i18n::t("db_error.title").to_string(),
                    body,
                    on_close: crate::ui::info_modal::InfoModalAction::Exit(1),
                },
            );
        }

        // Theme fallback 알림 — normalize 가 잘못된 theme 이름을 정정한 경우.
        if let Some(invalid) = invalid_theme_name {
            crate::ui::info_modal::show_info_modal(
                &mut state,
                crate::ui::info_modal::InfoModal {
                    title: crate::i18n::t("theme_error.title").to_string(),
                    body: crate::i18n::t_fmt("theme_error.body", &invalid),
                    on_close: crate::ui::info_modal::InfoModalAction::Continue,
                },
            );
        }

        self.engine.start_ipc(&self.view.proxy);
        let engine_state = self
            .engine_state
            .take()
            .expect("App.engine_state must be present to register a main window");
        self.register_window(gpu, state, engine_state, window);
        // Event Bus 1.0: `system.startup_complete`는 부팅 완료 직후 1회 발화.
        // init_app_state는 첫 윈도우 등록 시 한 번만 호출되므로 별도 once 가드 불필요.
        if let Some(mgr) = self.plugin_manager.as_mut() {
            use tasty_plugin_protocol::EventScope;
            use tasty_plugin_protocol::events::payloads::SystemStartupComplete;
            mgr.emit_host_event(
                "system.startup_complete",
                &SystemStartupComplete::default(),
                EventScope::System,
            );
        }
    }

    /// Create a new window with its own terminal.
    pub(crate) fn create_new_window(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
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

        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .expect("failed to create window"),
        );
        window.set_ime_allowed(true);

        // state.db 초기화. 실패하면 InfoModal로 안내 후 종료(Exit 1).
        // create_app_state 이전에 호출해야 plugin/recent_files 등이 정상 동작.
        let db_init_error = crate::db::init().err();

        let mut settings = crate::settings::Settings::load();
        // 모든 enum-like 필드 정규화. invalid 가 있었으면 즉시 파일에 반영해서
        // 다음 부팅에 같은 popup / 잘못된 동작이 재발하지 않게 한다.
        let normalize_report = settings.normalize();
        if normalize_report.changed {
            if let Err(e) = settings.save() {
                tracing::warn!("failed to persist normalized settings: {e}");
            }
        }

        // memory.db 초기화. state.db와 독립 파일(~/.tasty/memory.db). 현재는
        // 에이전트 memory.* IPC만 의존하므로 실패해도 앱을 종료시키지 않는다 —
        // 핸들러가 호출 시점에 "store not initialized"를 응답한다. 1.5에서
        // surface.meta.* 포워딩이 들어가면 정책 재검토.
        let memory_config = tasty_memory::MemoryConfig {
            entry_max_bytes: settings.memory.entry_max_mb.saturating_mul(1024 * 1024),
            secret_quota_per_owner_bytes: settings
                .memory
                .secret_quota_mb_per_plugin
                .saturating_mul(1024 * 1024),
            regular_quota_total_bytes: settings
                .memory
                .regular_quota_mb_total
                .saturating_mul(1024 * 1024),
        };
        if let Err(e) = tasty_memory::init_with_config(memory_config) {
            tracing::warn!("memory.db init failed: {e}");
        }

        // Apply theme via tasty-themes (first-run init, fallback, partial accumulation, global install).
        let invalid_theme_name = boot_apply_theme(&mut settings.appearance);
        if invalid_theme_name.is_some() || normalize_report.changed {
            if let Err(e) = settings.save() {
                tracing::warn!("failed to persist settings after theme apply: {e}");
            }
        }
        let gpu = pollster::block_on(crate::gpu::GpuState::new(
            window.clone(),
            &settings.appearance,
            self.view.proxy.clone(),
        ))
        .expect("failed to initialize GPU");

        // Reuse parked state if available (restoring previous session)
        let (mut state, parked_engine) = if !self.parked_states.is_empty() {
            let parked = self.parked_states.remove(0);
            tracing::info!(
                "restoring parked state, {} remaining",
                self.parked_states.len()
            );
            let (st, eng) = parked;
            (st, Some(eng))
        } else {
            let st = self.create_app_state(&gpu, settings.appearance.sidebar_width);
            (st, None)
        };

        // 새 윈도우의 engine: parked 가 있으면 그쪽을 재사용, 없으면 App.engine_state
        // 를 take. create_app_state 가 항상 self.engine_state 를 set 하므로
        // 두 번째 main window 생성 시에도 새 engine 이 만들어져 들어와 있음
        // (글로벌 Arc 들은 첫 engine 과 공유 — create_app_state 의 shared 분기 참조).
        let mut engine_state = if let Some(e) = parked_engine {
            e
        } else {
            self.engine_state
                .take()
                .expect("App.engine_state must be present to register a main window")
        };

        // Ensure at least one workspace exists for the new window
        state.ensure_workspace_exists(&mut engine_state);

        // DB 초기화 실패 알림. 가장 먼저 푸시해서 큐 head에 둠 → [확인] 시 Exit(1).
        if let Some(err) = db_init_error {
            tracing::error!("state.db init failed: {err}");
            let (key, args) = err.user_message_i18n();
            let body = match args.len() {
                0 => crate::i18n::t(key).to_string(),
                1 => crate::i18n::t_fmt(key, &args[0]),
                _ => crate::i18n::t_fmt2(key, &args[0], &args[1]),
            };
            crate::ui::info_modal::show_info_modal(
                &mut state,
                crate::ui::info_modal::InfoModal {
                    title: crate::i18n::t("db_error.title").to_string(),
                    body,
                    on_close: crate::ui::info_modal::InfoModalAction::Exit(1),
                },
            );
        }

        // Theme fallback 알림 (잘못된 theme 이름이었던 경우).
        if let Some(invalid) = invalid_theme_name {
            crate::ui::info_modal::show_info_modal(
                &mut state,
                crate::ui::info_modal::InfoModal {
                    title: crate::i18n::t("theme_error.title").to_string(),
                    body: crate::i18n::t_fmt("theme_error.body", &invalid),
                    on_close: crate::ui::info_modal::InfoModalAction::Continue,
                },
            );
        }

        self.register_window(gpu, state, engine_state, window);
        tracing::info!("created new window {:?}", self.view.focused_window_id);
    }
}
