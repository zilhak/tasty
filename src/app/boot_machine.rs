//! 첫 창의 부팅을 단계별로 진행하며 대기 중에도 이벤트 루프에 제어를 돌려준다.
//! 일반 부팅은 첫 로딩 프레임을 그린 뒤 창을 표시한다. shell setup에서 넘어온 창은 이미 보인다.
//! 부팅 중 사용자 입력은 처리하지 않고 AppEvent는 완료 후 순서대로 재생한다.
//! 레이아웃 복원 전에 다른 변경이 끼지 않게 하며 IPC 서버도 완료 단계에서 시작한다.
//! RedrawRequested가 오지 않아도 about_to_wait에서 다음 단계를 진행하도록 예약한다.

use std::sync::Arc;
use std::time::{Duration, Instant};

use winit::event_loop::ActiveEventLoop;
use winit::window::Window;

use crate::app::App;
use crate::gpu::GpuState;

/// about_to_wait에서 다음 부팅 단계를 예약하는 간격.
pub(crate) const BOOT_FRAME_INTERVAL: Duration = Duration::from_millis(16);

/// 레이아웃에 필요한 플러그인 kind 등록을 기다리는 상한. 동기 부팅 경로와 공유한다.
/// 기한이 지나도 복원은 진행하며 미등록 kind는 deferred placeholder로 남긴다.
/// 대기를 늘리면 첫 화면의 placeholder를 줄일 수 있지만 부팅도 늦어진다.
/// docs/features/layout-persistence/index.md의 Plugin surface 복원을 따른다.
pub(crate) const PLUGIN_WAIT_DEADLINE: Duration = Duration::from_millis(300);

pub(crate) enum BootPhase {
    GpuInit,
    /// 엔진·플러그인 워커를 기다린다. 결과 없이 채널이 끊기면 동기로 다시 초기화한다.
    WaitingEngine {
        started: Instant,
        rx: std::sync::mpsc::Receiver<
            anyhow::Result<(crate::core::CoreState, crate::plugin::PluginManager)>,
        >,
        /// 워커 결과를 확인한 횟수. 성공한 화면 렌더 횟수와는 다르다.
        frames: u32,
    },
    /// 복원에 필요한 플러그인 kind 등록을 기다리며 이벤트를 처리한다.
    WaitingPlugins {
        started: Instant,
        deadline: Instant,
        needed: Vec<String>,
    },
    /// 레이아웃 적용 뒤 RemoteSurface 복원 응답을 기다린다.
    RestoringLayout {
        started: Instant,
        deadline: Instant,
    },
}

pub(crate) struct BootState {
    pub(crate) window: Arc<Window>,
    pub(crate) gpu: GpuState,
    settings: crate::settings::Settings,
    /// 테마 저장이 설정 파일을 복구하기 전에 읽은 상태. 사용자에게 최초 문제를 알릴 때 사용한다.
    settings_origin: tasty_settings::SettingsOrigin,
    pub(crate) phase: BootPhase,
    /// 일반 부팅은 resumed 진입, shell setup은 확인 시점부터 측정한다.
    boot_t0: Instant,
    db_init_error: Option<crate::db::DbInitError>,
    invalid_theme_name: Option<String>,
    restored_idx: Option<usize>,
    /// 부팅 미완 중 도착한 `AppEvent` — Ready 후 도착 순서대로 재생한다.
    pub(crate) pending_events: Vec<crate::AppEvent>,
}

/// 이미 준비된 창과 GPU로 엔진 생성 오류를 보여 주며 같은 내용을 로그에도 남긴다.
fn boot_engine_error_info(err: &anyhow::Error) -> crate::gpu::BootErrorInfo {
    let title = crate::i18n::t("boot.engine_error.title").to_string();
    let body = crate::i18n::t_fmt("boot.engine_error.body", &err.to_string());
    let hint = crate::i18n::t("boot.engine_error.hint").to_string();
    tracing::error!("boot engine creation failed: {err:#}\n{title}\n{body}\n{hint}");
    crate::gpu::BootErrorInfo { title, body, hint }
}

