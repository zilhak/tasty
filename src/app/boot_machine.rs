//! 부팅 상태 머신 (`BootPhase`) — 첫 윈도우 부팅의 동기 대기를 프레임 구동으로 전개.
//!
//! 두 축을 담당한다:
//! - **축A (visibility)**: 창을 hidden 으로 만들고 첫 로딩 프레임(`render_loading`)
//!   present 후 `set_visible(true)` — OS 기본 배경(흰) 프레임 자체를 제거한다.
//! - **frame-driven boot**: layout 복원 대기(구 300ms + 500ms sleep 루프)를 phase
//!   스텝으로 전개해, 대기 동안 메인 스레드가 얼지 않고 매 프레임 이벤트 루프에
//!   제어가 돌아온다 (3부 로딩 스피너의 전제).
//!
//! 진입 경로는 2개이며 모두 [`App::begin_boot`] 로 들어온다:
//! 1. 일반 부팅 — `resumed()` (창 hidden 생성 → 축A 적용).
//! 2. shell setup 완료 — `handle_shell_setup_window_event` 의 Confirmed (창이 이미
//!    보이는 상태이므로 축A 는 스킵, phase 구동은 동일).
//!
//! 구동: 부팅 미완 동안 `about_to_wait` 가 매 회 [`App::drive_boot_frame`] 을 호출
//! 하고 `ControlFlow::WaitUntil(+16ms)` 로 재예약한다 — hidden/표시 직후 창이
//! `RedrawRequested` 를 못 받을 수 있는 플랫폼(Windows WM_PAINT 등)에서도 진행이
//! 보장된다. `RedrawRequested` 도 같은 함수로 라우팅된다 (스텝은 조건 재확인이라
//! 중복 호출 무해).
//!
//! **부팅 가드**: phase 미완 동안 사용자 입력(window event)은 core 상태에 닿지
//! 않게 소비하고, `AppEvent` 는 지연 큐에 쌓아 Ready 후 순서대로 재생한다 — 구
//! 코드에서 `resumed()` 가 블로킹하는 동안 winit 큐에 쌓이던 것과 등가.
//! `ApplyPendingLayoutRestore` 의 bootstrap 전제(적용 전 다른 mutate 없음)를
//! 이벤트 루프 가동 중에도 지키기 위함이다. IPC 서버는 `finish_boot` 에서야
//! 시작하므로 부팅 중 IPC 유입은 구조적으로 없다.

use std::sync::Arc;
use std::time::{Duration, Instant};

use winit::event_loop::ActiveEventLoop;
use winit::window::Window;

use crate::app::App;
use crate::gpu::GpuState;

/// 부팅 스텝 페이스 — `about_to_wait` 워치독의 재예약 간격. 구 sleep(20ms) 폴링과
/// 동급 케이던스이면서 60fps 프레임 예산에 맞춘 값.
pub(crate) const BOOT_FRAME_INTERVAL: Duration = Duration::from_millis(16);

/// 부팅 상태 머신의 phase. 스텝 의미론은 구 동기 코드와 1:1 대응한다
/// (`window_lifecycle.rs` 의 `create_app_state` 참조).
pub(crate) enum BootPhase {
    /// GPU init 완료 + 첫 로딩 프레임 present 직후. 다음 스텝에서 엔진(CoreState)
    /// ·plugin manager 원자 초기화(T2.6·T3) 워커를 spawn 하고 WaitingEngine 으로
    /// 전이한다.
    GpuInit,
    /// 원자 초기화(T2.6·T3)가 워커 스레드에서 도는 동안 결과 채널을 폴링 —
    /// 구(S-4까지)의 동기 원자 스텝을 워커로 옮겨, 이 구간에도 매 프레임 로딩
    /// 렌더가 돈다 (4부 워커 분리). 채널 disconnect(워커 panic)는 동기 재시도
    /// fallback 으로 받는다.
    WaitingEngine {
        started: Instant,
        rx: std::sync::mpsc::Receiver<(crate::core::CoreState, crate::plugin::PluginManager)>,
        /// 워커 체류 동안 돈 부팅 프레임 스텝 수 — 로딩 프레임이 실제로
        /// 갱신됐는지의 계측 증거 (T2.7 로그).
        frames: u32,
    },
    /// pending layout restore 가 요구하는 plugin surface kind 등록 대기
    /// (구 1차 300ms sleep 루프의 프레임 전개). 스텝: pump →
    /// `finalize_plugin_hello` → 등록 확인, 미충족이면 다음 프레임 재시도.
    WaitingPlugins {
        started: Instant,
        deadline: Instant,
        needed: Vec<String>,
    },
    /// layout apply 완료 후 RemoteSurface 복원 round-trip 대기 (구 2차 500ms
    /// sleep 루프). 스텝: pump 는 조건 확인 전 무조건 1회 → still_pending 확인.
    RestoringLayout { started: Instant, deadline: Instant },
}

