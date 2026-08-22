//! 종료 상태 머신 (`ShutdownPhase`) — 종료 cascade 의 동기 대기를 프레임 구동으로
//! 전개해, 대기 동안 종료 로딩 화면(워드마크 + **회전하는** 스피너 + 단계 문구)이
//! 계속 갱신되게 한다. 부팅 상태 머신([`crate::app::boot_machine`])의 대칭이다.
//!
//! # 왜 상태 머신인가
//!
//! 종료가 느린 것 자체는 이 모듈이 고치지 않는다(그건 plugin 종료 대기의 몫이다).
//! 문제는 종료 cascade 가 **단일 동기 콜스택**이라 그 2 초 남짓 동안 이벤트 루프로
//! 제어가 한 번도 돌아오지 않는다는 것이다 — 화면만 얹으면 스피너가 멈춘 정지
//! 프레임이 된다. 그래서 화면보다 먼저 **시퀀스를 프레임 단위로 쪼개는 것**이
//! 전제다.
//!
//! # 빠른 종료는 화면을 띄우지 않는다
//!
//! [`App::drive_shutdown_frame`] 은 한 번의 호출 안에서 **더 진행할 수 없을 때까지**
//! 스텝을 반복한다. 즉 대기가 없는 종료(plugin 0 개 — 실측 0.6ms)는 첫 구동에서
//! `Done` 까지 가므로 종료 화면이 **한 프레임도 그려지지 않는다**. 화면이 깜빡였다
//! 사라지는 것을 막는 최소 표시 시간이나 지연 표시 타이머가 따로 필요 없다 —
//! "기다릴 일이 있을 때만 보인다" 가 구조에서 나온다.
//!
//! # 창이 없으면 프레임을 돌리지 않는다
//!
//! macOS 최소화(창 파괴 + park)나 트레이 hide 상태에서 종료하면 그릴 창 자체가
//! 없다. 그 경우 [`App::begin_shutdown`] 은 상태 머신을 설치하지 않고 **그 자리에서
//! 블로킹으로** 전 스텝을 돌린다(= 이 변경 이전과 동일한 동작). 이벤트 루프가
//! 창 없이도 `about_to_wait` 를 계속 깨워 준다는 보장이 플랫폼마다 다르므로,
//! 보여 줄 화면이 없는 경로에서 구동 방식까지 바꿀 이유가 없다.
//!
//! # 종료 가드
//!
//! 종료 진행 중에는 steady-state 파이프라인(IPC 처리 / intent drain / plugin pump)을
//! 태우지 않는다(`about_to_wait` 의 종료 분기가 조기 return). 부팅 가드가 `AppEvent`
//! 를 **지연 후 재생**하는 것과 달리 종료 가드는 **폐기**한다 — 재생할 미래가 없다.
//! IPC 커맨드도 같은 이유로 이 구간에는 처리되지 않는다(연결은 받되 응답이 없다).
//!
//! 계측 마커(S1~S4)는 동기 시퀀스 시절과 **같은 지점에서 같은 이름으로** 발화한다.
//! 표는 [`docs/architecture/shutdown-sequence.md`].

use std::time::{Duration, Instant};

use winit::event_loop::ActiveEventLoop;

use crate::app::{App, shutdown_trace};

/// 종료 스텝 페이스 — `about_to_wait` 워치독의 재예약 간격. 부팅의
/// [`BOOT_FRAME_INTERVAL`](crate::app::boot_machine::BOOT_FRAME_INTERVAL) 과 같은
/// 60fps 케이던스.
pub(crate) const SHUTDOWN_FRAME_INTERVAL: Duration = Duration::from_millis(16);

/// 창이 없는 종료(블로킹 폴백)의 폴링 간격. 프레임 케이던스에 맞출 이유가 없으므로
/// (그릴 프레임이 없다) plugin 자식 회수 폴링과 같은 짧은 값을 쓴다.
const HEADLESS_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// 부팅 워커 회수 상한 — 동기 시절 `recv_timeout(5s)` 와 같은 값. 폴링으로 바뀌었을
/// 뿐 의미론(5 초까지 기다리고 포기)은 동일하다.
const BOOT_WORKER_RECLAIM_TIMEOUT: Duration = Duration::from_secs(5);