impl App {
    /// 테마를 적용하고 첫 로딩 프레임을 그린다. hidden으로 만든 창만 표시를 전환한다.
    pub(crate) fn begin_boot(
        &mut self,
        window: Arc<Window>,
        gpu: GpuState,
        mut settings: crate::settings::Settings,
        boot_t0: Instant,
        window_hidden: bool,
    ) {
        let settings_origin = settings.origin;
        let (db_init_error, invalid_theme_name) = Self::init_boot_db_and_theme(&mut settings);

        // 엔진이 슬롯을 읽기 전에 레이아웃 마이그레이션과 전체 슬롯의 scrollback GC를 수행한다.
        crate::core::layout_persistence::migrate_and_gc_on_boot(settings.general.restore_layout);

        let mut boot = BootState {
            window,
            gpu,
            settings,
            settings_origin,
            phase: BootPhase::GpuInit,
            boot_t0,
            db_init_error,
            invalid_theme_name,
            restored_idx: None,
            pending_events: Vec::new(),
        };

        Self::present_first_boot_frame(&mut boot, boot_t0, window_hidden);
        self.boot = Some(boot);
    }

    /// 첫 로딩 프레임부터 사용자 테마를 쓰도록 적용한다. state.db도 엔진 생성 전에 준비한다.
    fn init_boot_db_and_theme(
        settings: &mut crate::settings::Settings,
    ) -> (Option<crate::db::DbInitError>, Option<String>) {
        let t_db_theme = Instant::now();
        let db_init_error = crate::db::init().err();
        let invalid_theme_name = crate::app::window_lifecycle::boot_apply_theme(settings);
        if let Err(e) = settings.save() {
            tracing::warn!("failed to persist settings after theme apply: {e}");
        }
        tracing::info!(
            target: "tasty::boot",
            ms = t_db_theme.elapsed().as_secs_f64() * 1000.0,
            "T2.5 db_theme (begin_boot enter -> first loading frame)"
        );
        (db_init_error, invalid_theme_name)
    }

    /// hidden 창은 RedrawRequested를 못 받을 수 있어 즉시 그린다.
    /// 렌더 실패 시에도 창을 표시하므로 OS 기본 배경이 잠깐 보일 수 있다.
    fn present_first_boot_frame(boot: &mut BootState, boot_t0: Instant, window_hidden: bool) {
        let phase_key = crate::gpu::loading::boot_phase_text_key(&boot.phase);
        if let Err(e) = boot.gpu.render_loading(&boot.window, phase_key) {
            tracing::warn!("boot loading first frame render failed: {e} — showing window anyway");
        }
        if window_hidden {
            boot.window.set_visible(true);
            tracing::info!(
                target: "tasty::boot",
                ms = boot_t0.elapsed().as_secs_f64() * 1000.0,
                "T2.9 window_visible (boot start -> set_visible(true) after first loading frame)"
            );
        }
        boot.window.request_redraw();
    }