/// 부팅 진행 중 상태 — `App.boot` 에 `Some` 으로 존재하는 동안이 "부팅 미완".
/// `finish_boot` 가 take 해 `register_window` 로 합류하면 소멸한다.
pub(crate) struct BootState {
    pub(crate) window: Arc<Window>,
    pub(crate) gpu: GpuState,
    settings: crate::settings::Settings,
    pub(crate) phase: BootPhase,
    /// 부팅 시작 시각 (`boot_total` 계측 기준). 일반 경로는 `resumed()` 진입 시각,
    /// shell setup 경로는 Confirmed 시각.
    boot_t0: Instant,
    db_init_error: Option<crate::db::DbInitError>,
    invalid_theme_name: Option<String>,
    /// `ApplyPendingLayoutRestore` 가 복원한 활성 workspace idx.
    restored_idx: Option<usize>,
    /// 부팅 미완 중 도착한 `AppEvent` — Ready 후 도착 순서대로 재생한다.
    pub(crate) pending_events: Vec<crate::AppEvent>,
}

impl App {
    /// 부팅 상태 머신 시작 — db/theme 초기화(T2.5) 후 첫 로딩 프레임을 그리고
    /// (hidden 생성 경로면) 창을 표시한다. 이후 스텝은 `drive_boot_frame` 이 구동.
    ///
    /// `window_hidden`: 창이 `.with_visible(false)` 로 생성됐는가 — 일반 경로 true
    /// (첫 present 후 `set_visible(true)` = 축A), shell setup 완료 경로 false (이미
    /// setup 화면이 보이는 창이라 표시 전환 불필요).
    pub(crate) fn begin_boot(
        &mut self,
        window: Arc<Window>,
        gpu: GpuState,
        mut settings: crate::settings::Settings,
        boot_t0: Instant,
        window_hidden: bool,
    ) {
        let (db_init_error, invalid_theme_name) = Self::init_boot_db_and_theme(&mut settings);

        let mut boot = BootState {
            window,
            gpu,
            settings,
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

    /// T2.5: db + theme. theme 는 첫 present *전*에 설치해 로딩 프레임부터
    /// 사용자 theme 배경으로 그린다 (부팅 중 배경색 전환 방지). state.db 는
    /// create_app_state(엔진 초기화) 이전 필수 선행이라 같은 스텝에 묶는다 —
    /// 구 init_app_state 선두와 동일 순서. memory.db 는 boot 가 App::new 이전에
    /// 이미 초기화함 (D.3.C.M.1).
    fn init_boot_db_and_theme(
        settings: &mut crate::settings::Settings,
    ) -> (Option<crate::db::DbInitError>, Option<String>) {
        let t_db_theme = Instant::now();
        let db_init_error = crate::db::init().err();
        let invalid_theme_name =
            crate::app::window_lifecycle::boot_apply_theme(&mut settings.appearance);
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

    /// 첫 로딩 프레임 — hidden 창은 RedrawRequested 를 못 받을 수 있으므로
    /// 이벤트 대기 없이 즉시 그린다. 실패해도 창은 표시한다 (영구 hidden 방지
    /// fallback — 그 경우 OS 기본 배경이 짧게 보일 수 있으나 부팅은 진행된다).
    fn present_first_boot_frame(boot: &mut BootState, boot_t0: Instant, window_hidden: bool) {
        if let Err(e) = boot.gpu.render_loading(&boot.window, &boot.phase) {
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

    /// 부팅 1프레임: 현재 phase 1스텝 수행 → (미완이면) 로딩 프레임 렌더 +
    /// 다음 프레임 요청, (Ready 도달이면) `finish_boot` 로 합류.
    ///
    /// 재진입 안전: `self.boot` 를 take 한 뒤 구동하므로 스텝 도중 중첩 호출은
    /// no-op 이다.
    pub(crate) fn drive_boot_frame(&mut self, event_loop: &ActiveEventLoop) {
        let Some(mut boot) = self.boot.take() else {
            return;
        };
        let ready = self.boot_step(&mut boot);
        if ready {
            self.finish_boot(boot, event_loop);
            return;
        }
        match boot.gpu.render_loading(&boot.window, &boot.phase) {
            Ok(()) => {}
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                // surface 재구성 후 다음 프레임에서 재시도 (redraw.rs 관례와 동일).
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

    /// 현재 phase 1스텝. 부팅 완료(Ready 도달) 시 true.
    fn boot_step(&mut self, boot: &mut BootState) -> bool {
        if matches!(boot.phase, BootPhase::GpuInit) {
            // 원자 스텝(T2.6·T3)을 워커 스레드로 — cols/rows 는 GPU cell
            // metrics 의존이라 메인에서 계산해 전달한다. 워커가 도는 동안
            // 메인은 WaitingEngine 에서 매 프레임 로딩 렌더를 지속한다.
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

    /// `WaitingEngine` 스텝 — 워커 채널 폴링 후 결과 수신 시 core/plugin 장착,
    /// disconnect 시 동기 fallback. 반환: 부팅 완료 여부.
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
            Ok((engine, mgr)) => {
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
            Err(std::sync::mpsc::TryRecvError::Empty) => false,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                // 워커 panic (예: engine 생성 expect) — 결과 없이 채널이
                // drop 됐다. 동기 재시도: 워커 도입 전과 동일한 경로라,
                // 같은 원인이면 같은 표면(메인 panic)으로 수렴하고 일시
                // 원인이면 부팅이 정상 진행된다.
                tracing::error!("boot engine worker channel disconnected — synchronous fallback");
                self.ensure_engine_and_plugins(&boot.gpu, boot.settings.appearance.sidebar_width);
                self.boot_transition_after_engine(boot)
            }
        }
    }

    /// `WaitingPlugins` 스텝 — pending layout restore 가 요구하는 plugin surface
    /// kind 등록 여부를 확인, satisfied/deadline 시 apply 후 `RestoringLayout` 로
    /// 전이. 반환: 항상 false (이 스텝만으로는 부팅이 완료되지 않음).
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
        // deadline 초과 시 기존 동기 루프와 동일하게 그대로 진행 (안전망
        // 의미론 유지 — apply 는 어차피 수행되고 carry 가 layout 을 보호).
        if satisfied || Instant::now() >= *deadline {
            tracing::info!(
                target: "tasty::boot",
                ms = started.elapsed().as_secs_f64() * 1000.0,
                reason = if satisfied { "satisfied" } else { "deadline" },
                "T4 layout_wait_plugins (deadline 300ms)"
            );
            // 전이 시 1회 apply — 구 코드의 "1차 루프 탈출 → apply → 2차
            // 루프 진입" 순서와 동일. 단일 take 는 Intent 본문이 보장.
            boot.restored_idx = self.boot_apply_pending_layout_restore();
            let now = Instant::now();
            boot.phase = BootPhase::RestoringLayout {
                started: now,
                deadline: now + Duration::from_millis(500),
            };
        }
        false
    }

    /// `RestoringLayout` 스텝 — RemoteSurface 복원 round-trip 완료 여부를 확인.
    /// 반환: 부팅 완료 여부 (satisfied 또는 deadline 초과 시 true).
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

    /// 원자 초기화(T2.6·T3) 워커 spawn — 결과는 채널로 돌아온다 (WaitingEngine
    /// 스텝이 try_recv 폴링). 워커 본문은 동기 경로와 동일한 App-free 함수
    /// (`build_engine_and_plugins`) 라 의미론 이중화가 없다.
    ///
    /// 스레드 생성 실패 시: 에러 로그 후 tx 가 즉시 drop 되므로 첫 폴링이
    /// Disconnected 를 보고 동기 fallback 으로 합류한다.
    fn spawn_engine_worker(
        &self,
        boot: &BootState,
    ) -> std::sync::mpsc::Receiver<(crate::core::CoreState, crate::plugin::PluginManager)> {
        let (cols, rows) = crate::app::window_lifecycle::boot_grid_size(
            &boot.gpu,
            boot.settings.appearance.sidebar_width,
        );
        let factory: crate::waker::SharedWakerFactory = Arc::new(
            crate::waker_factory_winit::WinitWakerFactory::new(self.view.proxy.clone()),
        );
        let proxy = self.view.proxy.clone();
        let memory = self.core.memory_arc();
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
                    #[cfg(debug_assertions)]
                    input_simulation_enabled,
                );
                if tx.send(result).is_err() {
                    // 수신부(부팅 머신)가 먼저 사라진 경우 — 종료 경로. 여기서
                    // drop 되는 PluginManager 의 자식 프로세스는 PluginProcess
                    // 의 Drop 이 kill 로 정리한다.
                    tracing::warn!("boot engine worker: receiver dropped; discarding init result");
                }
            });
        if let Err(e) = spawned {
            tracing::error!("boot engine worker spawn failed: {e} — synchronous fallback");
        }
        rx
    }

    /// 엔진+plugin manager 장착 직후의 공통 전이 — pending layout restore 유무로
    /// WaitingPlugins 진입 또는 즉시 완료. 워커 정상 수신과 동기 fallback 이
    /// 공유하며, 구(동기 GpuInit 스텝) 후반부와 의미론 동일. 반환: 부팅 완료 여부.
    fn boot_transition_after_engine(&mut self, boot: &mut BootState) -> bool {
        if self.core_state().pending_layout_restore.is_some() {
            let needed = self.boot_required_plugin_kinds();
            let now = Instant::now();
            boot.phase = BootPhase::WaitingPlugins {
                started: now,
                deadline: now + Duration::from_millis(300),
                needed,
            };
            false
        } else {
            // 복원할 layout 없음 (첫 설치) — 대기 phase 없이 즉시 완료.
            true
        }
    }

    /// 부팅 중 종료 경로 공용 — WaitingEngine 워커가 spawn 한 plugin 자식
    /// 프로세스 회수. 결과를 잠시 기다려 PluginManager 를 받아 graceful
    /// shutdown 한다. 회수 없이 프로세스가 끝나면 워커가 강제 종료돼
    /// PluginProcess::drop 이 못 돌고 자식이 잔존할 수 있다.
    ///
    /// 부팅 미완이 아니거나 WaitingEngine 이 아니면 no-op — steady-state 종료
    /// 경로에서 불려도 무해하다.
    pub(crate) fn reclaim_boot_engine_worker_for_exit(&mut self) {
        let Some(boot) = self.boot.as_mut() else {
            return;
        };
        let BootPhase::WaitingEngine { rx, .. } = &boot.phase else {
            return;
        };
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok((engine, mut mgr)) => {
                drop(engine);
                mgr.shutdown_all();
            }
            Err(e) => {
                // 워커 panic(즉시 Disconnected) 또는 5s 초과 — 더 기다리지 않는다.
                tracing::warn!("boot engine worker not reclaimed before exit: {e}");
            }
        }
    }

    /// Ready 합류 — AppState 조립 + IPC server 시작 + `register_window` +
    /// `system.startup_complete` 발화 (구 `init_app_state` 후반부와 동일), 이후
    /// 부팅 중 지연된 `AppEvent` 를 재생한다.
    fn finish_boot(&mut self, boot: BootState, event_loop: &ActiveEventLoop) {
        let BootState {
            window,
            gpu,
            settings: _,
            phase: _,
            boot_t0,
            db_init_error,
            invalid_theme_name,
            restored_idx,
            pending_events,
        } = boot;

        let mut state = self.assemble_app_state(restored_idx);
        Self::report_boot_init_errors(&mut state, db_init_error, invalid_theme_name);
        self.start_boot_ipc_and_webhooks(&mut state);

        let mut core_state = self
            .core_state
            .take()
            .expect("App.core_state must be present to register a main window");
        // attach/detach 단계 3: force-detach 통지가 stream client 로 push 되도록
        // IPC 서버와 동일한 StreamHub 를 attach registry 에 주입.
        core_state.attach.set_notifier(self.stream_hub.clone());
        self.register_window(gpu, state, core_state, window.clone());
        self.emit_startup_complete_event();

        tracing::info!(
            target: "tasty::boot",
            ms = boot_t0.elapsed().as_secs_f64() * 1000.0,
            "boot_total (boot start -> Ready; T2.5~T6 + 미계측 잔여 합)"
        );
        // T7 기준 시각 — 부팅 완료(Ready). 구 코드의 resumed() 말미와 등가 시점.
        crate::boot::trace::mark_resumed_done();

        // 첫 실 UI 프레임 — MainView 는 dirty=true 로 시작하므로 redraw 요청만.
        window.request_redraw();

        // 부팅 중 지연된 AppEvent 재생 (도착 순서 유지). TerminalOutput 의 waker
        // dedup 게이트가 "이벤트 소비됐는데 engine 은 views 밖" 상태로 닫힌 채
        // 유실되지 않도록, 반드시 register_window *후* 에 재생한다.
        for ev in pending_events {
            use winit::application::ApplicationHandler;
            self.user_event(event_loop, ev);
        }
    }

    /// 부팅 초기화 에러(DB init 실패 / theme 이름 정정) 를 `state` 에 InfoModal 로
    /// 반영한다 — `create_new_window` 와 동일한 안내 방식. `self`/App 상태에는
    /// 닿지 않고 전달된 `state` 만 변형하는 순수 변환 함수.
    fn report_boot_init_errors(
        state: &mut crate::state::AppState,
        db_init_error: Option<crate::db::DbInitError>,
        invalid_theme_name: Option<String>,
    ) {
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
                state,
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
                state,
                crate::adapters::ui::info_modal::InfoModal {
                    title: crate::i18n::t("theme_error.title").to_string(),
                    body: crate::i18n::t_fmt("theme_error.body", &invalid),
                    on_close: crate::adapters::ui::info_modal::InfoModalAction::Continue,
                },
            );
        }
    }

