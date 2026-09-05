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

/// 부팅이 pending layout restore 가 요구하는 plugin surface kind 의 hello 등록을
/// 기다리는 상한. `WaitingPlugins` 스텝(상태 머신 경로)과
/// `boot_wait_for_required_plugin_kinds`(동기 루프 경로)가 공유한다.
///
/// **이 값은 유실을 막는 값이 아니다.** satisfied 든 이 상한 초과든 apply 는
/// 어차피 수행되고, apply(=`rebuild_pane` 트리)는 kind 가 아직 registry 에
/// 없어도 서브트리를 버리지 않고 deferred placeholder 를 남긴다 —
/// `reify_displayed_surfaces` 가 kind 등록 후 실제화한다(유실 0). 그래서 이
/// 상한은 **"부팅 첫 프레임에 plugin 자리가 placeholder 로 잠깐 비어 보이는
/// 것을 얼마나 감출까"의 트레이드오프**다:
/// - 크게 잡으면 부팅이 그만큼(최대 이 값) hello 를 더 기다려 첫 프레임에 실제
///   surface 가 이미 차 있을 확률이 오르지만, 그 대기만큼 부팅이 느려진다.
/// - 작게(0 에 가깝게) 잡으면 부팅이 빨라지지만, hello 가 늦은 plugin 은
///   placeholder 로 떴다가 다음 프레임 이후 채워지는 깜빡임이 보일 수 있다.
///
/// 300 이라는 구체값의 유래는 코드에 근거가 남아있지 않다(승격 전 두 곳 다
/// 주석 없는 리터럴이었다). 조정하려는 사람은 300 의 유래가 아니라 위
/// 트레이드오프로 판단하면 된다. deferred/reify 의미론은
/// [`docs/features/layout-persistence/index.md`] 의 "Plugin surface 복원" 참조.
pub(crate) const PLUGIN_WAIT_DEADLINE: Duration = Duration::from_millis(300);

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
        rx: std::sync::mpsc::Receiver<
            anyhow::Result<(crate::core::CoreState, crate::plugin::PluginManager)>,
        >,
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
    /// 부팅이 **설정 파일을 처음 읽었을 때** 본 상태. T2.5 의 theme 저장이 이미
    /// 손상 원본을 백업으로 옮기고 정상 파일을 써 놓으므로, 나중에 만들어지는 engine 의
    /// `settings.origin` 은 늘 `Clean` 이다 — 사용자에게 알리려면 여기서 붙잡아 둬야 한다.
    settings_origin: tasty_settings::SettingsOrigin,
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