/// 종료 상태 머신의 phase. 스텝 의미론은 동기 시절 `shutdown_lifecycle_cascade` 와
/// 1:1 대응하며, 계측 마커도 같은 경계에서 발화한다.
///
/// **프레임을 넘길 수 있는 phase 는 둘뿐이다** — `ReclaimingBootWorker`(채널 폴링)
/// 와 `StoppingPlugins`(자식 회수 폴링). 나머지는 실측 1ms 미만이라 진입한 프레임
/// 안에서 다음 phase 로 넘어간다.
pub(crate) enum ShutdownPhase {
    /// S1 — `layout.json` 저장. `surface.closed` 발화 전에 끝나야 한다(layout 은
    /// *살아있는* 상태를 기록한다).
    SavingLayout,
    /// S2 — 부팅 중 종료 전용. `WaitingEngine` 워커가 spawn 한 plugin 자식을
    /// 잔존시키지 않으려면 워커 결과를 회수해야 한다. `try_recv` + deadline 폴링.
    ReclaimingBootWorker { started: Instant, deadline: Instant },
    /// S3 + S3b — `system.shutdown_initiated` 발화 → surface close cascade →
    /// 미뤄둔 observer sink 회수. 이어서 plugin 에 shutdown 요청까지 뿌린다.
    ClosingSurfaces,
    /// S4 — plugin 자식 회수 폴링. 요청은 `ClosingSurfaces` 말미에 이미 전부
    /// 나갔으므로 여기서는 기다리기만 한다(대기가 겹친다).
    StoppingPlugins,
    /// 전 단계 완료 — 다음 구동에서 `event_loop.exit()`.
    Done,
}

impl ShutdownPhase {
    /// phase → i18n 문구 키. 화면에 보이는 것은 **기다리는 대상**이므로, 프레임을
    /// 넘기지 않는 phase 의 키는 실제로는 거의 렌더되지 않는다(그래도 정의해 둔다 —
    /// 느린 디스크에서 S1 이 길어지는 경우가 있다).
    pub(crate) fn text_key(&self) -> &'static str {
        match self {
            Self::SavingLayout => "shutdown.phase_saving_layout",
            Self::ReclaimingBootWorker { .. } => "shutdown.phase_finishing_startup",
            Self::ClosingSurfaces => "shutdown.phase_closing_surfaces",
            // Done 은 렌더 대상이 아니지만(즉시 exit) 키를 비워 두면 호출부에
            // Option 분기가 생긴다 — 마지막 문구를 그대로 유지하는 편이 낫다.
            Self::StoppingPlugins | Self::Done => "shutdown.phase_stopping_plugins",
        }
    }
}

/// 종료 진행 중 상태 — `App.shutdown` 에 `Some` 으로 존재하는 동안이 "종료 중".
pub(crate) struct ShutdownState {
    pub(crate) phase: ShutdownPhase,
}

/// 스텝 1회의 결과.
enum StepOutcome {
    /// phase 가 진행됐다 — 같은 프레임에서 계속 돌린다.
    Advanced,
    /// 아직 조건이 안 됐다 — 이 프레임은 여기까지, 렌더 후 다음 프레임에 재시도.
    Waiting,
    /// 전 단계 완료.
    Finished,
}