    /// IPC/stream 서버 시작 + 웹훅 리스너 init — `finish_boot` 의 첫 윈도우 등록
    /// 직전 1회 지점. 웹훅 bind 실패는 `state` 에 Warning 토스트로 반영한다.
    fn start_boot_ipc_and_webhooks(&mut self, state: &mut crate::state::AppState) {
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
            // 웹훅 리스너 init — (A)config 로드 + (B)IPC 처리 가능 동시 만족 최초
            // 지점. finish_boot 는 첫 윈도우 1회만 호출되므로 중복 bind 가드
            // 불필요(리스너 내부 가드도 있음). injector 는 Clone(Arc).
            //
            // 포트 미설정/ bind 실패는 기존 toast 인프라로 사용자에게 알린다(신규
            // 디자인 컴포넌트 없이 재사용, S8). db/theme 부팅 경고가 InfoModal 을
            // 쓰는 것과 달리 웹훅 미기동은 치명적이지 않아 Warning 토스트로 족하다.
            //
            // 공유 훅 핸들러 레지스트리 시드(host embedded 기본값 + user config). 웹훅
            // 바인딩·`hook_handler.*` 조회가 이 전역 레지스트리를 보므로 리스너 init
            // 전에 채운다(plugin contribution 은 discover_and_start 에서 병합).
            crate::hook_handler::install_default_sources();
            // 완료 판정 전략 레지스트리 시드(TODO80 §B) — 훅 핸들러와 대칭 위치.
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

    /// Event Bus 1.0: `system.startup_complete` 를 부팅 완료 직후 1회 발화.
    /// `finish_boot` 는 첫 윈도우 등록 시 한 번만 호출되므로 별도 once 가드 불필요.
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

    /// 부팅 미완 동안의 `WindowEvent` 처리 — caller(`window_event`)는 호출 후 즉시
    /// return (shell setup 의 즉시 소비 선례와 동일). 사용자 입력이 core 상태에
    /// 닿지 않게 렌더/크기/종료 외 이벤트는 모두 소비한다.
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
                // WaitingEngine 워커가 만든 plugin 자식 프로세스 잔존 방지.
                self.reclaim_boot_engine_worker_for_exit();
                event_loop.exit();
            }
            // 부팅 미완 — 나머지 이벤트(키/마우스/포커스 등)는 소비만 한다.
            // ApplyPendingLayoutRestore 의 bootstrap 전제(적용 전 다른 mutate 없음)
            // 보호. 부팅은 최대 ~1s 이므로 입력 드롭 체감 없음.
            _ => {}
        }
    }
}
