//! 종료를 단계별로 진행하고 대기가 필요하면 로딩 화면을 그린다.
//! 바로 끝나는 종료는 화면을 그리지 않으며, 그릴 창이 없으면 같은 단계를 블로킹으로 실행한다.
//! 저장·observer join 같은 동기 작업까지 비동기로 바꾸지는 않는다.
//! 종료 중에는 일반 IPC·Intent·플러그인 처리를 멈추고 큐에 쌓인 IPC 요청을 거절한다.
//! 계측 마커: docs/architecture/shutdown-sequence.md.

use std::time::{Duration, Instant};

use winit::event_loop::ActiveEventLoop;

use crate::app::{App, shutdown_trace};

/// about_to_wait에서 종료 단계를 다시 실행할 간격.
pub(crate) const SHUTDOWN_FRAME_INTERVAL: Duration = Duration::from_millis(16);

/// 그릴 창이 없을 때 자식·워커 회수 상태를 확인하는 간격.
const HEADLESS_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// 부팅 워커 결과를 기다릴 기한. 실제 확인 시점은 종료 단계가 다시 실행되는 때다.
const BOOT_WORKER_RECLAIM_TIMEOUT: Duration = Duration::from_secs(5);

/// OS port queries are not cancellable; expiry permits exit without claiming a joined worker.
const PORT_SCAN_RECLAIM_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) enum ShutdownPhase {
    /// 살아 있는 상태를 복원할 수 있도록 닫힘 알림보다 먼저 레이아웃을 저장한다.
    SavingLayout,
    /// 부팅 워커가 만든 플러그인도 종료 대상에 넣기 위해 결과를 회수한다.
    ReclaimingBootWorker {
        started: Instant,
        deadline: Instant,
    },
    ClosingSurfaces,
    StoppingPlugins,
    Done,
    /// exit 요청 뒤에도 winit이 남은 이벤트를 처리할 수 있어 종료 가드를 유지한다.
    /// App.shutdown을 지우면 정리가 끝난 상태로 일반 처리가 재개될 수 있다.
    Exited,
}

impl ShutdownPhase {
    pub(crate) fn text_key(&self) -> &'static str {
        match self {
            Self::SavingLayout => "shutdown.phase_saving_layout",
            Self::ReclaimingBootWorker { .. } => "shutdown.phase_finishing_startup",
            Self::ClosingSurfaces => "shutdown.phase_closing_surfaces",
            Self::StoppingPlugins | Self::Done | Self::Exited => "shutdown.phase_stopping_plugins",
        }
    }
}

pub(crate) struct ShutdownState {
    pub(crate) exit_code: u8,
    save_final_view: bool,
    pub(crate) phase: ShutdownPhase,
    final_view_sequence: Option<u64>,
    port_scan_deadline: Option<Instant>,
    profile_detection_deadline: Option<Instant>,
    screenshot_deadline: Option<Instant>,
    explorer_files_deadline: Option<Instant>,
    runner_stop_deadline: Instant,
}

enum StepOutcome {
    Advanced,
    Waiting,
    Finished,
}

impl App {
    /// 종료 시작 시각과 순서를 공유한다. 이미 종료 중이면 다시 시작하지 않는다.
    pub(crate) fn begin_shutdown(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(code) = self.take_pending_modal_exit() {
            self.begin_error_shutdown(event_loop, code);
        } else {
            self.start_shutdown(event_loop, 0, true);
        }
    }

    pub(crate) fn begin_error_shutdown(&mut self, event_loop: &ActiveEventLoop, code: u8) {
        if let Some(state) = self.state.shutdown.as_mut() {
            state.exit_code = code;
            state.save_final_view = false;
        } else {
            self.start_shutdown(event_loop, code, false);
        }
    }