impl App {
    /// 종료 시퀀스 진입점 — 네 진입 경로(`AppEvent::Shutdown`, quit modal 즉시 종료,
    /// `close_behavior=="quit"`, 부팅 화면에서 창 닫기)가 **공유**한다.
    ///
    /// 진입점을 하나로 모은 이유는 두 가지다: ① layout 저장(S1)이 반드시 close
    /// cascade 앞에 와야 한다는 순서 제약을 호출자별로 재현하지 않기 위해,
    /// ② 계측 t0 이 경로마다 어긋나면 `shutdown_total` 의 의미가 흔들리기 때문.
    ///
    /// **중복 진입은 무해하다.** 이미 종료 중이면 즉시 return 한다 — 종료 화면이 뜬
    /// 상태에서 Cmd+Q 를 다시 누르거나 트레이 quit 이 들어와도 phase 가 되감기지
    /// 않는다(진입점이 여럿이라 실제로 일어난다).
    pub(crate) fn begin_shutdown(&mut self, event_loop: &ActiveEventLoop) {
        if self.shutdown.is_some() {
            return;
        }
        shutdown_trace::mark_start();
        self.shutdown = Some(ShutdownState {
            phase: ShutdownPhase::SavingLayout,
        });

        if self.has_shutdown_render_target() {
            // 첫 스텝은 곧바로 돌린다 — 대기가 없는 종료는 여기서 그대로 끝나고
            // (화면 없음), 대기가 있으면 첫 종료 프레임이 이 호출에서 present 된다.
            self.drive_shutdown_frame(event_loop);
        } else {
            // 보여 줄 창이 없다 — 프레임 구동 대신 그 자리에서 끝까지 돌린다
            // (이 변경 이전과 동일한 동작).
            self.run_shutdown_blocking(event_loop);
        }
    }

    /// 종료 화면을 그릴 창이 하나라도 있는가. 부팅 중이면 부팅 창, 아니면 등록된
    /// view 들이 대상이다.
    fn has_shutdown_render_target(&self) -> bool {
        self.boot.is_some() || !self.view.views.is_empty()
    }

    /// 창 없는 종료 — 스텝을 끝까지 블로킹으로 돌린다. 렌더 대상이 없으므로
    /// 프레임 케이던스를 지킬 이유가 없고, 이벤트 루프가 창 없이도 계속 깨어난다는
    /// 보장이 플랫폼마다 다르므로 구동을 이벤트 루프에 맡기지 않는다.
    fn run_shutdown_blocking(&mut self, event_loop: &ActiveEventLoop) {
        loop {
            match self.shutdown_step() {
                StepOutcome::Advanced => {}
                StepOutcome::Waiting => std::thread::sleep(HEADLESS_POLL_INTERVAL),
                StepOutcome::Finished => break,
            }
        }
        self.finish_shutdown(event_loop);
    }

    /// 종료 1프레임: 더 진행할 수 없을 때까지 스텝을 돌린 뒤, 아직 남았으면 종료
    /// 로딩 프레임을 present 한다. 전 단계 완료 시 `event_loop.exit()`.
    ///
    /// **한 프레임에 여러 스텝을 도는 것이 핵심이다** — 대기가 없는 종료가 화면을
    /// 깜빡이지 않는 이유이자, 대기 phase 사이의 짧은 단계(S1/S3)가 프레임을
    /// 낭비하지 않는 이유다.
    pub(crate) fn drive_shutdown_frame(&mut self, event_loop: &ActiveEventLoop) {
        if self.shutdown.is_none() {
            return;
        }
        loop {
            match self.shutdown_step() {
                StepOutcome::Advanced => {}
                StepOutcome::Waiting => break,
                StepOutcome::Finished => {
                    self.finish_shutdown(event_loop);
                    return;
                }
            }
        }
        self.render_shutdown_frame();
    }

    /// 마지막 phase 도달 — `shutdown_total` 발화 후 이벤트 루프 탈출. 이 지점
    /// **이후**가 Drop tail 이며 창이 이미 사라져 어떤 화면으로도 덮을 수 없다.
    fn finish_shutdown(&mut self, event_loop: &ActiveEventLoop) {
        self.shutdown = None;
        if let Some(t0) = shutdown_trace::started_at() {
            tracing::info!(
                target: "tasty::shutdown",
                ms = shutdown_trace::elapsed_ms(t0),
                "shutdown_total (shutdown enter -> event_loop.exit())"
            );
        }
        event_loop.exit();
    }

    /// 현재 phase 1스텝.
    fn shutdown_step(&mut self) -> StepOutcome {
        let Some(sd) = self.shutdown.as_ref() else {
            return StepOutcome::Finished;
        };
        match sd.phase {
            ShutdownPhase::SavingLayout => self.shutdown_step_saving_layout(),
            ShutdownPhase::ReclaimingBootWorker { started, deadline } => {
                self.shutdown_step_reclaim_boot_worker(started, deadline)
            }
            ShutdownPhase::ClosingSurfaces => self.shutdown_step_closing_surfaces(),
            ShutdownPhase::StoppingPlugins => self.shutdown_step_stopping_plugins(),
            ShutdownPhase::Done => StepOutcome::Finished,
        }
    }

