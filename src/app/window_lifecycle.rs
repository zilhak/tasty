//! `App` 의 윈도우 라이프사이클 메서드.
//!
//! - `create_app_state`: GPU 상태 + 사이드바 폭으로부터 새 `AppState` 를 만든다.
//!   첫 호출 시 plugin manager 도 초기화하며, `pending_layout_restore` 가 있으면
//!   plugin 등록을 짧게 기다린 뒤 layout 을 복원한다.
//! - `register_window`: 만들어진 `MainView` 를 hash 에 등록 + focused 로 설정 +
//!   `window.created` host event / lua hook 발화.
//! - `create_new_window`: 다중 윈도우용 — 새 winit window + GPU + AppState (parked 우선) + 모달 안내.
//!
//! 첫 부팅(첫 윈도우)은 이 모듈의 동기 함수 대신 부팅 상태 머신
//! (`boot_machine.rs` — `begin_boot` → phase 스텝 → `finish_boot`)이 담당하며,
//! 여기의 `build_engine_and_plugins`(워커 본문) / `boot_pump_step_*` /
//! `boot_apply_pending_layout_restore` / `assemble_app_state` 를 공유한다.
//! `ensure_engine_and_plugins` 는 동기 wrapper — 동기 경로(다중 창)와 부팅
//! 머신의 워커 실패 fallback 이 쓴다.

use std::sync::Arc;

use winit::window::Window;

use crate::app::App;
use crate::gpu::GpuState;
use crate::{plugin, window};