    /// boot를 꺼내서 처리하므로 처리 중 다시 호출되면 아무 일도 하지 않는다.
    pub(crate) fn drive_boot_frame(&mut self, event_loop: &ActiveEventLoop) {
        let Some(mut boot) = self.boot.take() else {
            return;
        };
        let ready = self.boot_step(&mut boot);
        // 엔진 생성 실패는 준비된 창과 GPU를 오류 화면에 넘겨 알린다.
        if self.boot_error_info.is_some() {
            self.enter_boot_error_mode(boot.window, boot.gpu);
            return;
        }
        if ready {
            self.finish_boot(boot, event_loop);
            return;
        }
        let phase_key = crate::gpu::loading::boot_phase_text_key(&boot.phase);
        match boot.gpu.render_loading(&boot.window, phase_key) {
            Ok(()) => {}
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                // 다음 프레임에 다시 그릴 수 있도록 surface를 재구성한다.
                boot.gpu.resize(boot.window.inner_size());
            }
            Err(e) => {
                let msg = format!("boot loading frame render error: {e}");
                tracing::warn!("{}", msg);
                crate::crash_report::record_error(&msg);
            }
        }
        boot.window.request_redraw();
        self.boot = Some(boot);
    }

    /// 한 단계를 진행하고 부팅 완료 여부를 반환한다.
    fn boot_step(&mut self, boot: &mut BootState) -> bool {
        if matches!(boot.phase, BootPhase::GpuInit) {
            let rx = self.spawn_engine_worker(boot);
            boot.phase = BootPhase::WaitingEngine {
                started: Instant::now(),
                rx,
                frames: 0,
            };
            return false;
        }
        if matches!(boot.phase, BootPhase::WaitingEngine { .. }) {
            return self.boot_step_waiting_engine(boot);
        }
        if matches!(boot.phase, BootPhase::WaitingPlugins { .. }) {
            return self.boot_step_waiting_plugins(boot);
        }
        self.boot_step_restoring_layout(boot)
    }

    fn boot_step_waiting_engine(&mut self, boot: &mut BootState) -> bool {
        let BootPhase::WaitingEngine {
            started,
            rx,
            frames,
        } = &mut boot.phase
        else {
            unreachable!("boot_step_waiting_engine called outside WaitingEngine phase");
        };
        *frames += 1;
        match rx.try_recv() {
            Ok(Ok((engine, mgr))) => {
                let wait_ms = started.elapsed().as_secs_f64() * 1000.0;
                let frames = *frames;
                self.core_state = Some(engine);
                self.plugin_manager = Some(mgr);
                tracing::info!(
                    target: "tasty::boot",
                    ms = wait_ms,
                    frames,
                    "T2.7 engine_wait (워커 체류; frames = 그동안 돈 로딩 프레임 스텝 수)"
                );
                self.boot_transition_after_engine(boot)
            }
            Ok(Err(e)) => {
                self.boot_error_info = Some(boot_engine_error_info(&e));
                false
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => false,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                // 스레드 생성 실패나 워커 패닉으로 결과가 없으면 동기로 다시 시도한다.
                tracing::error!("boot engine worker channel disconnected — synchronous fallback");
                if let Err(e) = self
                    .ensure_engine_and_plugins(&boot.gpu, boot.settings.appearance.sidebar_width)
                {
                    self.boot_error_info = Some(boot_engine_error_info(&e));
                    return false;
                }
                self.boot_transition_after_engine(boot)
            }
        }
    }

    fn boot_step_waiting_plugins(&mut self, boot: &mut BootState) -> bool {
        let BootPhase::WaitingPlugins {
            started,
            deadline,
            needed,
        } = &mut boot.phase
        else {
            unreachable!("boot_step_waiting_plugins called outside WaitingPlugins phase");
        };
        let satisfied = self.boot_pump_step_plugins_registered(needed);
        // 기한이 지나도 복원을 진행하며 미등록 kind는 placeholder로 남긴다.
        if satisfied || Instant::now() >= *deadline {
            tracing::info!(
                target: "tasty::boot",
                ms = started.elapsed().as_secs_f64() * 1000.0,
                reason = if satisfied { "satisfied" } else { "deadline" },
                deadline_ms = PLUGIN_WAIT_DEADLINE.as_millis() as u64,
                "T4 layout_wait_plugins"
            );
            boot.restored_idx = self.boot_apply_pending_layout_restore();
            let now = Instant::now();
            boot.phase = BootPhase::RestoringLayout {
                started: now,
                deadline: now + Duration::from_millis(500),
            };
        }
        false
    }

    fn boot_step_restoring_layout(&mut self, boot: &mut BootState) -> bool {
        let BootPhase::RestoringLayout { started, deadline } = &mut boot.phase else {
            unreachable!("boot_step_restoring_layout called outside RestoringLayout phase");
        };
        let done = self.boot_pump_step_remote_restores_done();
        if done || Instant::now() >= *deadline {
            tracing::info!(
                target: "tasty::boot",
                ms = started.elapsed().as_secs_f64() * 1000.0,
                reason = if done { "satisfied" } else { "deadline" },
                "T6 remote_surface_wait (deadline 500ms)"
            );
            true
        } else {
            false
        }
    }

    /// 동기 부팅과 같은 초기화 함수를 워커에서 실행한다.
    /// 스레드를 만들지 못하면 송신자가 해제돼 다음 수신 시 동기 재시도로 넘어간다.
    fn spawn_engine_worker(
        &self,
        boot: &BootState,
    ) -> std::sync::mpsc::Receiver<
        anyhow::Result<(crate::core::CoreState, crate::plugin::PluginManager)>,
    > {
        let (cols, rows) = crate::app::window_lifecycle::boot_grid_size(
            &boot.gpu,
            boot.settings.appearance.sidebar_width,
        );
        let factory: crate::waker::SharedWakerFactory = Arc::new(
            crate::waker_factory_winit::WinitWakerFactory::new(self.view.proxy.clone()),
        );
        let proxy = self.view.proxy.clone();
        let memory = self.core.memory_arc();
        let gauges = self.core.plugin_gauges();
        // 점유 중인 슬롯을 확인해야 하므로 메인 스레드에서 선택해 워커로 전달한다.
        let layout_slot = self.claim_free_layout_slot();
        #[cfg(debug_assertions)]
        let input_simulation_enabled = self.input_simulation_enabled;
        let (tx, rx) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("tasty-boot-engine".into())
            .spawn(move || {
                let result = crate::app::window_lifecycle::build_engine_and_plugins(
                    cols,
                    rows,
                    factory,
                    proxy,
                    memory,
                    layout_slot,
                    gauges,
                    #[cfg(debug_assertions)]
                    input_simulation_enabled,
                );
                if tx.send(result).is_err() {
                    // 수신자가 없으면 결과를 drop하며 플러그인 자식 정리도 Drop에 맡긴다.
                    tracing::warn!("boot engine worker: receiver dropped; discarding init result");
                }
            });
        if let Err(e) = spawned {
            tracing::error!("boot engine worker spawn failed: {e} — synchronous fallback");
        }
        rx
    }

    fn boot_transition_after_engine(&mut self, boot: &mut BootState) -> bool {
        if self.core_state().pending_layout_restore.is_some() {
            let needed = self.boot_required_plugin_kinds();
            let now = Instant::now();
            boot.phase = BootPhase::WaitingPlugins {
                started: now,
                deadline: now + PLUGIN_WAIT_DEADLINE,
                needed,
            };
            false
        } else {
            true
        }
    }

    /// 종료 전에 결과를 회수해야 할 부팅 워커가 있는지 확인한다.
    /// 회수 전에 프로세스를 끝내면 PluginManager의 Drop이 실행되지 않아 자식이 남을 수 있다.
    pub(super) fn boot_engine_worker_pending(&self) -> bool {
        self.boot
            .as_ref()
            .is_some_and(|b| matches!(b.phase, BootPhase::WaitingEngine { .. }))
    }

    /// 종료 단계에서 블로킹 없이 결과를 받는다. 대기 기한은 호출자가 관리한다.
    /// Ok(None)은 결과 대기 중이거나 엔진 생성 실패, Err는 채널 단절 또는 회수 대상 부재다.
    pub(super) fn try_recv_boot_engine_worker(
        &mut self,
    ) -> Result<Option<(crate::core::CoreState, crate::plugin::PluginManager)>, ()> {
        let Some(boot) = self.boot.as_mut() else {
            return Err(());
        };
        let BootPhase::WaitingEngine { rx, .. } = &boot.phase else {
            return Err(());
        };
        match rx.try_recv() {
            Ok(Ok(payload)) => Ok(Some(payload)),
            Ok(Err(_)) => Ok(None),
            Err(std::sync::mpsc::TryRecvError::Empty) => Ok(None),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                tracing::warn!("boot engine worker not reclaimed before exit: disconnected");
                Err(())
            }
        }
    }

    /// 창·IPC를 등록하고 시작 이벤트를 알린 뒤 부팅 중 보류한 이벤트를 재생한다.
    fn finish_boot(&mut self, boot: BootState, event_loop: &ActiveEventLoop) {
        let BootState {
            window,
            gpu,
            settings: _,
            settings_origin,
            phase: _,
            boot_t0,
            db_init_error,
            invalid_theme_name,
            restored_idx,
            pending_events,
        } = boot;

        // 복원 예정이면 기본 workspace를 만들지 않았으므로 복원 실패 시 여기서 보충한다.
        let bootstrapped = match self.core_state.as_mut() {
            Some(engine) => crate::app::App::bootstrap_workspace_if_empty(&mut self.core, engine),
            None => None,
        };
        let mut state = self.assemble_app_state(bootstrapped.or(restored_idx));
        Self::report_boot_init_errors(&mut state, db_init_error, invalid_theme_name);
        Self::report_locale_fallback(&mut state);
        self.start_boot_ipc_and_webhooks(&mut state);
        Self::report_persistence_incidents(settings_origin, self.core_state(), &mut state);

        let mut core_state = self
            .core_state
            .take()
            .expect("App.core_state must be present to register a main window");
        // force-detach 통지를 IPC와 같은 스트림으로 보낸다.
        core_state.attach.set_notifier(self.stream_hub.clone());
        // 첫 창을 노출하기 전에 이전 실행의 에이전트 작업 상태를 정리하며 자동 실행은 하지 않는다.
        self.core.purge_stale_agent_state_on_boot(&core_state);
        self.core.inject_agent_runner_registry(&core_state);
        Self::report_missing_permissions(&mut state);
        self.register_window(
            gpu,
            state,
            core_state,
            window.clone(),
            crate::app::event::WindowRequestOrigin::User,
        );
        self.emit_startup_complete_event();

        tracing::info!(
            target: "tasty::boot",
            ms = boot_t0.elapsed().as_secs_f64() * 1000.0,
            "boot_total (boot start -> Ready; T2.5~T6 + 미계측 잔여 합)"
        );
        crate::boot::trace::mark_resumed_done();

        window.request_redraw();

        // TerminalOutput 이벤트를 보류한 동안 닫혀 있던 wake 중복 방지 상태를 풀 수 있도록
        // 엔진이 views에 등록된 뒤 이벤트를 재생한다.
        for ev in pending_events {
            use winit::application::ApplicationHandler;
            self.user_event(event_loop, ev);
        }
    }

    /// 언어팩 폴백을 한 번 알린다. 설정값은 유지하며 로더가 남긴 경고 로그는 반복하지 않는다.
    fn report_locale_fallback(state: &mut crate::state::AppState) {
        if let Some(msg) =
            crate::i18n::load_report().and_then(crate::i18n::LoadReport::user_warning)
        {
            state.toasts.push(
                msg,
                crate::adapters::ui::ToastKind::Warning,
                crate::adapters::ui::ToastScope::Window,
            );
        }
        // 폰트 실패는 언어 폴백과 별개다. 긴 경로만 줄여 조치 안내가 잘리지 않게 한다.
        if let Some(detail) = crate::boot::locale::font_warning() {
            state.toasts.push(
                crate::i18n::t_fmt_fit("i18n.warn.font_unresolved", &detail),
                crate::adapters::ui::ToastKind::Warning,
                crate::adapters::ui::ToastScope::Window,
            );
        }
    }

    /// DB 초기화 실패와 잘못된 테마 이름을 모달로 알린다.
    fn report_boot_init_errors(
        state: &mut crate::state::AppState,
        db_init_error: Option<crate::db::DbInitError>,
        invalid_theme_name: Option<String>,
    ) {
        if let Some(err) = db_init_error {
            tracing::error!("state.db init failed: {err}");
            let (key, args) = err.user_message_i18n();
            let body = match args.len() {
                0 => crate::i18n::t(key).to_string(),
                1 => crate::i18n::t_fmt(key, &args[0]),
                _ => crate::i18n::t_fmt2(key, &args[0], &args[1]),
            };
            crate::adapters::ui::info_modal::show_info_modal(
                state,
                crate::adapters::ui::info_modal::InfoModal {
                    title: crate::i18n::t("db_error.title").to_string(),
                    body,
                    on_close: crate::adapters::ui::info_modal::InfoModalAction::Exit(1),
                    extra_buttons: Vec::new(),
                },
            );
        }

        if let Some(invalid) = invalid_theme_name {
            crate::adapters::ui::info_modal::show_info_modal(
                state,
                crate::adapters::ui::info_modal::InfoModal {
                    title: crate::i18n::t("theme_error.title").to_string(),
                    body: crate::i18n::t_fmt("theme_error.body", &invalid),
                    on_close: crate::adapters::ui::info_modal::InfoModalAction::Continue,
                    extra_buttons: Vec::new(),
                },
            );
        }
    }

    /// macOS에서 확인할 수 있는 권한 중 하나라도 허용되지 않았으면 안내 모달을 띄운다.
    /// 부팅 직후에 권한을 자동으로 요청하지 않으므로, 사용자가 권한 화면을 찾아가는
    /// 경로는 이 안내다. 사용자가 권한을 회수할 수 있어 부팅할 때마다 다시 확인하며,
    /// 안내를 띄웠다는 사실은 저장하지 않는다. 어떤 권한을 보는지와 그 한계는
    /// `crates/tasty-platform/src/macos_permissions.rs`의 `should_show_permission_notice`에
    /// 있다. macOS가 아니면 아무 일도 하지 않는다.
    fn report_missing_permissions(state: &mut crate::state::AppState) {
        if !crate::macos_permissions::wants_permission_notice() {
            return;
        }
        crate::adapters::ui::info_modal::show_info_modal(
            state,
            crate::adapters::ui::info_modal::InfoModal {
                title: crate::i18n::t("macos_permissions.notice.title").to_string(),
                body: crate::i18n::t("macos_permissions.notice.body").to_string(),
                on_close: crate::adapters::ui::info_modal::InfoModalAction::Continue,
                extra_buttons: permission_notice_buttons(),
            },
        );
    }

    /// 설정·레이아웃을 읽거나 보존하지 못한 사실과 복구에 필요한 정보를 알린다.
    fn report_persistence_incidents(
        settings_origin: tasty_settings::SettingsOrigin,
        engine: &crate::core::CoreState,
        state: &mut crate::state::AppState,
    ) {
        use crate::adapters::ui::{ToastKind, ToastScope};
        use tasty_settings::SettingsOrigin;

        let mut warn = |msg: String| {
            state
                .toasts
                .push(msg, ToastKind::Warning, ToastScope::Window);
        };
        // 테마 저장으로 파일이 복구됐어도 부팅 때 처음 발견한 문제는 안내해야 한다.
        match settings_origin {
            SettingsOrigin::Clean => {}
            SettingsOrigin::Unparsable => {
                let path = tasty_settings::Settings::config_path().unwrap_or_default();
                let shown = tasty_utils::path::tilde_abbreviate(&path);
                // 파일이 여전히 해석되지 않으면 백업에 성공했다고 안내하지 않는다.
                let key = if tasty_settings::Settings::file_is_unparsable(&path) {
                    "persistence.warn.settings_unparsable_blocked"
                } else {
                    "persistence.warn.settings_unparsable"
                };
                // 경로만 줄여 문장 끝의 조치 안내를 남긴다.
                warn(crate::i18n::t_fmt_fit(key, &shown));
            }
            SettingsOrigin::ProtectedUnreadable => {
                warn(crate::i18n::t("persistence.warn.settings_locked").to_string());
            }
        }
        if engine.layout_slot_preserve_failed {
            warn(crate::i18n::t("persistence.warn.layout_unparsable_blocked").to_string());
        } else if engine.layout_slot_unparsable {
            warn(crate::i18n::t("persistence.warn.layout_unparsable").to_string());
        }
        if engine.layout_slot_protected {
            warn(crate::i18n::t("persistence.warn.layout_locked").to_string());
        }
    }

    fn start_boot_ipc_and_webhooks(&mut self, state: &mut crate::state::AppState) {
        let ipc_proxy = self.view.proxy.clone();
        let ipc_waker: crate::ipc::server::IpcWaker = std::sync::Arc::new(move || {
            crate::shortcuts::send_app_event(&ipc_proxy, crate::AppEvent::IpcReady);
        });
        let stream_proxy = self.view.proxy.clone();
        let stream_waker: crate::ipc::server::IpcWaker = std::sync::Arc::new(move || {
            crate::shortcuts::send_app_event(&stream_proxy, crate::AppEvent::StreamReady);
        });
        let stream_ctx = tasty_ipc::stream_hub::StreamContext {
            hub: self.stream_hub.clone(),
            inbound_tx: self.stream_inbound_tx.clone(),
            waker: stream_waker,
        };
        let connections = self.core.connections().clone();
        if let Some(injector) = self.hub.start_ipc(ipc_waker, stream_ctx, connections) {
            // 리스너가 참조할 훅 레지스트리를 먼저 채운다. 웹훅 시작 실패는 비치명적 경고로 알린다.
            crate::hook_handler::install_default_sources();
            crate::completion_strategy::install_default_sources();
            let report = crate::webhook::init_from_config(injector.clone());
            if let Some(msg) = report.user_warning() {
                state.toasts.push(
                    msg,
                    crate::adapters::ui::ToastKind::Warning,
                    crate::adapters::ui::ToastScope::Window,
                );
            }
            self.core.set_host_ipc_injector(injector);
        }
    }

    fn emit_startup_complete_event(&mut self) {
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

    /// 복원 중 사용자 입력이 구조를 바꾸지 않도록 렌더·크기·종료 외 이벤트는 소비한다.
    pub(crate) fn handle_boot_window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        event: winit::event::WindowEvent,
    ) {
        use winit::event::WindowEvent;
        match event {
            WindowEvent::RedrawRequested => self.drive_boot_frame(event_loop),
            WindowEvent::Resized(size) => {
                if let Some(boot) = self.boot.as_mut() {
                    boot.gpu.resize(size);
                }
            }
            WindowEvent::CloseRequested => {
                // 종료 상태 머신이 부팅 워커와 플러그인 자식도 회수한다.
                self.begin_shutdown(event_loop);
            }
            _ => {}
        }
    }
}