    fn start_shutdown(
        &mut self,
        event_loop: &ActiveEventLoop,
        exit_code: u8,
        save_final_view: bool,
    ) {
        if self.state.shutdown.is_some() {
            return;
        }
        shutdown_trace::mark_start();
        self.state.shutdown = Some(ShutdownState {
            exit_code,
            save_final_view,
            phase: ShutdownPhase::SavingLayout,
            final_view_sequence: None,
            port_scan_deadline: Some(Instant::now() + PORT_SCAN_RECLAIM_TIMEOUT),
            profile_detection_deadline: Some(Instant::now() + Duration::from_secs(5)),
            screenshot_deadline: Some(Instant::now() + Duration::from_secs(5)),
            explorer_files_deadline: Some(Instant::now() + Duration::from_secs(5)),
            runner_stop_deadline: Instant::now() + Duration::from_secs(5),
        });

        for session in self.engines.all_sessions_mut() {
            // Begin stop now; the retained session receipt is observed by the shutdown phase.
            let _ = session.poll_runner_stop(&self.services.tasks);
        }
        self.port_scans.begin_shutdown();
        self.services.profile_detections.begin_shutdown();
        self.screenshot_workers.begin_shutdown();
        self.explorer_files.begin_shutdown();
        self.tutorial_progress.stop();

        // Native child views sit above the GPU loading frame. Normal redraws no
        // longer run after shutdown starts, so hide them before the first frame.
        for view in self.view.views.values_mut() {
            if let Some(main) = view.as_main_mut() {
                main.hide_webviews_for_shutdown();
            }
        }

        if self.has_shutdown_render_target() {
            self.drive_shutdown_frame(event_loop);
        } else {
            self.run_shutdown_blocking(event_loop);
        }
    }

    /// Exited 뒤에는 프레임을 예약하지 않지만 종료 가드는 유지한다.
    pub(crate) fn shutdown_needs_frames(&self) -> bool {
        self.state
            .shutdown
            .as_ref()
            .is_some_and(|sd| !matches!(sd.phase, ShutdownPhase::Exited))
    }

    fn has_shutdown_render_target(&self) -> bool {
        self.boot.is_some() || !self.view.views.is_empty()
    }

    /// 창이 없을 때는 플랫폼 이벤트 루프의 추가 깨움을 기다리지 않고 종료를 진행한다.
    fn run_shutdown_blocking(&mut self, event_loop: &ActiveEventLoop) {
        loop {
            self.reject_pending_ipc();
            match self.shutdown_step() {
                StepOutcome::Advanced => {}
                StepOutcome::Waiting => std::thread::sleep(HEADLESS_POLL_INTERVAL),
                StepOutcome::Finished => break,
            }
        }
        self.finish_shutdown(event_loop);
    }