/// 부팅 중 engine 생성 실패의 진단을 만든다. 예전엔 곧바로 `exit(1)` 했으나, 이 단계는
/// 부팅 GPU init 이후라 **GPU·창이 살아있다** — 그래서 진단을 창에 그려 런처로 실행한
/// 사용자에게도 보이게 한다(`enter_boot_error_mode`). stderr·파일 로그로도 낸다(터미널
/// 사용자용): `tracing::error!` 는 fmt subscriber 로 stderr + 파일 둘 다에 나간다
/// (eprintln 은 파일에 안 남아 사후 진단이 불가능, C.11 준수). 진단 3줄을 한 이벤트로
/// 합친다 — tracing 매크로 확장이 커서 여러 번 부르면 cognitive_complexity(deny)를 넘긴다.
fn boot_engine_error_info(err: &anyhow::Error) -> crate::gpu::BootErrorInfo {
    let title = crate::i18n::t("boot.engine_error.title").to_string();
    let body = crate::i18n::t_fmt("boot.engine_error.body", &err.to_string());
    let hint = crate::i18n::t("boot.engine_error.hint").to_string();
    tracing::error!("boot engine creation failed: {err:#}\n{title}\n{body}\n{hint}");
    crate::gpu::BootErrorInfo { title, body, hint }
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
        let settings_origin = settings.origin;
        let (db_init_error, invalid_theme_name) = Self::init_boot_db_and_theme(&mut settings);

        // 레거시 `layout.json` → `layouts/01.json` 마이그레이션 + 전 슬롯 union
        // scrollback GC. **`spawn_engine_worker` 가 슬롯을 읽기 전**이어야 한다 —
        // 마이그레이션 결과를 engine 이 봐야 하고, GC 는 부팅 1 회만 돌아야 한다.
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

    /// 첫 로딩 프레임 — hidden 창은 RedrawRequested 를 못 받을 수 있으므로
    /// 이벤트 대기 없이 즉시 그린다. 실패해도 창은 표시한다 (영구 hidden 방지
    /// fallback — 그 경우 OS 기본 배경이 짧게 보일 수 있으나 부팅은 진행된다).
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
        // 엔진 생성이 실패했다 — GPU·창은 살아있으니(부팅 GPU init 이후 단계) 실패
        // 화면을 그려 사용자에게 보인다. boot 는 재저장하지 않고 window/gpu 소유권을
        // boot error 모드로 넘긴다. GPU 부재·창 생성 실패는 여기 오지 않는다(그쪽은
        // 그릴 수단이 없어 진단 후 즉시 exit). ADR-0117 재검토 트리거.
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
                // 워커가 engine 생성에 실패했다(셸 spawn·PTY/fd 등). GPU·창은
                // 살아있으니 진단을 창에 그려 보인다(drive_boot_frame 이 boot_error_info
                // 를 보고 boot error 모드로 전환). 첫 창이라도 사라지며 깜빡이지 않는다.
                self.boot_error_info = Some(boot_engine_error_info(&e));
                false
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => false,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                // 워커 스레드 자체가 panic 해 결과 없이 채널이 drop 된 경우(engine
                // 생성 실패는 이제 위 `Ok(Err)` 로 오므로, 여기는 그 밖의 예상 밖
                // panic 이다). 동기 재시도 — 일시 원인이면 부팅이 진행된다.
                //
                // 슬롯: `ensure_engine_and_plugins` 가 스스로 다시 고른다. 워커가
                // 결과를 못 보냈다는 건 그 engine 이 살아남지 못했다는 뜻이라 점유
                // 집합은 여전히 비어 있고, 슬롯 파일 목록도 그대로여서 워커가
                // 골랐던 것과 같은 슬롯으로 수렴한다.
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
                deadline_ms = PLUGIN_WAIT_DEADLINE.as_millis() as u64,
                "T4 layout_wait_plugins"
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
        // 슬롯 선택은 `App` 순회(점유 스캔)가 필요하므로 **메인 스레드에서** 정해
        // 워커로 move 한다. 부팅 시점엔 살아있는 engine 이 없으니 점유는 비어 있고,
        // 저장된 슬롯이 있으면 그 중 가장 낮은 번호(보통 1), 없으면 1 이 된다 —
        // 슬롯이 3개 저장돼 있어도 부팅은 창 1개, 슬롯 1개 점유다.
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
                deadline: now + PLUGIN_WAIT_DEADLINE,
                needed,
            };
            false
        } else {
            // 복원할 layout 없음 (첫 설치) — 대기 phase 없이 즉시 완료.
            true
        }
    }

    /// 회수해야 할 부팅 워커가 도는 중인가 — 종료 상태 머신의 S2 진입 판정.
    ///
    /// `WaitingEngine` 워커는 plugin 자식 프로세스를 spawn 한다. 결과를 회수하지
    /// 않고 프로세스가 끝나면 그 `PluginManager` 가 워커 스레드와 함께 강제로
    /// 사라져 `PluginProcess::drop` 이 돌지 못하고 자식이 잔존할 수 있다.
    ///
    /// 부팅 미완이 아니거나 `WaitingEngine` 이 아니면 false — steady-state 종료는
    /// S2 를 건너뛰고, 그 경로에서 `S2 boot_worker_reclaim` 마커가 아예 나오지
    /// 않는 것이 정상이다.
    pub(super) fn boot_engine_worker_pending(&self) -> bool {
        self.boot
            .as_ref()
            .is_some_and(|b| matches!(b.phase, BootPhase::WaitingEngine { .. }))
    }

    /// 부팅 워커 결과 **논블로킹** 폴링 — 종료 상태 머신의 S2 스텝이 매 프레임
    /// 부른다. 동기 시절의 `recv_timeout(5s)` 를 대체하며, deadline 판정은
    /// 호출자(상태 머신)가 들고 있다.
    ///
    /// 반환: `Ok(Some(..))` 수신 / `Ok(None)` 아직 / `Err(())` 채널 단절(워커 panic
    /// 등 — 더 기다릴 것이 없다). 회수 대상이 아니면 `Err(())` 로 취급해 즉시
    /// 다음 phase 로 보낸다.
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
            // engine 생성이 실패해 회수할 PluginManager 가 없다 — 회수 대상 없음으로 취급.
            Ok(Err(_)) => Ok(None),
            Err(std::sync::mpsc::TryRecvError::Empty) => Ok(None),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                tracing::warn!("boot engine worker not reclaimed before exit: disconnected");
                Err(())
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
            settings_origin,
            phase: _,
            boot_t0,
            db_init_error,
            invalid_theme_name,
            restored_idx,
            pending_events,
        } = boot;

        // 복원 실패 안전망 — 복원 예정이었으면 `new_with_ids` 가 기본 워크스페이스를
        // 만들지 않았으므로(PTY 누수 방지), 복원이 하나도 못 만든 경우 여기서 메운다.
        // 동기 경로(`create_app_state`)와 같은 지점·같은 헬퍼. 복원이 정상이면 no-op.
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
        // attach/detach 단계 3: force-detach 통지가 stream client 로 push 되도록
        // IPC 서버와 동일한 StreamHub 를 attach registry 에 주입.
        core_state.attach.set_notifier(self.stream_hub.clone());
        // agent task runner 재시작 정화(결정 2) — headless `boot.rs` 와 동일 정책.
        // 자동 시작은 하지 않는다(결정 1). 첫 윈도우 등록(= client 노출) 전에
        // 1 회만 수행.
        self.core.purge_stale_agent_state_on_boot(&core_state);
        // 렌더 경로(DAG surface 러너 배지)가 `Core` 없이도 러너 생사를 물을 수
        // 있도록 같은 레지스트리 Arc 를 CoreState 에 심는다(memory 주입과 동형).
        if core_state
            .agent_runner_registry
            .set(self.core.agent_runner_registry())
            .is_err()
        {
            tracing::warn!("agent runner registry already injected into CoreState");
        }
        Self::report_missing_full_disk_access(&mut state, &mut core_state.settings);
        self.register_window(gpu, state, core_state, window.clone());
        self.emit_startup_complete_event();

        // macOS 파일 TCC 프롬프트를 여기서 몰아 띄운다. 첫 윈도우가 등록돼 앱이
        // foreground 로 활성화된 뒤라야 프롬프트가 사용자에게 보인다 — 윈도우 생성
        // 전에 부르면 백그라운드로 밀릴 수 있다. 워커 스레드로 나가므로 바로 아래
        // `boot_total` 계측과 첫 프레임 요청을 붙잡지 않는다. macOS 외에서는 no-op.
        crate::macos_permissions::spawn_prewarm();

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

    /// 부팅 로케일 판정이 영어로 폴백했으면(요청 언어의 팩 부재 · 형상 위반 —
    /// `docs/dev-guide/i18n.md` "언어팩") 경고 토스트 1회. 치명적이지 않으니 웹훅 bind
    /// 실패 경고와 같은 구조(`ToastKind::Warning`, 창 스코프)로 알리고, 설정값은 건드리지
    /// 않는다. headless/CLI 경로는 `tasty_i18n` 이 load 시점에 남긴 `tracing::warn!` 한
    /// 줄이 전부다 — 여기서 로그를 중복 남기지 않는다.
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
                    extra_buttons: Vec::new(),
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
                    extra_buttons: Vec::new(),
                },
            );
        }
    }

    /// macOS 에서 Full Disk Access 가 없어 보이면 안내 모달을 1 회 심는다.
    ///
    /// 파일 pre-warm 이 못 덮는 "다른 앱의 데이터" 프롬프트는 FDA 로만 사라지는데,
    /// FDA 는 앱이 요청할 수 없어 사용자가 직접 켜야 한다 — 원인을 모르면 계속
    /// 막히므로 발견성이 중요하다. 다만 성가시지 않도록 **평생 1 회**만 띄우고,
    /// 띄운 사실을 즉시 설정에 기록한다. 다시 보려면 설정에서 켠다.
    ///
    /// FDA 판정은 휴리스틱이라 오탐이 날 수 있다. 그래서 이 결과는 안내 표시
    /// 여부에만 쓰고 어떤 기능도 막지 않는다. macOS 외에서는 no-op.
    fn report_missing_full_disk_access(
        state: &mut crate::state::AppState,
        settings: &mut crate::settings::Settings,
    ) {
        if !crate::macos_permissions::wants_full_disk_access_notice(settings) {
            return;
        }
        crate::adapters::ui::info_modal::show_info_modal(
            state,
            crate::adapters::ui::info_modal::InfoModal {
                title: crate::i18n::t("macos_permissions.fda.title").to_string(),
                body: crate::i18n::t("macos_permissions.fda.body").to_string(),
                on_close: crate::adapters::ui::info_modal::InfoModalAction::Continue,
                extra_buttons: full_disk_access_notice_buttons(),
            },
        );
        crate::macos_permissions::mark_full_disk_access_notice_shown(settings);
    }

    /// IPC/stream 서버 시작 + 웹훅 리스너 init — `finish_boot` 의 첫 윈도우 등록
    /// 직전 1회 지점. 웹훅 bind 실패는 `state` 에 Warning 토스트로 반영한다.
    /// 부팅 때 사용자 파일(설정·레이아웃)을 읽지 못한 사실을 한 번 알린다.
    ///
    /// 로그만으로는 부족하다 — 사용자가 보는 것은 "설정이 초기화됐다" / "탭이 사라졌다"
    /// 이고, 원본이 어디로 갔는지(백업 경로)와 왜 저장이 멈췄는지를 알아야 복구할 수
    /// 있다. 웹훅 미기동과 같은 Warning 토스트 인프라를 재사용한다.
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
        // `engine.settings.origin` 이 아니라 부팅이 처음 읽었을 때의 값을 본다 — 그 사이
        // T2.5 의 theme 저장이 원본을 백업으로 옮기고 정상 파일을 써 놓기 때문이다.
        match settings_origin {
            SettingsOrigin::Clean => {}
            SettingsOrigin::Unparsable => {
                let path = tasty_settings::Settings::config_path().unwrap_or_default();
                let shown = tasty_utils::path::tilde_abbreviate(&path);
                // 위 T2.5 저장이 원본을 옮겼으면 그 자리는 이제 정상 파일이다. 아직도
                // 해석되지 않는다면 보존이 실패한 것(백업 자리 소진 등)이고 저장은 계속
                // 거부된다 — 그때 "보관했다" 고 말하면 사용자가 없는 파일을 찾는다.
                let key = if tasty_settings::Settings::file_is_unparsable(&path) {
                    "persistence.warn.settings_unparsable_blocked"
                } else {
                    "persistence.warn.settings_unparsable"
                };
                // `t_fmt` 가 아니라 `t_fmt_fit` 이다 — 긴 경로가 문구를 토스트 캡 밖으로
                // 밀어내면 잘려나가는 것이 문장 끝의 조치 안내다. 경로만 줄인다.
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
            // 완료 판정 전략 레지스트리 시드 — 훅 핸들러와 대칭 위치.
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
                // 부팅 화면에서의 창 닫기도 종료 상태 머신으로 보낸다 — WaitingEngine
                // 워커가 만든 plugin 자식 회수(S2)가 그 안에 있고, 회수 대기 동안
                // 부팅 창에 종료 프레임이 계속 그려진다.
                self.begin_shutdown(event_loop);
            }
            // 부팅 미완 — 나머지 이벤트(키/마우스/포커스 등)는 소비만 한다.
            // ApplyPendingLayoutRestore 의 bootstrap 전제(적용 전 다른 mutate 없음)
            // 보호. 부팅은 최대 ~1s 이므로 입력 드롭 체감 없음.
            _ => {}
        }
    }
}