/// 권한 안내 모달에 붙는 버튼. 시스템 설정이 아니라 Tasty의 권한 화면으로 보낸다.
#[cfg(all(target_os = "macos", feature = "gui"))]
fn permission_notice_buttons() -> Vec<crate::adapters::ui::info_modal::InfoModalButton> {
    vec![crate::adapters::ui::info_modal::InfoModalButton {
        label: crate::i18n::t("macos_permissions.notice.open_settings").to_string(),
        action: crate::adapters::ui::info_modal::InfoModalButtonAction::OpenPermissionSettings,
    }]
}

#[cfg(not(all(target_os = "macos", feature = "gui")))]
fn permission_notice_buttons() -> Vec<crate::adapters::ui::info_modal::InfoModalButton> {
    Vec::new()
}

#[cfg(test)]
mod boot_error_tests {
    use super::*;

    /// 번역 초기화 전후 모두 서로 다른 제목·본문·조치 안내를 사용해야 한다.
    #[test]
    fn engine_error_info_reads_three_distinct_diagnostics() {
        let info = boot_engine_error_info(&anyhow::anyhow!("shell not found: /bad/path"));
        assert!(!info.title.is_empty(), "title must be present");
        assert!(!info.body.is_empty(), "body must be present");
        assert!(!info.hint.is_empty(), "hint must be present");
        assert_ne!(info.title, info.body, "title and body must differ");
        assert_ne!(info.body, info.hint, "body and hint must differ");
        assert_ne!(info.title, info.hint, "title and hint must differ");
    }
}