    /// 대기 단계에 닿을 때까지 진행한다. 바로 끝나면 로딩 화면을 그리지 않는다.
    pub(crate) fn drive_shutdown_frame(&mut self, event_loop: &ActiveEventLoop) {
        let Some(sd) = self.state.shutdown.as_ref() else {
            return;
        };
        let exited = matches!(sd.phase, ShutdownPhase::Exited);
        self.reject_pending_ipc();
        if exited {
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

    /// exit를 요청한 뒤에도 종료 가드를 유지한다. 이후 App Drop 시간은 별도로 계측한다.
    fn finish_shutdown(&mut self, event_loop: &ActiveEventLoop) {
        // 마지막으로 큐를 비운 뒤 들어오는 연결은 서버 Drop까지 남을 수 있다.
        self.reject_pending_ipc();
        self.set_shutdown_phase(ShutdownPhase::Exited);
        if let Some(t0) = shutdown_trace::started_at() {
            tracing::info!(
                target: "tasty::shutdown",
                ms = shutdown_trace::elapsed_ms(t0),
                "shutdown_total (shutdown enter -> event_loop.exit())"
            );
        }
        event_loop.exit();
    }

    /// 일반 핸들러를 실행하지 않고 종료 오류를 돌려줘 정리된 상태를 건드리지 않는다.
    fn reject_pending_ipc(&mut self) {
        let Some(ipc) = self.hub.ipc_server.as_ref() else {
            return;
        };
        while let Ok(cmd) = ipc.try_recv() {
            let id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
            crate::ipc::server::send_response(
                &cmd.response_tx,
                crate::ipc::protocol::JsonRpcResponse::error(id, -32000, "host is shutting down"),
            );
        }
    }

    fn shutdown_step(&mut self) -> StepOutcome {
        let Some(sd) = self.state.shutdown.as_ref() else {
            return StepOutcome::Finished;
        };
        match sd.phase {
            ShutdownPhase::SavingLayout => self.shutdown_step_saving_layout(),
            ShutdownPhase::ReclaimingBootWorker { started, deadline } => {
                self.shutdown_step_reclaim_boot_worker(started, deadline)
            }
            ShutdownPhase::ClosingSurfaces => self.shutdown_step_closing_surfaces(),
            ShutdownPhase::StoppingPlugins => self.shutdown_step_stopping_plugins(),
            ShutdownPhase::Done | ShutdownPhase::Exited => StepOutcome::Finished,
        }
    }

    fn set_shutdown_phase(&mut self, phase: ShutdownPhase) {
        if let Some(sd) = self.state.shutdown.as_mut() {
            sd.phase = phase;
        }
    }

    /// 부팅 중에는 저장 대상 engine이 아직 없어 빈 레이아웃으로 덮어쓰지 않는다.
    fn shutdown_step_saving_layout(&mut self) -> StepOutcome {
        // Do not capture an authorized installation as though it were a committed surface.
        // Cleanup has a bounded receipt deadline and records Uncertain before the halt path.
        if self.journal.pauses_observation() && !self.journal.is_halted() {
            self.poll_journal_application();
            if self.journal.pauses_observation() && !self.journal.is_halted() {
                return StepOutcome::Waiting;
            }
        }
        for session in self.engines.all_sessions_mut() {
            crate::app::attach_activation::cancel_engine(
                &mut self.pending_server_attaches,
                session.id,
                &mut session.borrow_mut(),
                &self.stream_hub,
            );
        }
        let t_flush = Instant::now();
        if self
            .state
            .shutdown
            .as_ref()
            .is_some_and(|shutdown| shutdown.final_view_sequence.is_none())
        {
            // Error dialogs may describe failed persistence initialization. Do not replace
            // the last checkpoint with fallback presentation, but drain already accepted work.
            if self
                .state
                .shutdown
                .as_ref()
                .is_some_and(|state| state.save_final_view)
            {
                self.flush_layout_persistence(true);
                if !self.journal.is_halted() {
                    for session in self.engines.all_sessions_mut() {
                        if !session.runtime.settings.general.restore_layout
                            && let Some(binding) = session.journal_binding.clone()
                        {
                            self.journal.retire_engine(session.id, binding, false);
                        }
                    }
                }
            }
            self.state
                .shutdown
                .as_mut()
                .expect("shutdown phase")
                .final_view_sequence = Some(self.journal.latest_view_sequence());
        }
        self.poll_journal_application();
        if self.journal.has_pending_view_writes() || self.journal.has_pending_retirements() {
            return StepOutcome::Waiting;
        }
        tracing::info!(
            target: "tasty::shutdown",
            ms = shutdown_trace::elapsed_ms(t_flush),
            "S1 layout_flush (SaveLayoutNow force — main + parked engine)"
        );
        // 부팅 워커의 플러그인 매니저를 회수해야 뒤의 종료 대상에도 포함된다.
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

    /// 부팅 워커 결과를 회수해 매니저를 장착한다. 플러그인 종료 대기는 뒤의 S4에서 한다.
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
            // 결과 없이 채널이 닫혔으면 더 기다리지 않는다.
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

    /// 이미 장착된 매니저를 덮어쓰지 않는다. 중복 결과는 Drop의 자식 종료 처리를 따른다.
    fn adopt_reclaimed_plugin_manager(&mut self, mgr: crate::plugin::PluginManager) {
        if self.plugin_manager.is_none() {
            self.plugin_manager = Some(mgr);
        } else {
            tracing::warn!(
                "reclaimed boot plugin manager while one is already installed — dropping the reclaimed one"
            );
        }
    }

    /// 같은 req_tx에서 surface.closed가 shutdown 요청보다 먼저 처리돼야 한다.
    /// 플러그인이 닫힘에 따른 파일·자식 정리를 수행할 기회를 주기 위한 순서다.
    /// 모든 종료 요청을 여기서 보낸 뒤 S4에서 함께 기다린다. shutdown_channel_order가 호출 배치를 검사한다.
    fn shutdown_step_closing_surfaces(&mut self) -> StepOutcome {
        let remaining = self
            .engines
            .all_sessions_mut()
            .map(|session| session.poll_runner_stop(&self.services.tasks))
            .filter(|observation| {
                matches!(
                    observation,
                    tasty_task_runtime::RunnerStopObservation::Waiting
                )
            })
            .count();
        if remaining != 0 {
            if self
                .state
                .shutdown
                .as_ref()
                .is_some_and(|state| Instant::now() < state.runner_stop_deadline)
            {
                return StepOutcome::Waiting;
            }
            tracing::warn!(
                remaining,
                "runner shutdown timed out; scopes retain unjoined workers"
            );
        }
        self.emit_shutdown_initiated();
        self.shutdown_close_surfaces();
        self.shutdown_join_observer_sinks();
        self.remote.begin_shutdown();
        self.begin_plugin_shutdown();
        self.set_shutdown_phase(ShutdownPhase::StoppingPlugins);
        StepOutcome::Advanced
    }

    fn wait_for_port_scan_shutdown(&mut self) -> bool {
        let scans_remaining = self.port_scans.poll_shutdown();
        if scans_remaining != 0
            && let Some(deadline) = self
                .state
                .shutdown
                .as_ref()
                .and_then(|state| state.port_scan_deadline)
        {
            if Instant::now() < deadline {
                return true;
            }
            tracing::warn!(
                scans_remaining,
                "port scan shutdown timed out; workers remain unjoined"
            );
            if let Some(state) = self.state.shutdown.as_mut() {
                state.port_scan_deadline = None;
            }
        }

        false
    }

    fn wait_for_profile_shutdown(&mut self) -> bool {
        let detections_remaining = self.services.profile_detections.poll_shutdown();
        if detections_remaining != 0
            && let Some(deadline) = self
                .state
                .shutdown
                .as_ref()
                .and_then(|state| state.profile_detection_deadline)
        {
            if Instant::now() < deadline {
                return true;
            }
            tracing::warn!(
                detections_remaining,
                "profile detection shutdown timed out; workers remain unjoined"
            );
            if let Some(state) = self.state.shutdown.as_mut() {
                state.profile_detection_deadline = None;
            }
        }

        false
    }

    fn wait_for_screenshot_shutdown(&mut self) -> bool {
        let screenshots_remaining = self.screenshot_workers.poll_shutdown();
        if screenshots_remaining != 0
            && let Some(deadline) = self
                .state
                .shutdown
                .as_ref()
                .and_then(|state| state.screenshot_deadline)
        {
            if Instant::now() < deadline {
                return true;
            }
            tracing::warn!(
                screenshots_remaining,
                "screenshot shutdown timed out; workers remain unjoined"
            );
            if let Some(state) = self.state.shutdown.as_mut() {
                state.screenshot_deadline = None;
            }
        }
        false
    }
    fn wait_for_explorer_file_shutdown(&mut self) -> bool {
        let remaining = self.explorer_files.poll_shutdown()
            + self.local_reads.poll_shutdown()
            + self.tutorial_progress.poll_shutdown();
        if remaining != 0
            && let Some(deadline) = self
                .state
                .shutdown
                .as_ref()
                .and_then(|s| s.explorer_files_deadline)
        {
            if Instant::now() < deadline {
                return true;
            }
            tracing::warn!(
                remaining,
                "Explorer file shutdown timed out; worker remains unjoined and file work is not cancelled"
            );
            if let Some(state) = self.state.shutdown.as_mut() {
                state.explorer_files_deadline = None;
            }
        }
        false
    }
    fn shutdown_step_stopping_plugins(&mut self) -> StepOutcome {
        if self.wait_for_port_scan_shutdown()
            || self.wait_for_profile_shutdown()
            || self.wait_for_screenshot_shutdown()
            || self.wait_for_explorer_file_shutdown()
        {
            return StepOutcome::Waiting;
        }

        let remote = self.remote.shutdown_observation();
        match remote {
            tasty_remote::outbound::ShutdownObservation::Waiting => return StepOutcome::Waiting,
            tasty_remote::outbound::ShutdownObservation::Joined => {}
            other => tracing::warn!(
                ?other,
                "remote I/O retirement was not a confirmed clean join"
            ),
        }
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

    fn render_shutdown_frame(&mut self) {
        let Some(sd) = self.state.shutdown.as_ref() else {
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

    /// surface 유실은 다음 프레임에 다시 시도한다.
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

    /// 종료는 취소할 수 없어 렌더·크기 변경 외 입력은 처리하지 않는다.
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
            _ => {}
        }
    }
}
