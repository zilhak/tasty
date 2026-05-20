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

impl App {
    /// Create an AppState from a GPU state, computing grid size from the sidebar width.
    pub(crate) fn create_app_state(
        &mut self,
        gpu: &GpuState,
        sidebar_width: crate::model::LogicalPx,
    ) -> crate::state::AppState {
        let sf = gpu.scale_factor();
        let size = gpu.size();
        let sidebar_w = sidebar_width.to_physical(sf);
        let terminal_rect = crate::model::Rect {
            x: sidebar_w,
            y: crate::model::PhysicalPx(0.0),
            width: (crate::model::PhysicalPx(size.width as f32) - sidebar_w)
                .max(crate::model::PhysicalPx(1.0)),
            height: crate::model::PhysicalPx(size.height as f32),
        };
        let (cols, rows) = gpu.grid_size_for_rect(&terminal_rect);

        let factory: tasty_core::SharedWakerFactory = Arc::new(
            crate::waker_factory_winit::WinitWakerFactory::new(self.engine.proxy.clone()),
        );
        let waker: crate::terminal::Waker = factory.make_default_waker();

        let mut state =
            crate::state::AppState::new(cols, rows, waker).expect("failed to create app state");
        state.engine.waker_factory = Some(factory.clone());
        // 비동기 파일 식별 worker. file_format Arc 를 공유하므로 plugin contribute /
        // user reload 변경이 worker 호출에도 그대로 반영된다.
        state.engine.identify_worker = Some(Arc::new(crate::identify_worker::IdentifyWorker::new(
            state.engine.file_format.clone(),
            self.engine.proxy.clone(),
        )));

        // 첫 윈도우 생성 시 plugin manager 한 번만 초기화.
        if self.plugin_manager.is_none() {
            // EngineState 와 같은 file_format / file_handler Arc 를 공유해
            // plugin enable/disable 시 EngineState 가 보유한 registry 가 그대로 갱신되도록 한다.
            let mut mgr = plugin::PluginManager::with_registries(
                factory,
                state.engine.file_format.clone(),
                state.engine.file_handler.clone(),
            );
            mgr.set_surface_registry(state.engine.surface_registry.clone());
            // 기본 제공 플러그인이 설치되지 않았으면 번들에서 복사. 사용자가
            // 명시적으로 제거한 항목 (`removed_builtins`)은 건드리지 않는다.
            plugin::install_builtins_if_needed(&mut mgr);
            mgr.packages = plugin::discover();
            mgr.discover_and_start();
            state
                .tool_registry
                .set_plugin_items(mgr.plugin_tool_items());
            self.plugin_manager = Some(mgr);
        }

        // pending_layout_restore가 있으면, plugin이 surface_kinds를 등록할 시간을
        // 잠깐 주고 적용한다. 시간 내에 hello가 도착하지 않은 plugin이 제공하는
        // kind는 복원에서 일단 skip되며, 추후 정상 흐름으로 새로 만들 수 있다.
        if let Some(saved) = state.engine.pending_layout_restore.take() {
            if let Some(mgr) = self.plugin_manager.as_mut() {
                use std::time::{Duration, Instant};
                let deadline = Instant::now() + Duration::from_millis(300);
                let needed: Vec<String> = saved.required_plugin_kinds();
                while Instant::now() < deadline {
                    mgr.pump();
                    let registered_all = needed
                        .iter()
                        .all(|k| state.engine.surface_registry.get(k).is_some());
                    if registered_all {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
            if saved.restore(&mut state.engine) {
                tracing::info!("Layout restored from layout.json (deferred)");
                // AppState::new 시점에는 layout이 아직 복원되지 않아 state.active_workspace=0.
                // restore가 끝난 지금 실제 활성 인덱스로 sync해야 사용자가 보는 화면이
                // 일치한다 (sync 없으면 첫 화면이 비활성 workspace[0]의 deferred
                // placeholder들로 채워진다).
                if let Some(restored_idx) = state.engine.restored_active_workspace.take() {
                    state.switch_workspace(restored_idx);
                }
            }
        }
        #[cfg(debug_assertions)]
        {
            state.engine.input_simulation_enabled = self.input_simulation_enabled;
        }
        // App 의 preset_store Arc 를 EngineState 에 공유. apply popup / 우클릭 저장 등이
        // MainWindow 컨텍스트에서 lock 한 번으로 직접 접근할 수 있게 한다.
        state.engine.preset_store = Some(self.engine.preset_store.clone());
        state
    }

    /// Register a MainWindow and set it as focused.
    pub(crate) fn register_window(
        &mut self,
        gpu: GpuState,
        state: crate::state::AppState,
        window: Arc<Window>,
    ) {
        let window_id = window.id();
        let main = window::main::MainWindow::new(gpu, state, window, self.engine.proxy.clone());
        self.windows.insert(window_id, Box::new(main));
        self.engine.focused_window_id = Some(window_id);
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
    pub(crate) fn init_app_state(
        &mut self,
        window: Arc<Window>,
        gpu: GpuState,
        settings: crate::settings::Settings,
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

        self.engine.start_ipc();
        self.register_window(gpu, state, window);
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

        // Apply saved theme preset at startup. theme 이름이 preset에 없으면
        // catppuccin-mocha로 fallback하고 사용자에게 InfoModal로 알린다.
        let presets = crate::theme::presets();
        let invalid_theme_name =
            if let Some(preset) = presets.iter().find(|p| p.id == settings.appearance.theme) {
                crate::theme::set_theme(preset.theme);
                None
            } else {
                let invalid = settings.appearance.theme.clone();
                let fallback_id = "catppuccin-mocha";
                settings.appearance.theme = fallback_id.to_string();
                if let Some(default_preset) = presets.iter().find(|p| p.id == fallback_id) {
                    crate::theme::set_theme(default_preset.theme);
                }
                Some(invalid)
            };
        let gpu = pollster::block_on(crate::gpu::GpuState::new(
            window.clone(),
            &settings.appearance,
            self.engine.proxy.clone(),
        ))
        .expect("failed to initialize GPU");

        // Reuse parked state if available (restoring previous session)
        let mut state = if !self.parked_states.is_empty() {
            let parked = self.parked_states.remove(0);
            tracing::info!(
                "restoring parked state with {} workspace(s), {} remaining",
                parked.engine.workspaces.len(),
                self.parked_states.len()
            );
            parked
        } else {
            self.create_app_state(&gpu, settings.appearance.sidebar_width)
        };

        // Ensure at least one workspace exists for the new window
        state.ensure_workspace_exists();

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

        self.register_window(gpu, state, window);
        tracing::info!("created new window {:?}", self.engine.focused_window_id);
    }
}
