//! `App` 의 윈도우 라이프사이클 메서드.
//!
//! - `create_app_state`: GPU 상태 + 사이드바 폭으로부터 새 `AppState` 를 만든다.
//!   첫 호출 시 plugin manager 도 초기화하며, `pending_layout_restore` 가 있으면
//!   plugin 등록을 짧게 기다린 뒤 layout 을 복원한다.
//! - `register_window`: 만들어진 `MainView` 를 hash 에 등록 + focused 로 설정 +
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
    // host UI zoom 을 항상 실어 부팅 직후 steady state 도 올바른 배율로 설치한다.
    let ui_zoom = appearance.ui_scale_factor();
    tasty_themes::install_global_with_zoom(appearance, ui_zoom);
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

        // CoreState를 App 직속에 1회 init.
        if self.core_state.is_none() {
            // 두 번째 main window 생성 시: 첫 engine 의 글로벌 Arc 들을 공유한다.
            // surface_registry 는 plugin_manager 가 첫 부팅 시 set 한 것과 같은
            // Arc 여야 plugin 이 register 한 surface kind 가 두 번째 윈도우에서도
            // 보임. file_format / file_handler 도 동일 — plugin contribute 한
            // file 동작이 두번째 윈도우에서 누락 안 되도록.
            //
            // 첫 부팅 시점에는 source 없음 → CoreState::new 의 기본 Arc 사용.
            // preset_store 는 Core 가 유일 owner (D.3.C.M.2) — engine 에는 더 이상 없다.
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

            // IdGenerator 는 CoreState::new 시점에 default workspace 만들면서
            // 첫 ID 들 발급하므로, **생성 전에** source 의 next_ids 를 주입해야
            // workspace_id/pane_id/tab_id/surface_id 충돌이 안 난다.
            let shared_ids = shared.as_ref().map(|s| s.8.clone());
            let mut engine = crate::core::CoreState::new_with_ids(
                cols,
                rows,
                waker.clone(),
                shared_ids,
                self.core.memory_arc(),
            )
            .expect("failed to create engine state");
            engine.waker_factory = Some(factory.clone());
            if let Some((
                surface_registry,
                file_format,
                file_handler,
                identify_worker,
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
                engine.approval_store = approval_store;
                engine.telemetry_seq = telemetry_seq;
                engine.anomaly_detector = anomaly_detector;
                engine.agent_seq = agent_seq;
                // next_ids 는 위에서 이미 생성 시점에 주입됨.
            } else {
                // 첫 부팅 — identify_worker 는 App proxy 가 필요.
                engine.identify_worker =
                    Some(Arc::new(crate::identify_worker::IdentifyWorker::new(
                        engine.file_format.clone(),
                        self.view.proxy.clone(),
                    )));
            }
            #[cfg(debug_assertions)]
            {
                engine.input_simulation_enabled = self.input_simulation_enabled;
            }
            self.core_state = Some(engine);
        }

        if self.plugin_manager.is_none() {
            let (file_format, file_handler, surface_registry) = {
                let engine = self.core_state();
                (
                    engine.file_format.clone(),
                    engine.file_handler.clone(),
                    engine.surface_registry.clone(),
                )
            };
            let mut mgr =
                plugin::PluginManager::with_registries(factory, file_format, file_handler);
            mgr.set_surface_registry(surface_registry);
            mgr.set_i18n_registrar(std::sync::Arc::new(crate::i18n::BinI18nRegistrar));
            plugin::install_builtins_if_needed(&mut mgr);
            mgr.packages = plugin::discover();
            mgr.discover_and_start();
            self.plugin_manager = Some(mgr);
        }

        // pending_layout_restore 가 있으면: wait-for-plugin loop 를 거쳐 등록
        // 대기 → `DomainIntent::ApplyPendingLayoutRestore` 발화. Intent 본문
        // (Core::apply) 안에서 take + restore + restored_active_workspace 추출이
        // 한 번에 일어난다 — caller 는 events 만 검사.
        //
        // Intent 큐 우회 직접 apply — bootstrap context (main loop 진입 전) 라
        // 큐 drain 이 일어나지 않는다. D.3.C.D.4.c 결정.
        let restored_idx_after_layout = if self.core_state().pending_layout_restore.is_some() {
            // wait-for-plugin: required_plugin_kinds 만 peek (take 안 함).
            // Intent 본문이 단일 take 를 보장.
            {
                use std::time::{Duration, Instant};
                let needed: Vec<String> = self
                    .core_state()
                    .pending_layout_restore
                    .as_ref()
                    .map(|s| s.required_plugin_kinds())
                    .unwrap_or_default();
                let deadline = Instant::now() + Duration::from_millis(300);
                while Instant::now() < deadline {
                    let hello_pairs = if let Some(mgr) = self.plugin_manager.as_mut() {
                        mgr.pump()
                    } else {
                        Vec::new()
                    };
                    self.finalize_plugin_hello(hello_pairs);
                    let registered_all = {
                        let engine = self.core_state();
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
                        tracing::info!("Layout restored from layout.json (deferred)");
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

            // ApplyPendingLayoutRestore 가 RemoteSurface 들을 생성하고
            // `HostCmd::RemoteSurfaceRestored` 를 큐잉했다. pump 를 추가로 돌려
            // 송신 → plugin 응답 round-trip 이 끝날 때까지 대기한다. 이게 끝나야
            // RemoteSurface 의 snapshot_cache 가 plugin 의 최신 값으로 갱신된
            // 상태로 main loop 에 진입 — 사용자 동작 race 가 사라진다. carry 값이
            // 이미 안전망 역할을 하므로 (1) layout.json 오염은 이 wait 와 무관하게
            // 차단된 상태이고, 이 wait 는 부팅 직후 사용자 동작이 응답으로 덮어
            // 씌워지는 깜박임/덮어쓰기를 추가로 방지하는 목적.
            //
            // deadline: plugin 이 panic/hang 등으로 영영 응답 안 보내는 케이스
            // 보호. 초과해도 (1) carry 덕에 layout 손상은 없음.
            {
                use std::time::{Duration, Instant};
                let deadline = Instant::now() + Duration::from_millis(500);
                while Instant::now() < deadline {
                    let still_pending = if let Some(mgr) = self.plugin_manager.as_mut() {
                        let hello_pairs = mgr.pump();
                        if !hello_pairs.is_empty() {
                            self.finalize_plugin_hello(hello_pairs);
                        }
                        self.plugin_manager
                            .as_ref()
                            .is_some_and(|m| m.has_pending_surface_restores())
                    } else {
                        false
                    };
                    if !still_pending {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
            }

            restored
        } else {
            None
        };

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
        }
        state
    }

    /// Register a MainView and set it as focused.
    pub(crate) fn register_window(
        &mut self,
        gpu: GpuState,
        state: crate::state::AppState,
        core_state: crate::core::CoreState,
        window: Arc<Window>,
    ) {
        let window_id = window.id();
        let main =
            window::main::MainView::new(gpu, state, core_state, window, self.view.proxy.clone());
        self.view.views.insert(window_id, Box::new(main));
        self.view.focused_view_id = Some(window_id);
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
        // state.db 초기화는 create_app_state 이전에 반드시 호출. memory.db 는
        // boot 가 App::new 이전에 이미 초기화함 (D.3.C.M.1) — 여기서 별도 호출하지 않는다.
        let db_init_error = crate::db::init().err();

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
            crate::adapters::ui::info_modal::show_info_modal(
                &mut state,
                crate::adapters::ui::info_modal::InfoModal {
                    title: crate::i18n::t("db_error.title").to_string(),
                    body,
                    on_close: crate::adapters::ui::info_modal::InfoModalAction::Exit(1),
                },
            );
        }

        // Theme fallback 알림 — normalize 가 잘못된 theme 이름을 정정한 경우.
        if let Some(invalid) = invalid_theme_name {
            crate::adapters::ui::info_modal::show_info_modal(
                &mut state,
                crate::adapters::ui::info_modal::InfoModal {
                    title: crate::i18n::t("theme_error.title").to_string(),
                    body: crate::i18n::t_fmt("theme_error.body", &invalid),
                    on_close: crate::adapters::ui::info_modal::InfoModalAction::Continue,
                },
            );
        }

        let ipc_proxy = self.view.proxy.clone();
        let ipc_waker: crate::ipc::server::IpcWaker = std::sync::Arc::new(move || {
            crate::shortcuts::send_app_event(&ipc_proxy, crate::AppEvent::IpcReady);
        });
        let stream_proxy = self.view.proxy.clone();
        let stream_waker: crate::ipc::server::IpcWaker = std::sync::Arc::new(move || {
            crate::shortcuts::send_app_event(&stream_proxy, crate::AppEvent::StreamReady);
        });
        let stream_ctx = crate::adapters::production::stream_hub::StreamContext {
            hub: self.stream_hub.clone(),
            inbound_tx: self.stream_inbound_tx.clone(),
            waker: stream_waker,
        };
        if let Some(injector) = self.hub.start_ipc(ipc_waker, stream_ctx) {
            self.core.set_host_ipc_injector(injector);
        }
        let mut core_state = self
            .core_state
            .take()
            .expect("App.core_state must be present to register a main window");
        // attach/detach 단계 3: force-detach 통지가 stream client 로 push 되도록
        // IPC 서버와 동일한 StreamHub 를 attach registry 에 주입.
        core_state.attach.set_notifier(self.stream_hub.clone());
        self.register_window(gpu, state, core_state, window);
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
        // CSD: macOS 는 fullsize-content-view(네이티브 신호등 유지). 그 외 OS no-op.
        attrs = crate::platform::window_chrome::apply_csd_attributes(attrs);

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
        if normalize_report.changed
            && let Err(e) = settings.save()
        {
            tracing::warn!("failed to persist normalized settings: {e}");
        }

        // memory.db 는 boot 가 App::new 이전에 초기화함 (D.3.C.M.1).

        // Apply theme via tasty-themes (first-run init, fallback, partial accumulation, global install).
        let invalid_theme_name = boot_apply_theme(&mut settings.appearance);
        if (invalid_theme_name.is_some() || normalize_report.changed)
            && let Err(e) = settings.save()
        {
            tracing::warn!("failed to persist settings after theme apply: {e}");
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

        // 새 윈도우의 engine: parked 가 있으면 그쪽을 재사용, 없으면 App.core_state
        // 를 take. create_app_state 가 항상 self.core_state 를 set 하므로
        // 두 번째 main window 생성 시에도 새 engine 이 만들어져 들어와 있음
        // (글로벌 Arc 들은 첫 engine 과 공유 — create_app_state 의 shared 분기 참조).
        let mut core_state = if let Some(e) = parked_engine {
            e
        } else {
            self.core_state
                .take()
                .expect("App.core_state must be present to register a main window")
        };

        // Ensure at least one workspace exists for the new window
        if core_state.workspaces.is_empty() {
            match self.core.create_default_workspace(&mut core_state) {
                Ok(idx) => state.active_workspace = idx,
                Err(e) => tracing::error!("bootstrap workspace failed: {e}"),
            }
        }

        // DB 초기화 실패 알림. 가장 먼저 푸시해서 큐 head에 둠 → [확인] 시 Exit(1).
        if let Some(err) = db_init_error {
            tracing::error!("state.db init failed: {err}");
            let (key, args) = err.user_message_i18n();
            let body = match args.len() {
                0 => crate::i18n::t(key).to_string(),
                1 => crate::i18n::t_fmt(key, &args[0]),
                _ => crate::i18n::t_fmt2(key, &args[0], &args[1]),
            };
            crate::adapters::ui::info_modal::show_info_modal(
                &mut state,
                crate::adapters::ui::info_modal::InfoModal {
                    title: crate::i18n::t("db_error.title").to_string(),
                    body,
                    on_close: crate::adapters::ui::info_modal::InfoModalAction::Exit(1),
                },
            );
        }

        // Theme fallback 알림 (잘못된 theme 이름이었던 경우).
        if let Some(invalid) = invalid_theme_name {
            crate::adapters::ui::info_modal::show_info_modal(
                &mut state,
                crate::adapters::ui::info_modal::InfoModal {
                    title: crate::i18n::t("theme_error.title").to_string(),
                    body: crate::i18n::t_fmt("theme_error.body", &invalid),
                    on_close: crate::adapters::ui::info_modal::InfoModalAction::Continue,
                },
            );
        }

        self.register_window(gpu, state, core_state, window);
        tracing::info!("created new window {:?}", self.view.focused_view_id);
    }
}