    /// phase 전이 — `shutdown` 이 사라진 뒤에 불리면 무해한 no-op.
    fn set_shutdown_phase(&mut self, phase: ShutdownPhase) {
        if let Some(sd) = self.shutdown.as_mut() {
            sd.phase = phase;
        }
    }

    /// S1 — layout 저장. 저장 대상은 살아있는 main view + parked engine 이므로,
    /// 부팅 중 종료(둘 다 비어 있음)에서는 자연히 no-op 이다(빈 layout 으로
    /// 덮어쓰지 않는다).
    fn shutdown_step_saving_layout(&mut self) -> StepOutcome {
        let t_flush = Instant::now();
        self.flush_layout_persistence(true);
        tracing::info!(
            target: "tasty::shutdown",
            ms = shutdown_trace::elapsed_ms(t_flush),
            "S1 layout_flush (SaveLayoutNow force — main + parked engine)"
        );
        // 부팅 워커가 도는 중이면 그 회수가 close cascade 보다 먼저다 — 워커가 만든
        // plugin 자식이 S4 의 회수 대상에 들어가야 잔존하지 않는다.
        let next = if self.boot_engine_worker_pending() {
            let now = Instant::now();
            ShutdownPhase::ReclaimingBootWorker {
                started: now,
                deadline: now + BOOT_WORKER_RECLAIM_TIMEOUT,
            }
        } else {
            ShutdownPhase::ClosingSurfaces
        };
        self.set_shutdown_phase(next);
        StepOutcome::Advanced
    }

    /// S2 — 부팅 워커 결과 폴링. 동기 시절의 `recv_timeout(5s)` 를 `try_recv` +
    /// deadline 으로 바꾼 것뿐이라 의미론은 같다(5 초까지 기다리고 포기).
    ///
    /// 회수한 `PluginManager` 는 그 자리에서 `shutdown_all`(블로킹) 하지 않고
    /// `self.plugin_manager` 에 장착한다 — 뒤이은 S4 가 폴링으로 회수하므로 대기
    /// 중에도 스피너가 계속 돈다. 동기 시절 "S4/S4a 가 S2 안에 중첩 발화" 하던
    /// 것이 이 변경으로 **S2 뒤에 나란히** 나온다.
    fn shutdown_step_reclaim_boot_worker(
        &mut self,
        started: Instant,
        deadline: Instant,
    ) -> StepOutcome {
        let reason = match self.try_recv_boot_engine_worker() {
            Ok(Some((engine, mgr))) => {
                drop(engine);
                self.adopt_reclaimed_plugin_manager(mgr);
                "reclaimed"
            }
            // 워커 panic 등으로 결과 없이 채널이 닫혔다 — 더 기다릴 것이 없다.
            Err(()) => "unreclaimed",
            Ok(None) => {
                if Instant::now() < deadline {
                    return StepOutcome::Waiting;
                }
                tracing::warn!("boot engine worker not reclaimed before exit: timeout");
                "unreclaimed"
            }
        };
        tracing::info!(
            target: "tasty::shutdown",
            ms = shutdown_trace::elapsed_ms(started),
            reason,
            "S2 boot_worker_reclaim (부팅 중 종료 전용, timeout 5s)"
        );
        self.set_shutdown_phase(ShutdownPhase::ClosingSurfaces);
        StepOutcome::Advanced
    }

    /// 회수한 매니저를 S4 의 회수 대상에 넣는다. 부팅 중(`WaitingEngine`)에는
    /// `self.plugin_manager` 가 구조적으로 `None` 이지만, 그렇지 않은 상황이 생기면
    /// 조용히 덮어쓰는 대신 그 자리에서 drop 한다 — `PluginProcess::drop` 이 자식을
    /// kill 하므로 잔존은 없고, 이미 장착된 매니저의 회수 대상만 잃지 않는다.
    fn adopt_reclaimed_plugin_manager(&mut self, mgr: crate::plugin::PluginManager) {
        if self.plugin_manager.is_none() {
            self.plugin_manager = Some(mgr);
        } else {
            tracing::warn!(
                "reclaimed boot plugin manager while one is already installed — dropping the reclaimed one"
            );
        }
    }