/// 부팅 흐름의 테마 적용. `tasty-themes` 의 디스크 초기화 + 전역 Theme 설치를 한 단계로 묶는다.
///
/// 반환: `settings.appearance.theme` 가 디스크/캐시에 없어 mocha 로 fallback 된 경우 원래 요청 id.
/// (호출자가 InfoModal 로 사용자에게 알린다.)
pub(super) fn boot_apply_theme(appearance: &mut tasty_settings::AppearanceSettings) -> Option<String> {
    if let Err(e) = tasty_themes::first_run_init() {
        tracing::warn!("themes first_run_init failed: {e}");
    }
    // 빌트인 테마(앱 소유)를 임베드 정본과 동기화 — 옛 스키마/색의 디스크 복사본을
    // 갱신한다. mocha 정본 보장도 겸한다(ensure_mocha_exists 의 상위 집합).
    if let Err(e) = tasty_themes::sync_builtin_themes() {
        tracing::warn!("sync_builtin_themes failed: {e}");
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

/// 부팅 시 터미널 그리드 크기 계산 — GPU cell metrics + 사이드바 폭 의존이라
/// 메인 스레드 몫이다. `ensure_engine_and_plugins`(동기)와 부팅 상태 머신의
/// 워커 spawn(cols/rows 를 정수 2개로 뽑아 전달)이 공유한다.
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

/// 엔진(CoreState)+plugin manager 원자 초기화(T2.6·T3)의 App-free 본문 —
/// **첫 부팅 전용** (두 번째 창의 글로벌 Arc 공유 분기는
/// `ensure_engine_and_plugins` 에 남아 있다). `App` 참조가 없어 부팅 상태
/// 머신이 워커 스레드에서 실행하며(`boot_machine.rs` 의 WaitingEngine),
/// 동기 wrapper 의 첫 부팅 분기도 같은 본문을 쓴다.
///
/// panic: `CoreState::new_with_ids` 실패 시 expect — 워커에서 돌 때는 결과
/// 채널 drop 으로 표면화되고 WaitingEngine 의 disconnect fallback 이 받는다.
pub(super) fn build_engine_and_plugins(
    cols: usize,
    rows: usize,
    factory: crate::waker::SharedWakerFactory,
    proxy: winit::event_loop::EventLoopProxy<crate::AppEvent>,
    memory: std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>>,
    #[cfg(debug_assertions)] input_simulation_enabled: bool,
) -> (crate::core::CoreState, plugin::PluginManager) {
    let engine = build_core_state_first_boot(
        cols,
        rows,
        factory.clone(),
        proxy,
        memory,
        #[cfg(debug_assertions)]
        input_simulation_enabled,
    );
    let mgr = build_plugin_manager(factory, &engine);
    (engine, mgr)
}

/// 첫 부팅의 CoreState 생성 (T2.6 계측 포함). 공유 source 없음 —
/// `CoreState::new` 의 기본 Arc 사용. preset_store 는 Core 가 유일 owner
/// (D.3.C.M.2) — engine 에는 더 이상 없다.
fn build_core_state_first_boot(
    cols: usize,
    rows: usize,
    factory: crate::waker::SharedWakerFactory,
    proxy: winit::event_loop::EventLoopProxy<crate::AppEvent>,
    memory: std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>>,
    #[cfg(debug_assertions)] input_simulation_enabled: bool,
) -> crate::core::CoreState {
    // layout.json 로드 + scrollback orphan GC 등 디스크 I/O 포함 — T2↔T3
    // 갭의 두 번째 기여자라 별도 계측 (첫 번째는 begin_boot 의 db+theme).
    let t_engine = std::time::Instant::now();
    let waker: crate::terminal::Waker = factory.make_default_waker();
    let mut engine = crate::core::CoreState::new_with_ids(cols, rows, waker, None, memory)
        .expect("failed to create engine state");
    engine.waker_factory = Some(factory);
    // 첫 부팅 — identify_worker 는 App proxy 가 필요.
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
        "T2.6 engine_init (CoreState::new_with_ids + layout.json load)"
    );
    engine
}

/// PluginManager 생성 + builtin 설치·discovery·spawn (T3a·T3b 계측 포함).
/// engine 의 registry Arc 들을 공유해 만든다 — App-free.
fn build_plugin_manager(
    factory: crate::waker::SharedWakerFactory,
    engine: &crate::core::CoreState,
) -> plugin::PluginManager {
    let mut mgr = plugin::PluginManager::with_registries(
        factory,
        engine.file_format.clone(),
        engine.file_handler.clone(),
    );
    mgr.set_surface_registry(engine.surface_registry.clone());
    mgr.set_i18n_registrar(std::sync::Arc::new(crate::i18n::BinI18nRegistrar));
    // 공유 훅 핸들러 레지스트리(전역 싱글턴) port 주입 — plugin enable/disable
    // 시 `[[contributes.hook_handler]]` 를 등록/해제한다(S11).
    mgr.set_hook_handler_registry(std::sync::Arc::new(
        crate::hook_handler::HostHookHandlerPort,
    ));
    // T3 은 discovery 와 spawn 을 나눠 찍는다 — 4부 escalate 확정 시 어느
    // 쪽이 병목인지 판단하기 위함.
    let t3 = std::time::Instant::now();
    plugin::install_builtins_if_needed(&mut mgr);
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

impl App {
    /// Create an AppState from a GPU state, computing grid size from the sidebar width.
    ///
    /// 동기 경로 (다중 창 `create_new_window` 등). 첫 부팅은 이 함수 대신 부팅
    /// 상태 머신(`boot_machine.rs`)이 같은 하위 단계(`ensure_engine_and_plugins` /
    /// `boot_pump_step_*` / `boot_apply_pending_layout_restore` / `assemble_app_state`)
    /// 를 프레임 단위로 나눠 태운다 — 의미론은 이 함수와 동일해야 한다.
    pub(crate) fn create_app_state(
        &mut self,
        gpu: &GpuState,
        sidebar_width: tasty_type_geometry::length::LogicalPx,
    ) -> crate::state::AppState {
        self.ensure_engine_and_plugins(gpu, sidebar_width);

        // pending_layout_restore 가 있으면: wait-for-plugin loop 를 거쳐 등록
        // 대기 → `DomainIntent::ApplyPendingLayoutRestore` 발화. Intent 본문
        // (Core::apply) 안에서 take + restore + restored_active_workspace 추출이
        // 한 번에 일어난다 — caller 는 events 만 검사.
        //
        // Intent 큐 우회 직접 apply — bootstrap context (main loop 진입 전) 라
        // 큐 drain 이 일어나지 않는다. D.3.C.D.4.c 결정.
        let restored_idx_after_layout = if self.core_state().pending_layout_restore.is_some() {
            self.boot_wait_for_required_plugin_kinds();
            let restored = self.boot_apply_pending_layout_restore();
            self.boot_wait_for_remote_surface_restores();
            restored
        } else {
            None
        };

        self.assemble_app_state(restored_idx_after_layout)
    }

    /// 엔진(CoreState)·plugin manager 의 원자 초기화 — `create_app_state` 선두 절반.
    /// 두 블록 모두 `is_none` 가드라 재호출은 no-op (다중 창 경로 안전).
    ///
    /// 동기 wrapper — 첫 부팅 본문은 App-free `build_engine_and_plugins`(부팅
    /// 상태 머신의 워커와 동일 본문)로 위임하고, 여기는 두 번째 main window 의
    /// 글로벌 Arc 공유 분기 + self 장착을 담당한다. 호출자: 동기 경로
    /// (`create_app_state` / `create_new_window`) + 부팅 상태 머신의 워커
    /// disconnect fallback (T2.6·T3 계측 포함).
    pub(super) fn ensure_engine_and_plugins(
        &mut self,
        gpu: &GpuState,
        sidebar_width: tasty_type_geometry::length::LogicalPx,
    ) {
        let (cols, rows) = boot_grid_size(gpu, sidebar_width);
        let factory: crate::waker::SharedWakerFactory = Arc::new(
            crate::waker_factory_winit::WinitWakerFactory::new(self.view.proxy.clone()),
        );

        // CoreState를 App 직속에 1회 init.
        if self.core_state.is_none() {
            // 두 번째 main window 생성 시: 첫 engine 의 글로벌 Arc 들을 공유한다.
            // surface_registry 는 plugin_manager 가 첫 부팅 시 set 한 것과 같은
            // Arc 여야 plugin 이 register 한 surface kind 가 두 번째 윈도우에서도
            // 보임. file_format / file_handler 도 동일 — plugin contribute 한
            // file 동작이 두번째 윈도우에서 누락 안 되도록.
            //
            // 첫 부팅 시점에는 source 없음 → App-free 본문
            // (`build_core_state_first_boot`)이 CoreState::new 의 기본 Arc 사용.
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
                // layout.json 로드 + scrollback orphan GC 등 디스크 I/O 포함 (T2.6).
                let t_engine = std::time::Instant::now();
                // IdGenerator 는 CoreState::new 시점에 default workspace 만들면서
                // 첫 ID 들 발급하므로, **생성 전에** source 의 next_ids 를 주입해야
                // workspace_id/pane_id/tab_id/surface_id 충돌이 안 난다.
                let waker: crate::terminal::Waker = factory.make_default_waker();
                let mut engine = crate::core::CoreState::new_with_ids(
                    cols,
                    rows,
                    waker,
                    Some(next_ids),
                    self.core.memory_arc(),
                )
                .expect("failed to create engine state");
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
                    "T2.6 engine_init (CoreState::new_with_ids + layout.json load)"
                );
                engine
            } else {
                build_core_state_first_boot(
                    cols,
                    rows,
                    factory.clone(),
                    self.view.proxy.clone(),
                    self.core.memory_arc(),
                    #[cfg(debug_assertions)]
                    self.input_simulation_enabled,
                )
            };
            self.core_state = Some(engine);
        }

        if self.plugin_manager.is_none() {
            let mgr = build_plugin_manager(factory, self.core_state());
            self.plugin_manager = Some(mgr);
        }
    }

    /// `DomainIntent::ApplyPendingLayoutRestore` 1회 apply (T5 계측 포함).
    /// take + restore + restored_active_workspace 추출은 Intent 본문(Core::apply)
    /// 안에서 한 번에 일어난다 — 단일 take 보장. 반환: 복원된 활성 workspace idx.
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
        tracing::info!(
            target: "tasty::boot",
            ms = t5.elapsed().as_secs_f64() * 1000.0,
            "T5 layout_apply (ApplyPendingLayoutRestore)"
        );
        restored
    }

    /// AppState 조립 — `create_app_state` 의 마지막 절반. 부팅 상태 머신의 Ready
    /// 합류(finish_boot)와 동기 경로가 공유한다.
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
        }
        state
    }

    /// wait-for-plugin: pending layout restore 가 요구하는 plugin surface kind
    /// 들이 등록될 때까지 pump + sleep 폴링 (deadline 300ms).
    /// `required_plugin_kinds` 만 peek (take 안 함) — Intent 본문이 단일 take 를
    /// 보장. T4 부팅 계측 포함 (탈출 사유: satisfied / deadline).
    fn boot_wait_for_required_plugin_kinds(&mut self) {
        use std::time::{Duration, Instant};
        let needed = self.boot_required_plugin_kinds();
        let t4 = Instant::now();
        let deadline = t4 + Duration::from_millis(300);
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
            "T4 layout_wait_plugins (deadline 300ms)"
        );
    }

    /// pending layout restore 가 요구하는 plugin surface kind 목록 peek (take 안 함).
    pub(super) fn boot_required_plugin_kinds(&self) -> Vec<String> {
        self.core_state()
            .pending_layout_restore
            .as_ref()
            .map(|s| s.required_plugin_kinds())
            .unwrap_or_default()
    }

    /// wait-for-plugin 1스텝: pump → `finalize_plugin_hello` → 필요 kind 전수 등록
    /// 확인. 동기 루프(위)와 부팅 상태 머신의 WaitingPlugins 스텝이 공유한다.
    /// 반환: 필요 kind 가 전부 등록됐는가 (satisfied).
    pub(super) fn boot_pump_step_plugins_registered(&mut self, needed: &[String]) -> bool {
        let hello_pairs = if let Some(mgr) = self.plugin_manager.as_mut() {
            mgr.pump()
        } else {
            Vec::new()
        };
        self.finalize_plugin_hello(hello_pairs);
        let engine = self.core_state();
        needed
            .iter()
            .all(|k| engine.surface_registry.get(k).is_some())
    }

    /// ApplyPendingLayoutRestore 가 RemoteSurface 들을 생성하고
    /// `HostCmd::RemoteSurfaceRestored` 를 큐잉했다. pump 를 추가로 돌려
    /// 송신 → plugin 응답 round-trip 이 끝날 때까지 대기한다. 이게 끝나야
    /// RemoteSurface 의 snapshot_cache 가 plugin 의 최신 값으로 갱신된
    /// 상태로 main loop 에 진입 — 사용자 동작 race 가 사라진다. carry 값이
    /// 이미 안전망 역할을 하므로 (1) layout.json 오염은 이 wait 와 무관하게
    /// 차단된 상태이고, 이 wait 는 부팅 직후 사용자 동작이 응답으로 덮어
    /// 씌워지는 깜박임/덮어쓰기를 추가로 방지하는 목적.
    ///
    /// deadline: plugin 이 panic/hang 등으로 영영 응답 안 보내는 케이스
    /// 보호. 초과해도 (1) carry 덕에 layout 손상은 없음.
    /// T6 부팅 계측 포함 (탈출 사유: satisfied / deadline).
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

    /// RemoteSurface 복원 round-trip 대기 1스텝. **pump 는 조건 확인 전 무조건
    /// 1회** (1차 스텝과 미묘하게 다름 — hello 가 비어도 pump 가 send/recv 를
    /// 진행시켜야 round-trip 이 끝난다). 동기 루프(위)와 부팅 상태 머신의
    /// RestoringLayout 스텝이 공유한다. 반환: 더 이상 pending 이 없는가.
    pub(super) fn boot_pump_step_remote_restores_done(&mut self) -> bool {
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
        !still_pending
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

        // Windows 절전(suspend/resume) 감지 — WM_POWERBROADCAST 후킹. resume 시
        // 죽은 ConPTY 자식 정리 + 살아있는 자식 wake nudge (ADR-0017). power
        // broadcast 는 시스템 전역이라 어느 윈도우든 받으므로, 창마다 설치해 두면
        // ≥1 개 창이 살아있는 한 동작한다 (resume 헬스 패스는 idempotent).
        #[cfg(windows)]
        crate::platform::power_windows::install_resume_hook(&window, self.view.proxy.clone());

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
        let gpu = self
            .create_gpu_state(window.clone(), &settings.appearance)
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