/// FDA 안내 모달에 붙는 추가 버튼 — 시스템 설정의 전체 디스크 접근 권한 패널로
/// 바로 보낸다. 패널 위치를 글로 설명하는 것보다 한 번에 데려가는 편이 확실하다.
#[cfg(all(target_os = "macos", feature = "gui"))]
fn full_disk_access_notice_buttons() -> Vec<crate::adapters::ui::info_modal::InfoModalButton> {
    vec![crate::adapters::ui::info_modal::InfoModalButton {
        label: crate::i18n::t("macos_permissions.fda.open_settings").to_string(),
        action: crate::adapters::ui::info_modal::InfoModalButtonAction::OpenExternal(
            crate::macos_permissions::FULL_DISK_ACCESS_SETTINGS_URL.to_string(),
        ),
    }]
}

/// 비-macOS — 안내 자체가 뜨지 않으므로 버튼도 없다.
#[cfg(not(all(target_os = "macos", feature = "gui")))]
fn full_disk_access_notice_buttons() -> Vec<crate::adapters::ui::info_modal::InfoModalButton> {
    Vec::new()
}

#[cfg(test)]
mod boot_error_tests {
    use super::*;

    /// `boot_engine_error_info` 가 title/body/hint 를 서로 다른 세 진단 소스에서
    /// 읽는다(복붙으로 hint 에 title 키를 쓰는 류 실수 방지). pairwise-distinct +
    /// non-empty 는 i18n 초기화 여부와 무관하게 성립한다 — 미초기화면 세 키가 그대로,
    /// 초기화면 세 번역문이 오는데 둘 다 서로 다르다.
    ///
    /// 변이 검증: 셋 중 하나를 다른 것과 같은 소스로 바꾸면(예: `hint: title`)
    /// pairwise assert 가 깨진다.
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