    /// S3 + S3b — plugin 에 종료를 알리고 surface 를 정리한 뒤, plugin shutdown
    /// 요청까지 이 스텝 안에서 전부 뿌린다.
    ///
    /// 요청 발송을 S4 대기와 같은 스텝에 두지 않는 이유는 채널 순서 계약이다 —
    /// shutdown 요청은 `dispatch_pending_surface_lifecycle` 이 같은 `req_tx` 에
    /// 넣어 둔 `surface.closed` **뒤에** 놓여야 한다.
    fn shutdown_step_closing_surfaces(&mut self) -> StepOutcome {
        self.emit_shutdown_initiated();
        self.shutdown_close_surfaces();
        self.shutdown_join_observer_sinks();
        self.begin_plugin_shutdown();
        self.set_shutdown_phase(ShutdownPhase::StoppingPlugins);
        StepOutcome::Advanced
    }

    /// S4 — plugin 자식 회수 폴링. `poll_shutdown_all` 이 완료 시 S4/S4a 를
    /// 발화한다(대기가 겹치므로 S4 는 개별 합이 아니라 최댓값에 수렴).
    fn shutdown_step_stopping_plugins(&mut self) -> StepOutcome {
        let done = match self.plugin_manager.as_mut() {
            Some(mgr) => mgr.poll_shutdown_all(),
            None => true,
        };
        if !done {
            return StepOutcome::Waiting;
        }
        self.set_shutdown_phase(ShutdownPhase::Done);
        StepOutcome::Advanced
    }

    /// 종료 로딩 프레임 present — 부팅 중이면 부팅 창, 아니면 **등록된 창 전부**.
    ///
    /// 창이 여럿일 때 하나만 그리고 나머지를 먼저 닫으면 사용자에겐 창이 하나씩
    /// 사라지는 것으로 보여 크래시와 구분되지 않는다. 전부 같은 화면으로 바꾸는
    /// 편이 "지금 종료 중" 이라는 신호로 더 정확하다.
    fn render_shutdown_frame(&mut self) {
        let Some(sd) = self.shutdown.as_ref() else {
            return;
        };
        let key = sd.phase.text_key();
        if let Some(boot) = self.boot.as_mut() {
            Self::present_loading_frame(&mut boot.gpu, &boot.window, key);
            boot.window.request_redraw();
            return;
        }
        for w in self.view.views.values_mut() {
            let base = w.base_mut();
            let window = base.winit.clone();
            Self::present_loading_frame(&mut base.gpu, &window, key);
            window.request_redraw();
        }
    }

    /// 로딩 프레임 1장 — surface 유실은 다음 프레임 재시도(부팅 구동과 같은 관례).
    fn present_loading_frame(
        gpu: &mut crate::gpu::GpuState,
        window: &winit::window::Window,
        phase_text_key: &str,
    ) {
        match gpu.render_loading(window, phase_text_key) {
            Ok(()) => {}
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                gpu.resize(window.inner_size());
            }
            Err(e) => tracing::warn!("shutdown loading frame render error: {e}"),
        }
    }

    /// 종료 중 `WindowEvent` — 렌더/크기 외 이벤트는 전부 소비한다. 종료는 취소할
    /// 수 없으므로(이 변경 이전에도 그랬다) 입력이 core 상태에 닿을 이유가 없다.
    pub(crate) fn handle_shutdown_window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        use winit::event::WindowEvent;
        match event {
            WindowEvent::RedrawRequested => self.drive_shutdown_frame(event_loop),
            WindowEvent::Resized(size) => {
                if let Some(boot) = self.boot.as_mut()
                    && boot.window.id() == id
                {
                    boot.gpu.resize(size);
                } else if let Some(w) = self.view.views.get_mut(&id) {
                    w.base_mut().gpu.resize(size);
                }
            }
            // 종료 중 창 닫기 요청은 이미 진행 중인 종료로 흡수된다(중복 진입 무해).
            _ => {}
        }
    }
}
