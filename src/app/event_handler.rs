use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::WindowId;

use crate::adapters::ui::input::synthetic::is_synthetic_key_event;
use crate::app::timers::{Tick, min_deadline};
use crate::stall_watchdog::{self, Site};
use crate::view::ui::View;
use crate::view::{RepaintSource, ViewAction, ViewCtx};
use crate::{App, AppEvent};

impl ApplicationHandler<AppEvent> for App {
    #[allow(clippy::cognitive_complexity)] // complexity-exempt: 이벤트별 핸들러에 위임하는 평면 match이며 플랫폼 cfg를 포함한다. 분기를 한곳에서 볼 수 있도록 유지한다.
    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: AppEvent) {
        let _stall_guard = stall_watchdog::Guard::enter(Site::UserEvent);
        // 종료 중에는 정리된 상태를 다시 건드리지 않도록 새 이벤트를 버린다.
        if self.shutdown.is_some() {
            return;
        }

        // 부팅 중 출력 이벤트를 소비하면 engine이 아직 views에 없어 wake를 잃을 수 있다. 완료 뒤 재생한다.
        if let Some(boot) = self.boot.as_mut()
            && !matches!(event, AppEvent::Shutdown | AppEvent::QuitRequested)
        {
            boot.pending_events.push(event);
            return;
        }
        match event {
            AppEvent::CreateWindow(origin, completion) => {
                let outcome = self.create_new_window(event_loop, origin);
                if let Some(completion) = completion {
                    completion.reply_window_create(outcome.map(u64::from));
                }
            }
            AppEvent::RunLuaScript { source, name } => {
                if let Some(engine) = self.lua_engine.as_ref() {
                    engine.run_script(&source, Some(&name));
                } else {
                    tracing::warn!(target: "tasty_lua", "RunLuaScript dropped — lua engine unavailable");
                }
            }
            AppEvent::OpenSettings => {
                self.open_settings_modal(event_loop);
            }
            AppEvent::OpenPlugins => {
                self.open_plugins_modal(event_loop);
            }
            AppEvent::TerminalOutput(surface_id) => self.handle_terminal_output(surface_id),
            AppEvent::IpcReady => {
                // IPC 연속 이벤트가 타이머·입력·렌더 처리를 막지 않도록 회차 간격을 지킨다.
                if self
                    .ipc_pacer
                    .event_may_run_round(std::time::Instant::now())
                    && self.process_ipc()
                    && let Some(w) = self.focused_window_mut()
                {
                    w.mark_dirty();
                }
            }
            AppEvent::StreamReady => {
                let outcome = self.stream_hub.pump_inbound(&self.stream_inbound_rx);
                self.apply_stream_outcome(outcome);
            }
            #[cfg(all(windows, feature = "gui"))]
            AppEvent::SystemResumed => {
                self.resume_health_pass();
            }
            AppEvent::EguiRepaint { window_id } => {
                // egui viewport는 창마다 ROOT이므로 winit 창 ID로 대상을 찾는다.
                if let Some(w) = self.view.views.get_mut(&window_id) {
                    // 애니메이션의 연속 repaint 요청에도 상한을 적용한다.
                    w.mark_dirty_from(RepaintSource::EguiAnimation);
                }
                // 아직 views에 등록되지 않은 부팅 창의 요청은 여기서 처리하지 않는다.
            }
            AppEvent::Shutdown => {
                self.begin_shutdown(event_loop);
            }
            AppEvent::Minimize => self.handle_minimize(),
            AppEvent::QuitRequested => {
                self.handle_quit_requested(event_loop);
            }
            AppEvent::CloseWindow(id) => {
                self.request_close_window(id, event_loop);
            }
            // Windows·Linux는 숨긴 창을 재사용한다. macOS는 parked 상태로 새 창을 만든다.
            #[cfg(any(windows, target_os = "linux"))]
            AppEvent::TrayShowWindow => {
                for w in self.view.views.values() {
                    w.base().winit.set_visible(true);
                    w.base().winit.set_minimized(false);
                    w.base().winit.focus_window();
                }
                tracing::info!(
                    "restored {} window(s) from system tray",
                    self.view.views.len()
                );
            }
            // 타이머 실행은 이어지는 about_to_wait가 담당한다.
            AppEvent::TimerTick => {}
            AppEvent::AttachClientData => {
                self.apply_attach_client_output();
            }
            AppEvent::AutoAttachReady => {
                self.drain_auto_attach_results();
            }
            AppEvent::ScreenshotCaptureReady => {
                self.drain_screenshot_capture_results();
            }
            AppEvent::ImageUploadReady => {
                self.drain_image_upload_results();
            }
            AppEvent::TransferProgressTick => {
                self.drain_transfer_progress();
            }
            AppEvent::IdentifyDone {
                request_id,
                target,
                detector,
                origin_surface_id,
                dispatch_origin,
                ignore_size_limit,
            } => self.handle_identify_done(
                request_id,
                target,
                detector,
                origin_surface_id,
                dispatch_origin,
                ignore_size_limit,
            ),
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let _stall_guard = stall_watchdog::Guard::enter(Site::Resumed);
        // resumed가 다시 호출돼도 부팅 중 아직 views가 비어 있는 창을 중복 생성하지 않는다.
        if !self.view.views.is_empty()
            || self.shell_setup_gpu.is_some()
            || self.boot.is_some()
            || self.boot_error_gpu.is_some()
        {
            return;
        }

        #[cfg(target_os = "macos")]
        crate::macos_delegate::inject_delegate_methods();

        let boot_t0 = std::time::Instant::now();

        // 첫 로딩 프레임을 그리기 전에 OS 기본 배경이 드러나지 않도록 hidden으로 만든다.
        let window = Self::boot_create_hidden_window(event_loop, boot_t0);

        let mut init_settings = Self::boot_load_normalized_settings();

        let gpu = self.try_init_boot_gpu(&window, &init_settings.appearance);

        let (window, gpu) = match self.enter_shell_setup_if_needed(&mut init_settings, window, gpu)
        {
            Some(pair) => pair,
            None => return,
        };

        window.set_ime_allowed(true);
        self.begin_boot(window, gpu, init_settings, boot_t0, true);

        #[cfg(windows)]
        crate::jump_list::setup_jump_list();

        // 창을 다시 만들더라도 트레이는 하나만 유지한다. 생성할 수 없으면 taskbar·dock 최소화로 동작한다.
        #[cfg(all(
            any(windows, target_os = "macos", target_os = "linux"),
            feature = "gui"
        ))]
        self.ensure_tray_icon_once();

        tracing::info!(
            target: "tasty::boot",
            ms = boot_t0.elapsed().as_secs_f64() * 1000.0,
            "resumed_total (T1~T2 + T2.5 + 첫 로딩 프레임 — 상태 머신 전개로 T2.6~T6 은 boot_total 로 이동)"
        );
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        // GPU 렌더에서 멈춘 경우를 다른 콜백의 지연과 구별한다.
        let _stall_guard =
            stall_watchdog::Guard::enter(if matches!(event, WindowEvent::RedrawRequested) {
                Site::Redraw
            } else {
                Site::WindowEvent
            });
        // 포커스를 얻거나 잃을 때 winit이 합성한 키는 사용자가 이 창에서 누른 입력이 아니다.
        // 모든 창·모드로 보내기 전에 제외해 단축키·PTY·egui에 전달되지 않게 한다.
        if is_synthetic_key_event(&event) {
            return;
        }

        if self.boot_error_mode {
            self.handle_boot_error_window_event(event);
            return;
        }

        if self.shell_setup_mode {
            self.handle_shell_setup_window_event(event_loop, event);
            return;
        }

        if self.shutdown.is_some() {
            self.handle_shutdown_window_event(event_loop, id, event);
            return;
        }

        if self.boot.is_some() {
            self.handle_boot_window_event(event_loop, event);
            return;
        }

        if let Some(modal_id) = self.view.active_modal_id
            && id == modal_id
        {
            self.handle_active_modal_window_event(event_loop, id, event);
            return;
        }

        if let WindowEvent::CloseRequested = &event {
            self.request_close_window(id, event_loop);
            return;
        }

        // 첫 Focused 이벤트를 창 map의 신호로 사용해 에이전트 창의 초기 포커스 힌트를 지운다.
        if let WindowEvent::Focused(_) = &event
            && self.pending_focus_hint_clear.remove(&id)
            && let Some(view) = self.view.views.get(&id)
            && let Err(e) =
                crate::platform::window_stacking::clear_initial_focus_hint(&view.base().winit)
        {
            tracing::warn!("agent window: clearing the initial focus hint failed: {e}");
        }

        if let WindowEvent::Focused(true) = &event {
            self.handle_window_focused(id);
        }

        if self.view.is_modal_active() {
            let is_mouse_press = matches!(
                &event,
                WindowEvent::MouseInput {
                    state: winit::event::ElementState::Pressed,
                    ..
                }
            );
            if is_mouse_press {
                self.trigger_modal_shake();
            }
        }

        // 포커스된 plugin surface의 단축키를 호스트 action보다 먼저 처리한다.
        let plugin_consumed = if let WindowEvent::KeyboardInput { event: ke, .. } = &event {
            self.try_plugin_shortcut(id, ke)
        } else {
            false
        };
        if plugin_consumed {
            return;
        }

        self.dispatch_window_event_to_view(event_loop, id, event);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let _stall_guard = stall_watchdog::Guard::enter(Site::AboutToWait);
        // 부팅·종료 중에는 일반 타이머를 처리하지 않고 각 상태 머신이 대기를 정한다.
        // 평상시 대기 시각은 말미에서 타이머·지연 repaint를 함께 반영한다.

        // exit 요청 뒤에도 콜백이 올 수 있어 종료 가드를 유지한다.
        // 종료 단계는 새 IPC를 실행하지 않고 종료 중이라는 응답을 보낸다.
        if self.shutdown.is_some() {
            self.drive_shutdown_frame(event_loop);
            event_loop.set_control_flow(if self.shutdown_needs_frames() {
                winit::event_loop::ControlFlow::WaitUntil(
                    std::time::Instant::now()
                        + crate::app::shutdown_machine::SHUTDOWN_FRAME_INTERVAL,
                )
            } else {
                winit::event_loop::ControlFlow::Wait
            });
            return;
        }

        // redraw가 오지 않아도 다음 부팅 단계를 진행하도록 대기를 예약한다.
        if self.boot.is_some() {
            self.drive_boot_frame(event_loop);
            event_loop.set_control_flow(if self.boot.is_some() {
                winit::event_loop::ControlFlow::WaitUntil(
                    std::time::Instant::now() + crate::app::boot_machine::BOOT_FRAME_INTERVAL,
                )
            } else {
                winit::event_loop::ControlFlow::Wait
            });
            return;
        }

        // Lua 자동실행 재진입 상태는 이번 회차의 모든 이벤트 처리 전에 갱신한다.
        self.lua_autofire.checkpoint();
        self.ipc_pacer.loop_reached_about_to_wait();

        // 여기서 바뀐 상태와 이벤트가 같은 회차의 후속 처리에 포함되도록 타이머를 먼저 실행한다.
        let now = std::time::Instant::now();
        for key in self.timers.drain_due(now) {
            match key {
                Tick::Busy => {
                    self.poll_busy_states();
                    self.poll_global_hooks();
                    self.poll_idle_timeout_hooks();
                }
                Tick::AttachView => self.poll_attach_views(),
                Tick::LayoutFlush => self.flush_layout_persistence(false),
                Tick::DagGraph(sid) => self.mark_dag_graph_window_dirty(sid),
                Tick::DagListPopup => self.mark_dag_list_popup_windows_dirty(),
                // 이 타이머는 루프만 깨우며 아래 poll_auto_attach가 재연결 여부를 판단한다.
                Tick::Reconnect(_) => {}
                // 아래의 메뉴 폴링이 실행되도록 깨운다.
                Tick::NativeMenu => {}
                // Linux의 별도 WebView 이벤트 큐를 확인하도록 깨운다.
                Tick::WebviewKeyPoll => {}
                // 접근 시점의 lazy 정리를 보완한다.
                Tick::PtySweep => self.poll_pty_sweep(),
                Tick::CaptureSweep => self.poll_capture_sweep(),
                Tick::LogPrune => self.poll_log_prune(),
            }
        }

        if self.process_ipc()
            && let Some(w) = self.focused_window_mut()
        {
            w.mark_dirty();
        }

        // 방금 IPC가 만든 attach 요청을 이번 회차에 처리한다.
        self.dispatch_pending_gui_attach();

        // 사용자가 닫은 mirror의 연결도 정리해야 원격 점유가 남지 않는다.
        self.detach_orphaned_mirror_sessions();

        self.dispatch_pending_structural_forwards();

        self.dispatch_pending_resize_forwards();

        self.dispatch_pending_list_dir_forwards();
        self.dispatch_pending_git_query_forwards();
        self.dispatch_pending_markdown_content_forwards();
        self.dispatch_pending_mesh_full_resend_forwards();
        self.dispatch_pending_attention_clear_forwards();

        self.dispatch_pending_mesh_context_forwards();
        self.dispatch_pending_mesh_input_forwards();

        self.poll_auto_attach();

        self.poll_screenshot_captures();

        self.poll_image_uploads();

        let hello_pairs = if let Some(ref mut mgr) = self.plugin_manager {
            mgr.pump(now)
        } else {
            Vec::new()
        };
        self.finalize_plugin_hello(hello_pairs);
        self.record_plugin_rss_samples_if_present();
        self.forward_mesh_frames_for_parked();
        self.mark_invalidated_surfaces_dirty();
        self.mark_invalidated_popups_dirty();
        self.mark_invalidated_banners_dirty();
        self.process_plugin_ipc_calls();
        self.dispatch_pending_surface_lifecycle();
        self.dispatch_pending_host_events();
        self.dispatch_pending_memory_changes();
        self.dispatch_pending_agent_events();
        self.dispatch_pending_tool_events();
        self.dispatch_pending_palette_plugin_commands();
        self.dispatch_pending_intents();
        self.publish_lua_snapshot();
        self.dispatch_pending_lua_commands();
        self.dispatch_pending_popup_opens();
        self.dispatch_pending_handler_ipc();
        self.dispatch_pending_picker_results();
        self.dispatch_pending_file_picker_results();
        self.dispatch_pending_script_confirm();
        self.dispatch_plugin_popup_events();
        self.process_plugins_window_actions();
        self.process_pending_open_preset_window(event_loop);

        self.poll_tray_menu_events();

        // 네이티브 WebView의 키 이벤트는 winit KeyboardInput과 별도 경로로 들어온다.
        let needs_key_poll = self.pump_webview_key_events();
        // Linux GDK는 별도 연결로 이벤트를 받아 winit을 깨우지 못하므로 폴링을 예약한다.
        crate::app::timers::reschedule_webview_key_poll(
            &mut self.timers,
            needs_key_poll,
            std::time::Instant::now(),
        );

        self.poll_pending_native_menus();

        // 긴 회차 뒤에도 미래 시각을 예약하도록 현재 시각을 다시 읽는다.
        let has_pending_menu = self.any_pending_native_menu();
        crate::app::timers::reschedule_pending_menu_poll(
            &mut self.timers,
            has_pending_menu,
            std::time::Instant::now(),
        );

        self.tick_modal_shake();

        let dirty_since = self.earliest_layout_dirty_since();
        crate::app::timers::sync_layout_flush_timer(&mut self.timers, dirty_since, now);

        self.sync_dag_poll_timers(now);

        self.sync_reconnect_timers(now);

        self.flush_pending_pty_resizes();

        // 타이머 대기를 먼저 정한 뒤 지연 repaint 시각과 이른 쪽으로 합친다.
        self.sync_timer_control_flow(event_loop);

        // 이번 회차의 모든 dirty 요청을 확인한 뒤 미룬 repaint를 실행·예약해야 한다.
        self.drive_deferred_repaints(event_loop);
    }
}

impl App {
    /// hidden 상태의 첫 창을 만든다. 로딩·shell setup 경로가 렌더를 시도한 뒤 표시한다.
    fn boot_create_hidden_window(
        event_loop: &ActiveEventLoop,
        boot_t0: std::time::Instant,
    ) -> std::sync::Arc<winit::window::Window> {
        use winit::window::WindowAttributes;
        let mut attrs = WindowAttributes::default()
            .with_visible(false)
            .with_title(if cfg!(debug_assertions) {
                "Tasty (Debug)"
            } else {
                "Tasty"
            })
            .with_inner_size(winit::dpi::LogicalSize::new(1280, 720))
            .with_min_inner_size(winit::dpi::LogicalSize::new(640, 480));
        if let Some(icon) = crate::app_icon::winit_window_icon() {
            attrs = attrs.with_window_icon(Some(icon));
        }
        attrs = crate::platform::window_chrome::apply_csd_attributes(attrs);
        // 표시할 창이 없으면 오류를 로그로 남기고 실패 코드로 종료한다.
        let window = match event_loop.create_window(attrs) {
            Ok(w) => std::sync::Arc::new(w),
            Err(e) => {
                tracing::error!(
                    "boot window creation failed: {e}\n{}\n{}\n{}",
                    crate::i18n::t("boot.window_error.title"),
                    crate::i18n::t_fmt("boot.window_error.body", &e.to_string()),
                    crate::i18n::t("boot.window_error.hint"),
                );
                std::process::exit(1);
            }
        };
        tracing::info!(
            target: "tasty::boot",
            ms = boot_t0.elapsed().as_secs_f64() * 1000.0,
            "T1 window_create (resumed enter -> create_window return)"
        );
        window
    }

    /// GPU 초기화 전에 설정값을 정규화하고 바뀐 설정의 저장을 시도한다.
    fn boot_load_normalized_settings() -> crate::settings::Settings {
        let mut settings = crate::settings::Settings::load();
        let normalize_report = settings.normalize();
        if normalize_report.changed
            && let Err(e) = settings.save()
        {
            tracing::warn!("failed to persist normalized settings: {e}");
        }
        settings
    }

    /// 어댑터 부재는 진단 후 실패 종료한다. 그 밖의 오류는 panic으로 보고한다.
    /// 엔진 초기화 실패처럼 창·GPU로 오류를 그릴 수 있는 경로와 구별한다.
    fn try_init_boot_gpu(
        &mut self,
        window: &std::sync::Arc<winit::window::Window>,
        appearance: &crate::settings::AppearanceSettings,
    ) -> crate::gpu::GpuState {
        let t2 = std::time::Instant::now();
        let gpu = match self.create_gpu_state(window.clone(), appearance) {
            Ok(gpu) => gpu,
            Err(e) if e.downcast_ref::<crate::app::NoGpuAdapter>().is_some() => {
                tracing::error!(
                    "boot gpu init failed: no compatible adapter\n{}\n{}\n{}",
                    crate::i18n::t("boot.gpu_error.title"),
                    crate::i18n::t("boot.gpu_error.body"),
                    crate::i18n::t("boot.gpu_error.hint"),
                );
                std::process::exit(1);
            }
            Err(e) => panic!("failed to initialize GPU: {e}"),
        };
        tracing::info!(
            target: "tasty::boot",
            ms = t2.elapsed().as_secs_f64() * 1000.0,
            "T2 gpu_init (create_gpu_state)"
        );
        gpu
    }

    /// 셸을 사용할 수 없고 bash도 찾지 못하면 setup 화면으로 소유권을 옮겨 None을 반환한다.
    fn enter_shell_setup_if_needed(
        &mut self,
        settings: &mut crate::settings::Settings,
        window: std::sync::Arc<winit::window::Window>,
        gpu: crate::gpu::GpuState,
    ) -> Option<(std::sync::Arc<winit::window::Window>, crate::gpu::GpuState)> {
        if settings.general.is_shell_valid() {
            return Some((window, gpu));
        }
        if let Some(detected) = crate::settings::GeneralSettings::detect_bash() {
            tracing::info!("configured shell invalid; auto-detected bash at {detected}");
            settings.general.shell = detected;
            if let Err(e) = settings.save() {
                tracing::warn!("failed to save auto-detected shell: {e}");
            }
            return Some((window, gpu));
        }
        self.enter_shell_setup_mode(window, gpu);
        None
    }

    /// setup 첫 렌더가 실패해도 창은 표시해 숨긴 채 남지 않게 한다.
    fn enter_shell_setup_mode(
        &mut self,
        window: std::sync::Arc<winit::window::Window>,
        mut gpu: crate::gpu::GpuState,
    ) {
        tracing::warn!("bash not found; entering shell setup mode");
        self.shell_setup_mode = true;
        self.shell_setup_path = String::new();
        if let Err(e) = gpu.render_shell_setup(&window, &mut self.shell_setup_path) {
            tracing::warn!("shell setup first frame render failed: {e} — showing window anyway");
        }
        window.set_visible(true);
        self.shell_setup_gpu = Some(gpu);
        self.shell_setup_window = Some(window);
    }

    /// 창 복원 때 트레이를 다시 만들지 않는다. 생성 불가는 일반 최소화로 처리한다.
    #[cfg(all(
        any(windows, target_os = "macos", target_os = "linux"),
        feature = "gui"
    ))]
    fn ensure_tray_icon_once(&mut self) {
        if self.tray_icon.is_none()
            && let Some((tray, ids)) = crate::system_tray::create_tray_icon()
        {
            self.tray_icon = Some(tray);
            self.tray_menu_ids = Some(ids);
        }
    }

    /// 플러그인 RSS 관측은 첫 MainView의 이상 감지기에 전달한다.
    fn record_plugin_rss_samples_if_present(&mut self) {
        if let Some(mgr) = self.plugin_manager.as_mut() {
            let rss_samples = mgr.take_rss_samples();
            if !rss_samples.is_empty()
                && let Some(main) = self.view.views.values_mut().find_map(|w| w.as_main_mut())
            {
                crate::adapters::ipc::handler::record_plugin_rss_samples(
                    &self.core,
                    &mut main.state,
                    &mut main.core_state,
                    &rss_samples,
                );
            }
        }
    }

    /// 창 없는 engine은 redraw가 돌지 않아 공용 mesh 구동·전송 함수를 직접 호출한다.
    /// 복원되지 않은 다른 engine도 계속 처리하도록 parked 항목을 모두 순회한다.
    fn forward_mesh_frames_for_parked(&mut self) {
        if let Some(ref mgr) = self.plugin_manager {
            for (_, engine) in self.parked_states.iter_mut() {
                crate::plugin_bridge::mesh_forward::forward_mesh_frames_for_engine(
                    engine,
                    mgr,
                    &self.stream_hub,
                );
            }
        }
    }

    /// 플러그인이 무입력 상태의 변경을 알리면 다음 렌더에서 다시 전달하도록 표시한다.
    fn mark_invalidated_surfaces_dirty(&mut self) {
        let invalidated_surfaces = self
            .plugin_manager
            .as_mut()
            .map(|mgr| mgr.take_invalidated_surfaces())
            .unwrap_or_default();
        if invalidated_surfaces.is_empty() {
            return;
        }
        for w in self.view.views.values_mut() {
            let touched = match w.as_main_mut() {
                Some(main) => {
                    // any는 첫 true에서 멈추므로 모든 surface를 표시할 수 없다.
                    let mut any_touched = false;
                    for &sid in &invalidated_surfaces {
                        if main.mark_surface_invalidated(sid) {
                            any_touched = true;
                        }
                    }
                    any_touched
                }
                None => false,
            };
            if touched {
                w.mark_dirty();
            }
        }
    }

    /// popup의 소유 창을 모르므로 모든 MainView에 repaint 예약을 전달한다.
    fn mark_invalidated_popups_dirty(&mut self) {
        let invalidated_popups = self
            .plugin_manager
            .as_mut()
            .map(|mgr| mgr.take_invalidated_popups())
            .unwrap_or_default();
        if invalidated_popups.is_empty() {
            return;
        }
        for main in self.main_windows_iter_mut() {
            for &iid in &invalidated_popups {
                main.state.plugin_mesh_popup_pending_repaint.insert(iid);
            }
            main.mark_dirty();
        }
    }

    /// banner의 소유 창을 모르므로 모든 MainView에 repaint 예약을 전달한다.
    fn mark_invalidated_banners_dirty(&mut self) {
        let invalidated_banners = self
            .plugin_manager
            .as_mut()
            .map(|mgr| mgr.take_invalidated_banners())
            .unwrap_or_default();
        if invalidated_banners.is_empty() {
            return;
        }
        for main in self.main_windows_iter_mut() {
            for &iid in &invalidated_banners {
                main.state.plugin_mesh_banner_pending_repaint.insert(iid);
            }
            main.mark_dirty();
        }
    }

    /// Linux GTK에는 별도 메인 루프가 없어 winit에서 메뉴 이벤트를 처리한다.
    fn poll_tray_menu_events(&mut self) {
        // 트레이가 없어도 열린 GTK 메뉴는 redraw 사이에 계속 이벤트를 처리해야 한다.
        #[cfg(target_os = "linux")]
        if self.tray_icon.is_some() || self.any_pending_native_menu() {
            crate::system_tray::pump_gtk_events();
        }

        #[cfg(all(
            any(windows, target_os = "macos", target_os = "linux"),
            feature = "gui"
        ))]
        if let Some(ref ids) = self.tray_menu_ids
            && let Some(menu_id) = crate::system_tray::poll_menu_event()
        {
            if menu_id == ids.show_window {
                // 기존 MainView가 있으면 다시 표시하고, 모두 parked 상태일 때만 새 창을 만든다.
                #[cfg(target_os = "macos")]
                {
                    let target = self
                        .view
                        .focused_view_id
                        .filter(|id| {
                            self.view
                                .views
                                .get(id)
                                .is_some_and(|w| w.as_main().is_some())
                        })
                        .or_else(|| {
                            self.view
                                .views
                                .iter()
                                .find(|(_, w)| w.as_main().is_some())
                                .map(|(id, _)| *id)
                        });
                    if let Some(id) = target {
                        if let Some(w) = self.view.views.get(&id) {
                            w.base().winit.set_minimized(false);
                            w.base().winit.focus_window();
                        }
                        self.view.focused_view_id = Some(id);
                        tracing::info!("tray show: focusing existing main window");
                    } else {
                        tracing::info!("tray show: no live window, creating");
                        crate::shortcuts::send_app_event(
                            &self.view.proxy,
                            AppEvent::CreateWindow(
                                crate::app::event::WindowRequestOrigin::User,
                                None,
                            ),
                        );
                    }
                }
                #[cfg(any(windows, target_os = "linux"))]
                crate::shortcuts::send_app_event(&self.view.proxy, AppEvent::TrayShowWindow);
            } else if menu_id == ids.new_window {
                crate::shortcuts::send_app_event(
                    &self.view.proxy,
                    AppEvent::CreateWindow(crate::app::event::WindowRequestOrigin::User, None),
                );
            } else if menu_id == ids.quit {
                crate::shortcuts::send_app_event(&self.view.proxy, AppEvent::Shutdown);
            }
        }
    }

    /// 미뤄 둔 PTY resize를 처리한다. 남은 요청이 있으면 다음 redraw를 요청한다.
    fn flush_pending_pty_resizes(&mut self) {
        let mut any_pending = false;
        for w in self.view.views.values_mut() {
            if let Some(main) = w.as_main_mut()
                && crate::core::Core::flush_pty_resizes(&mut main.core_state)
            {
                any_pending = true;
            }
        }
        for (_, engine) in self.parked_states.iter_mut() {
            if crate::core::Core::flush_pty_resizes(engine) {
                any_pending = true;
            }
        }
        if any_pending {
            for w in self.view.views.values() {
                w.base().winit.request_redraw();
            }
        }
    }

    /// 출력을 비우기 전에 해당 surface 또는 전역 wake 중복 방지를 해제한다.
    fn note_drained_all(&self, surface_id: Option<u32>) {
        for w in self.view.views.values() {
            if let Some(main) = w.as_main()
                && let Some(factory) = main.core_state.waker_factory.as_ref()
            {
                factory.note_drained(surface_id);
            }
        }
        for (_, engine) in self.parked_states.iter() {
            if let Some(factory) = engine.waker_factory.as_ref() {
                factory.note_drained(surface_id);
            }
        }
    }

    /// surface ID가 있으면 해당 engine을, 없으면 모든 창·parked engine의 출력을 처리한다.
    fn handle_terminal_output(&mut self, surface_id: Option<u32>) {
        use crate::app::dispatch_domain::DispatchSource;
        use crate::core::intent::CoreEvent;
        // 비우는 동안 도착한 wake가 중복으로 버려지지 않도록 먼저 해제한다.
        self.note_drained_all(surface_id);
        let core = &mut self.core;
        let views = &mut self.view.views;
        let parked_states = &mut self.parked_states;
        let mut pending: Vec<(DispatchSource, Vec<CoreEvent>)> = Vec::new();
        if let Some(sid) = surface_id {
            let mut found = false;
            for (wid, w) in views.iter_mut() {
                let Some(main) = w.as_main_mut() else {
                    continue;
                };
                if main.core_state.find_terminal_by_id(sid).is_some() {
                    let outcome = core.process_pty_output(&mut main.core_state, sid);
                    if !outcome.events.is_empty() {
                        pending.push((DispatchSource::Main(*wid), outcome.events));
                    }
                    main.recalc_ime_preedit_anchor();
                    // 출력 처리는 끝냈다. 보이지 않는 surface는 전환할 때 새로 그리므로 지금 redraw하지 않는다.
                    if main.is_surface_visible(sid) {
                        main.mark_dirty_from(RepaintSource::TerminalOutput);
                    }
                    found = true;
                    break;
                }
            }
            if !found {
                for (idx, (_, engine)) in parked_states.iter_mut().enumerate() {
                    if engine.find_terminal_by_id(sid).is_some() {
                        let outcome = core.process_pty_output(engine, sid);
                        if !outcome.events.is_empty() {
                            pending.push((DispatchSource::Parked(idx), outcome.events));
                        }
                        break;
                    }
                }
            }
        } else {
            for (wid, w) in views.iter_mut() {
                if let Some(main) = w.as_main_mut() {
                    let outcome = core.process_all_pty_output(&mut main.core_state);
                    if !outcome.events.is_empty() {
                        pending.push((DispatchSource::Main(*wid), outcome.events));
                    }
                }
                w.mark_dirty_from(RepaintSource::TerminalOutput);
            }
            for (idx, (_, engine)) in parked_states.iter_mut().enumerate() {
                let outcome = core.process_all_pty_output(engine);
                if !outcome.events.is_empty() {
                    pending.push((DispatchSource::Parked(idx), outcome.events));
                }
            }
        }
        for (source, events) in pending {
            for ev in events {
                self.handle_core_event_system(source, ev);
            }
        }
    }

    /// macOS는 MainView 상태를 park하고 창을 없앤다. Windows·Linux는 창을 숨기거나 최소화한다.
    fn handle_minimize(&mut self) {
        #[cfg(target_os = "macos")]
        {
            let drained: Vec<_> = self.view.views.drain().map(|(_, w)| w).collect();
            for w in drained {
                if let Some(mut main_box) = crate::view::unbox_main(w) {
                    // 모달은 함께 park하지 않으므로 상태의 모달 ID·종류도 비운다.
                    main_box.state.active_modal_id = None;
                    main_box.state.active_modal_kind = None;
                    self.parked_states
                        .push((main_box.state, main_box.core_state));
                }
            }
            self.view.focused_view_id = None;
            self.view.active_modal_id = None;
            tracing::info!(
                "minimized to background ({} states parked)",
                self.parked_states.len()
            );
        }
        #[cfg(windows)]
        {
            if self.tray_icon.is_some() {
                for w in self.view.views.values() {
                    w.base().winit.set_visible(false);
                }
                tracing::info!("hid {} window(s) to system tray", self.view.views.len());
            } else {
                for w in self.view.views.values() {
                    w.base().winit.set_minimized(true);
                }
                tracing::info!("minimized {} window(s) to taskbar", self.view.views.len());
            }
        }
        #[cfg(target_os = "linux")]
        {
            if self.tray_icon.is_some() {
                for w in self.view.views.values() {
                    w.base().winit.set_visible(false);
                }
                tracing::info!("hid {} window(s) to system tray", self.view.views.len());
            } else {
                for w in self.view.views.values() {
                    w.base().winit.set_minimized(true);
                }
                tracing::info!("minimized {} window(s) to taskbar", self.view.views.len());
            }
        }
    }

    /// 확인한 셸 경로를 설정에 반영하고 이미 보이는 setup 창으로 부팅을 계속한다.
    fn finish_shell_setup_confirmed(&mut self) {
        let mut settings = crate::settings::Settings::load();
        let normalize_report = settings.normalize();
        settings.general.shell = self.shell_setup_path.clone();
        if let Err(e) = settings.save() {
            tracing::error!("failed to save settings: {e}");
        }
        self.shell_setup_mode = false;
        let window = self.shell_setup_window.take().unwrap();
        let gpu = self.shell_setup_gpu.take().unwrap();
        self.begin_boot(window, gpu, settings, std::time::Instant::now(), false);
        drop(normalize_report);
    }

    fn handle_shell_setup_window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        event: WindowEvent,
    ) {
        if let WindowEvent::RedrawRequested = &event {
            if let (Some(gpu), Some(window)) = (&mut self.shell_setup_gpu, &self.shell_setup_window)
            {
                let result = gpu.render_shell_setup(window, &mut self.shell_setup_path);
                match result {
                    Ok(crate::gpu::ShellSetupAction::None) => {}
                    Ok(crate::gpu::ShellSetupAction::Confirmed) => {
                        self.finish_shell_setup_confirmed();
                    }
                    Ok(crate::gpu::ShellSetupAction::Exit) => {
                        event_loop.exit();
                    }
                    Err(e) => {
                        let msg = format!("shell setup render error: {e}");
                        tracing::warn!("{}", msg);
                        crate::crash_report::record_error(&msg);
                    }
                }
            }
            if let (Some(gpu), Some(window)) = (&mut self.shell_setup_gpu, &self.shell_setup_window)
            {
                gpu.handle_egui_event(window, &event);
            }
            return;
        }
        if let (Some(gpu), Some(window)) = (&mut self.shell_setup_gpu, &self.shell_setup_window) {
            gpu.handle_egui_event(window, &event);
            if let WindowEvent::CloseRequested = &event {
                event_loop.exit();
            }
        }
    }

    /// 준비된 창과 GPU에 엔진 오류를 표시해 터미널 없이 실행한 사용자도 원인을 볼 수 있게 한다.
    pub(crate) fn enter_boot_error_mode(
        &mut self,
        window: std::sync::Arc<winit::window::Window>,
        mut gpu: crate::gpu::GpuState,
    ) {
        tracing::warn!("boot failed with a live GPU — showing the boot error screen");
        self.boot_error_mode = true;
        // 렌더 실패 시에도 창은 표시한다.
        if let Some(info) = &self.boot_error_info
            && let Err(e) = gpu.render_boot_error(&window, info)
        {
            tracing::warn!("boot error first frame render failed: {e} — showing window anyway");
        }
        window.set_visible(true);
        self.boot_error_gpu = Some(gpu);
        self.boot_error_window = Some(window);
    }

    /// 부팅 오류 화면에서 종료를 요청하면 실패 코드 1로 끝낸다.
    fn handle_boot_error_window_event(&mut self, event: WindowEvent) {
        if let WindowEvent::RedrawRequested = &event {
            let quit = if let (Some(gpu), Some(window), Some(info)) = (
                &mut self.boot_error_gpu,
                &self.boot_error_window,
                &self.boot_error_info,
            ) {
                match gpu.render_boot_error(window, info) {
                    Ok(quit) => quit,
                    Err(e) => {
                        let msg = format!("boot error render error: {e}");
                        tracing::warn!("{}", msg);
                        crate::crash_report::record_error(&msg);
                        false
                    }
                }
            } else {
                false
            };
            if quit {
                std::process::exit(1);
            }
            // RedrawRequested에서 다시 요청하면 유휴 상태에도 렌더가 계속 반복된다.
            if let (Some(gpu), Some(window)) = (&mut self.boot_error_gpu, &self.boot_error_window) {
                gpu.handle_egui_event(window, &event);
            }
            return;
        }
        if let (Some(gpu), Some(window)) = (&mut self.boot_error_gpu, &self.boot_error_window) {
            gpu.handle_egui_event(window, &event);
            window.request_redraw();
            if let WindowEvent::CloseRequested = &event {
                std::process::exit(1);
            }
        }
    }

    /// 모달에 이벤트를 전달하고 반환한 닫기 요청을 처리한다. 호출자는 이후 일반 창 처리를 하지 않는다.
    fn handle_active_modal_window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        id: WindowId,
        event: WindowEvent,
    ) {
        let action = if let Some(modal) = self.view.views.get_mut(&id) {
            let mut ctx = ViewCtx {
                event_loop,
                modal_active: false,
                plugin_manager: self.plugin_manager.as_ref(),
                stream_hub: &self.stream_hub,
            };
            modal.handle_event(event, &mut ctx)
        } else {
            ViewAction::None
        };

        match action {
            ViewAction::None => {}
            ViewAction::Close => {
                self.close_active_modal();
            }
            ViewAction::CloseWithEvent(app_event) => {
                self.close_active_modal();
                crate::shortcuts::send_app_event(&self.view.proxy, app_event);
            }
        }
    }

    fn handle_window_focused(&mut self, id: WindowId) {
        // focused_view_id는 모달이 아닌 MainView를 가리킨다.
        let is_main = self
            .view
            .views
            .get(&id)
            .map(|w| w.as_main().is_some())
            .unwrap_or(false);
        if is_main {
            self.view.focused_view_id = Some(id);
        }
        if let Some(mgr) = self.plugin_manager.as_mut() {
            use tasty_plugin_protocol::EventScope;
            use tasty_plugin_protocol::events::payloads::WindowFocused;
            let payload = WindowFocused {
                window_id: u64::from(id),
            };
            mgr.emit_host_event("window.focused", &payload, EventScope::System);
        }
        if let Some(modal_id) = self.view.active_modal_id
            && let Some(modal) = self.view.views.get(&modal_id)
        {
            modal.base().winit.focus_window();
        }
    }

    fn dispatch_window_event_to_view(
        &mut self,
        event_loop: &ActiveEventLoop,
        id: WindowId,
        event: WindowEvent,
    ) {
        let modal_active = self.view.is_modal_active();
        let action = {
            if let Some(w) = self.view.views.get_mut(&id) {
                let mut ctx = ViewCtx {
                    event_loop,
                    modal_active,
                    plugin_manager: self.plugin_manager.as_ref(),
                    stream_hub: &self.stream_hub,
                };
                // modeless PresetView의 닫기는 여기서 처리하며 모달은 앞의 전용 경로가 처리한다.
                w.handle_event(event, &mut ctx)
            } else {
                ViewAction::None
            }
        };
        if self.view.views.contains_key(&id) {
            match action {
                ViewAction::None => {}
                ViewAction::Close => {
                    if self.preset_view_id == Some(id) {
                        self.on_preset_window_closed(id);
                        return;
                    }
                    debug_assert!(false, "non-modal window returned Close unexpectedly");
                }
                ViewAction::CloseWithEvent(app_event) => {
                    if self.preset_view_id == Some(id) {
                        self.on_preset_window_closed(id);
                        crate::shortcuts::send_app_event(&self.view.proxy, app_event);
                        return;
                    }
                    debug_assert!(
                        false,
                        "non-modal window returned CloseWithEvent unexpectedly"
                    );
                }
            }

            let close_requested = self
                .view
                .views
                .get(&id)
                .map(|w| w.base().close_requested)
                .unwrap_or(false);
            if close_requested {
                self.close_self_requesting_window(id);
            }
        }
    }

    /// 창 이벤트 밖에서도 마지막 workspace를 닫을 수 있어 그 자리에서 닫기 요청을 처리한다.
    /// 빈 workspace 목록을 가진 창이 다음 redraw로 넘어가지 않게 한다.
    /// close_request_consumed_in_place 검사가 호출 위치를 확인한다.
    pub(crate) fn close_self_requesting_windows(&mut self) {
        let ids: Vec<WindowId> = self
            .view
            .views
            .iter()
            .filter(|(_, w)| w.base().close_requested)
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            self.close_self_requesting_window(id);
        }
    }

    /// 내부 close_requested를 처리한다. 버튼 닫기와 달리 plugin·Lua 창 닫기 통지는 보내지 않는다.
    fn close_self_requesting_window(&mut self, id: WindowId) {
        self.pending_focus_hint_clear.remove(&id);
        // 조건식 안에서 drop되지 않게 먼저 꺼내 은퇴 처리를 수행한다.
        if let Some(w) = self.view.views.remove(&id) {
            let was_last_main = self.view.views.values().all(|w| w.as_main().is_none());
            match crate::view::unbox_main(w) {
                // 마지막 MainView는 engine을 park하므로 레이아웃 슬롯 점유도 유지한다.
                Some(main_box) if was_last_main => {
                    tracing::info!("last main window closed via request, parking state");
                    self.parked_states
                        .push((main_box.state, main_box.core_state));
                }
                Some(mut main_box) => {
                    let active_workspace = main_box.state.active_workspace;
                    Self::retire_main_engine(
                        &mut self.core,
                        &mut main_box.core_state,
                        active_workspace,
                    );
                }
                None => {}
            }
        }
        if self.view.focused_view_id == Some(id) {
            self.view.focused_view_id = self
                .view
                .views
                .iter()
                .find(|(_, w)| w.as_main().is_some())
                .map(|(id, _)| *id);
        }
    }

    /// Windows 절전 복귀 뒤 PTY 상태·출력을 확인하고 의심되는 surface를 알린다.
    #[cfg(all(windows, feature = "gui"))]
    pub(crate) fn resume_health_pass(&mut self) {
        use crate::app::dispatch_domain::DispatchSource;
        use crate::core::intent::CoreEvent;
        tracing::info!("system resumed — running PTY health pass (ADR-0013)");
        let core = &mut self.core;
        let mut pending: Vec<(DispatchSource, Vec<CoreEvent>)> = Vec::new();
        for (wid, w) in self.view.views.iter_mut() {
            let Some(main) = w.as_main_mut() else {
                continue;
            };
            let suspects = main.core_state.wake_terminals_after_resume();
            let outcome = core.process_all_pty_output(&mut main.core_state);
            if !outcome.events.is_empty() {
                pending.push((DispatchSource::Main(*wid), outcome.events));
            }
            Self::notify_resume_suspects(&mut main.core_state, &suspects);
            w.mark_dirty();
        }
        for (idx, (_, engine)) in self.parked_states.iter_mut().enumerate() {
            let suspects = engine.wake_terminals_after_resume();
            let outcome = core.process_all_pty_output(engine);
            if !outcome.events.is_empty() {
                pending.push((DispatchSource::Parked(idx), outcome.events));
            }
            Self::notify_resume_suspects(engine, &suspects);
        }
        for (source, events) in pending {
            for ev in events {
                self.handle_core_event_system(source, ev);
            }
        }
    }

    #[cfg(all(windows, feature = "gui"))]
    fn notify_resume_suspects(engine: &mut crate::core::CoreState, suspects: &[u32]) {
        for &sid in suspects {
            let ws_id = engine
                .workspaces
                .iter()
                .find(|w| w.all_surface_ids().contains(&sid))
                .map(|w| w.id)
                .unwrap_or(0);
            let title = crate::i18n::t("resume.suspect.title").to_string();
            let body = crate::i18n::t("resume.suspect.body").to_string();
            if engine.notifications.add(ws_id, sid, title, body).is_some() {
                engine.raise_attention(sid, crate::core::AttentionKind::Completion);
            }
        }
    }

    /// PresetView는 바로 닫고 MainView는 남은 개수에 따라 해당 창 닫기 또는 종료 흐름으로 보낸다.
    pub(crate) fn request_close_window(&mut self, id: WindowId, event_loop: &ActiveEventLoop) {
        if self.preset_view_id == Some(id) {
            self.on_preset_window_closed(id);
            return;
        }
        if self.main_window_count() > 1 {
            self.close_main_window(id, tasty_plugin_protocol::LifecycleReason::User);
        } else {
            self.handle_quit_requested(event_loop);
        }
    }

    /// 창을 닫고 plugin·Lua에 알린다. 포커스 창이면 포커스를 넘긴다.
    /// 마지막 MainView의 종료 여부는 호출자가 결정한다.
    pub(crate) fn close_main_window(
        &mut self,
        id: WindowId,
        reason: tasty_plugin_protocol::LifecycleReason,
    ) {
        // engine이 drop되기 전에 슬롯을 마무리하고 닫기 통지를 보낸다.
        self.pending_focus_hint_clear.remove(&id);
        let removed = self.view.views.remove(&id);
        if let Some(mut main_box) = removed.and_then(crate::view::unbox_main) {
            let active_workspace = main_box.state.active_workspace;
            Self::retire_main_engine(&mut self.core, &mut main_box.core_state, active_workspace);
        }
        let scripts = self.autofire_scripts();
        if let Some(mgr) = self.plugin_manager.as_mut() {
            use tasty_plugin_protocol::EventScope;
            use tasty_plugin_protocol::events::payloads::WindowClosed;
            let payload = WindowClosed {
                window_id: u64::from(id),
                reason,
            };
            mgr.emit_host_event("window.closed", &payload, EventScope::System);
            crate::hooks::lua::fire(
                self.lua_engine.as_ref(),
                crate::hooks::lua::AutofireCtx {
                    scripts: &scripts,
                    guard: &mut self.lua_autofire,
                },
                "window.delete.post",
                &payload,
            );
        }
        if self.view.focused_view_id == Some(id) {
            self.view.focused_view_id = self
                .view
                .views
                .iter()
                .find(|(_, w)| w.as_main().is_some())
                .map(|(id, _)| *id);
        }
    }

    /// 배치 시작에 모든 engine에 끊긴 client를 표시한다. 실제 점유 해제는 배치 끝에 수행한다.
    /// 표시·해제를 나누는 이유는 OccupancyRegistry::mark_clients_disconnected를 따른다.
    pub(crate) fn mark_disconnected_clients(&mut self, clients: &[u32]) {
        if clients.is_empty() {
            return;
        }
        for w in self.view.views.values_mut() {
            if let Some(main) = w.as_main_mut() {
                main.core_state.attach.mark_clients_disconnected(clients);
            }
        }
        for (_, engine) in self.parked_states.iter_mut() {
            engine.attach.mark_clients_disconnected(clients);
        }
    }

    pub(crate) fn release_attach_for_disconnected(&mut self, clients: &[u32]) {
        for w in self.view.views.values_mut() {
            if let Some(main) = w.as_main_mut() {
                for &cid in clients {
                    main.core_state.attach.release_all_for_client(cid);
                    main.core_state.bulk_transfers.clear_client(cid);
                    main.core_state.capture_uploads.clear_client(cid);
                    main.core_state.mesh_mirror.remove_for_client(cid);
                }
            }
        }
        for (_, engine) in self.parked_states.iter_mut() {
            for &cid in clients {
                engine.attach.release_all_for_client(cid);
                engine.bulk_transfers.clear_client(cid);
                engine.capture_uploads.clear_client(cid);
                engine.mesh_mirror.remove_for_client(cid);
            }
        }
    }

    pub(crate) fn apply_stream_outcome(&mut self, outcome: tasty_ipc::stream_hub::PumpOutcome) {
        let hub = self.stream_hub.clone();

        // 같은 배치의 재attach가 곧 해제될 holder 때문에 거절되지 않게 먼저 끊김을 표시한다.
        self.mark_disconnected_clients(&outcome.disconnected);
        self.apply_attach_requests_batch(outcome.attach_requests, &hub);
        self.apply_workspace_attach_requests_batch(outcome.workspace_attach_requests, &hub);
        self.apply_input_frames_batch(outcome.input_frames);
        self.apply_structural_ops_batch(outcome.structural_ops, &hub);
        self.apply_resize_requests_batch(outcome.resize_requests);
        self.apply_attention_clear_requests_batch(outcome.attention_clear_requests);
        self.apply_mesh_context_requests_batch(outcome.mesh_context_requests, &hub);
        self.apply_mesh_full_resend_requests_batch(outcome.mesh_full_resend_requests, &hub);
        self.apply_mesh_input_events_batch(outcome.mesh_input_events, &hub);

        self.apply_capture_uploads_batch(outcome.capture_uploads, &hub);
        self.apply_list_dir_requests_batch(outcome.list_dir_requests, &hub);
        self.apply_git_query_requests_batch(outcome.git_query_requests, &hub);
        self.apply_markdown_content_requests_batch(outcome.markdown_content_requests, &hub);
        // begin보다 chunk가 먼저 처리되지 않도록 bulk 이벤트는 도착 순서를 유지한다.
        self.apply_bulk_events_batch(outcome.bulk_events, &hub);

        if !outcome.disconnected.is_empty() {
            self.release_attach_for_disconnected(&outcome.disconnected);
        }

        // 점유 변경 직후 readonly 화면을 갱신해 다음 주기 확인까지 빈 화면으로 남지 않게 한다.
        for w in self.view.views.values_mut() {
            if let Some(main) = w.as_main_mut() {
                main.core_state.push_structure_changes();
                if main.core_state.refresh_readonly_views() {
                    w.mark_dirty();
                }
            }
        }
        for (_, engine) in self.parked_states.iter_mut() {
            engine.push_structure_changes();
        }
    }

    fn apply_attach_requests_batch(
        &mut self,
        requests: impl IntoIterator<Item = (u32, u32)>,
        hub: &tasty_ipc::stream_hub::StreamHub,
    ) {
        for (client_id, surface_id) in requests {
            if !self.attach_on_owning_engine(surface_id, client_id, hub) {
                crate::core::attach_runtime::reject_attach(hub, client_id, "not_found", None);
            }
        }
    }

    fn apply_workspace_attach_requests_batch(
        &mut self,
        requests: impl IntoIterator<Item = (u32, u32)>,
        hub: &tasty_ipc::stream_hub::StreamHub,
    ) {
        for (client_id, workspace_id) in requests {
            if !self.attach_workspace_on_owning_engine(workspace_id, client_id, hub) {
                crate::core::attach_runtime::reject_attach(
                    hub,
                    client_id,
                    "workspace_not_found",
                    None,
                );
            }
        }
    }

    fn apply_input_frames_batch(&mut self, frames: impl IntoIterator<Item = (u32, Vec<u8>)>) {
        for (client_id, bytes) in frames {
            let routed = self.feed_stream_input(client_id, &bytes);
            #[cfg(debug_assertions)]
            if !routed {
                let echo_frame = crate::ipc::stream::StreamFrame::new(
                    crate::ipc::stream::StreamTag::Data,
                    bytes,
                );
                let _ = self.stream_hub.push(client_id, echo_frame); // debug echo의 전송 실패는 재시도하지 않는다.
            }
            #[cfg(not(debug_assertions))]
            let _ = routed; // release: echo 분기 없어 routed 미사용 — 값 drop(Result 아님).
        }
    }

    fn apply_structural_ops_batch(
        &mut self,
        ops: impl IntoIterator<
            Item = (
                u32,
                u64,
                crate::ipc::stream::StructuralOp,
                crate::ipc::stream::ForwardOrigin,
            ),
        >,
        hub: &tasty_ipc::stream_hub::StreamHub,
    ) {
        for (client_id, op_id, op, origin) in ops {
            self.apply_forwarded_structural_op(client_id, op_id, &op, origin, hub);
        }
    }

    fn apply_resize_requests_batch(
        &mut self,
        requests: impl IntoIterator<Item = (u32, u32, usize, usize)>,
    ) {
        for (client_id, remote_surface_id, cols, rows) in requests {
            self.apply_forwarded_resize(client_id, remote_surface_id, cols, rows);
        }
    }

    fn apply_attention_clear_requests_batch(
        &mut self,
        requests: impl IntoIterator<Item = (u32, u32)>,
    ) {
        for (client_id, remote_surface_id) in requests {
            self.apply_forwarded_attention_clear(client_id, remote_surface_id);
        }
    }

    /// 점유를 확인해 attention을 해제한다. 저장 대상이 아니므로 레이아웃 저장은 예약하지 않는다.
    fn apply_forwarded_attention_clear(&mut self, client_id: u32, remote_surface_id: u32) {
        for w in self.view.views.values_mut() {
            if let Some(main) = w.as_main_mut()
                && main
                    .core_state
                    .apply_attached_attention_clear(client_id, remote_surface_id)
            {
                w.mark_dirty();
                return;
            }
        }
        for (_, engine) in self.parked_states.iter_mut() {
            if engine.apply_attached_attention_clear(client_id, remote_surface_id) {
                return;
            }
        }
    }

    /// 소유 engine에 mesh 구독·표시 정보를 반영한다. 대상이나 점유가 없으면 MeshError를 회신한다.
    fn apply_mesh_context_requests_batch(
        &mut self,
        requests: impl IntoIterator<
            Item = (
                u32,
                u32,
                u32,
                u32,
                f32,
                Option<tasty_plugin_protocol::protocol::ThemeWire>,
                bool,
            ),
        >,
        hub: &tasty_ipc::stream_hub::StreamHub,
    ) {
        for (client_id, surface_id, width_px, height_px, pixels_per_point, theme, focused) in
            requests
        {
            let ok = self.apply_mesh_context_on_owning_engine(
                surface_id,
                client_id,
                width_px,
                height_px,
                pixels_per_point,
                theme,
                focused,
            );
            if !ok {
                reply_mesh_error(hub, client_id, surface_id, "not_attached");
            }
        }
    }

    fn apply_mesh_full_resend_requests_batch(
        &mut self,
        requests: impl IntoIterator<Item = (u32, u32)>,
        hub: &tasty_ipc::stream_hub::StreamHub,
    ) {
        for (client_id, surface_id) in requests {
            let ok = self.apply_mesh_full_resend_on_owning_engine(surface_id, client_id);
            if !ok {
                reply_mesh_error(hub, client_id, surface_id, "not_attached");
            }
        }
    }

    /// 입력을 소유 engine에 쌓는다. 헤드리스·GUI parked는 공용 구동 경로가 소비한다.
    /// 창이 살아 있는 GUI 서버는 이 입력 큐를 플러그인에 전달하는 경로가 아직 없다.
    fn apply_mesh_input_events_batch(
        &mut self,
        events: impl IntoIterator<Item = (u32, u32, tasty_plugin_protocol::protocol::RawInputWire)>,
        hub: &tasty_ipc::stream_hub::StreamHub,
    ) {
        for (client_id, surface_id, input) in events {
            let ok = self.apply_mesh_input_on_owning_engine(surface_id, client_id, input);
            if !ok {
                reply_mesh_error(hub, client_id, surface_id, "not_attached");
            }
        }
    }

    fn apply_capture_uploads_batch(
        &mut self,
        uploads: impl IntoIterator<Item = (u32, tasty_ipc::stream_hub::CaptureUploadMsg)>,
        hub: &tasty_ipc::stream_hub::StreamHub,
    ) {
        for (client_id, msg) in uploads {
            self.apply_capture_upload_msg(client_id, msg, hub);
        }
    }

    fn apply_list_dir_requests_batch(
        &mut self,
        requests: impl IntoIterator<Item = (u32, tasty_ipc::stream_hub::ListDirRequestMsg)>,
        hub: &tasty_ipc::stream_hub::StreamHub,
    ) {
        for (client_id, msg) in requests {
            self.apply_list_dir_request_msg(client_id, msg, hub);
        }
    }

    fn apply_git_query_requests_batch(
        &mut self,
        requests: impl IntoIterator<Item = (u32, tasty_ipc::stream_hub::GitQueryRequestMsg)>,
        hub: &tasty_ipc::stream_hub::StreamHub,
    ) {
        for (client_id, msg) in requests {
            self.apply_git_query_request_msg(client_id, msg, hub);
        }
    }

    fn apply_markdown_content_requests_batch(
        &mut self,
        requests: impl IntoIterator<Item = (u32, tasty_ipc::stream_hub::MarkdownContentRequestMsg)>,
        hub: &tasty_ipc::stream_hub::StreamHub,
    ) {
        for (client_id, msg) in requests {
            self.apply_markdown_content_request_msg(client_id, msg, hub);
        }
    }

    fn apply_bulk_events_batch(
        &mut self,
        events: impl IntoIterator<Item = (u32, tasty_ipc::stream_hub::BulkEvent)>,
        hub: &tasty_ipc::stream_hub::StreamHub,
    ) {
        for (client_id, event) in events {
            match hub.bulk_workspace(client_id) {
                Some(ws) => self.apply_bulk_event(client_id, event, ws, hub),
                None => tracing::warn!(
                    "bulk transfer: event from non-bulk client {client_id} — ignoring"
                ),
            }
        }
    }

    fn attach_on_owning_engine(
        &mut self,
        surface_id: u32,
        client_id: u32,
        hub: &tasty_ipc::stream_hub::StreamHub,
    ) -> bool {
        for w in self.view.views.values_mut() {
            if let Some(main) = w.as_main_mut() {
                let e = &mut main.core_state;
                if e.terminals.contains(surface_id) || e.is_surface_deferred(surface_id) {
                    e.attach_surface_for_stream(surface_id, client_id, hub);
                    return true;
                }
            }
        }
        for (_, engine) in self.parked_states.iter_mut() {
            if engine.terminals.contains(surface_id) || engine.is_surface_deferred(surface_id) {
                engine.attach_surface_for_stream(surface_id, client_id, hub);
                return true;
            }
        }
        false
    }

    fn attach_workspace_on_owning_engine(
        &mut self,
        workspace_id: u32,
        client_id: u32,
        hub: &tasty_ipc::stream_hub::StreamHub,
    ) -> bool {
        for w in self.view.views.values_mut() {
            if let Some(main) = w.as_main_mut() {
                let e = &mut main.core_state;
                if e.find_workspace_index_for_id(workspace_id).is_some() {
                    e.attach_workspace_for_stream(workspace_id, client_id, hub);
                    return true;
                }
            }
        }
        for (_, engine) in self.parked_states.iter_mut() {
            if engine.find_workspace_index_for_id(workspace_id).is_some() {
                engine.attach_workspace_for_stream(workspace_id, client_id, hub);
                return true;
            }
        }
        false
    }

    /// workspace 점유는 surface ID가 붙은 입력, 단일 surface 점유는 원시 입력으로 전달한다.
    fn feed_stream_input(&mut self, client_id: u32, bytes: &[u8]) -> bool {
        for w in self.view.views.values_mut() {
            if let Some(main) = w.as_main_mut()
                && main.core_state.attach.client_holds_workspace(client_id)
            {
                return Self::demux_workspace_input(&mut main.core_state, client_id, bytes);
            }
        }
        for (_, engine) in self.parked_states.iter_mut() {
            if engine.attach.client_holds_workspace(client_id) {
                return Self::demux_workspace_input(engine, client_id, bytes);
            }
        }
        self.feed_input_on_owning_engine(client_id, bytes)
    }

    fn demux_workspace_input(
        engine: &mut crate::core::CoreState,
        client_id: u32,
        bytes: &[u8],
    ) -> bool {
        match crate::ipc::stream::decode_mux(bytes) {
            Some((sid, payload)) => engine.feed_attached_workspace_input(client_id, sid, payload),
            None => false,
        }
    }

    fn feed_input_on_owning_engine(&mut self, client_id: u32, bytes: &[u8]) -> bool {
        for w in self.view.views.values_mut() {
            if let Some(main) = w.as_main_mut()
                && main.core_state.feed_attached_input(client_id, bytes)
            {
                return true;
            }
        }
        for (_, engine) in self.parked_states.iter_mut() {
            if engine.feed_attached_input(client_id, bytes) {
                return true;
            }
        }
        false
    }

    /// 창·parked engine에서 대상과 점유를 확인해 mesh 구독을 갱신한다.
    #[allow(clippy::too_many_arguments)]
    fn apply_mesh_context_on_owning_engine(
        &mut self,
        surface_id: u32,
        client_id: u32,
        width_px: u32,
        height_px: u32,
        pixels_per_point: f32,
        theme: Option<tasty_plugin_protocol::protocol::ThemeWire>,
        focused: bool,
    ) -> bool {
        for w in self.view.views.values_mut() {
            if let Some(main) = w.as_main_mut()
                && main.core_state.apply_attached_mesh_context(
                    surface_id,
                    client_id,
                    width_px,
                    height_px,
                    pixels_per_point,
                    theme.clone(),
                    focused,
                )
            {
                return true;
            }
        }
        for (_, engine) in self.parked_states.iter_mut() {
            if engine.apply_attached_mesh_context(
                surface_id,
                client_id,
                width_px,
                height_px,
                pixels_per_point,
                theme.clone(),
                focused,
            ) {
                return true;
            }
        }
        false
    }

    fn apply_mesh_input_on_owning_engine(
        &mut self,
        surface_id: u32,
        client_id: u32,
        input: tasty_plugin_protocol::protocol::RawInputWire,
    ) -> bool {
        for w in self.view.views.values_mut() {
            if let Some(main) = w.as_main_mut()
                && main
                    .core_state
                    .apply_attached_mesh_input(surface_id, client_id, input.clone())
            {
                return true;
            }
        }
        for (_, engine) in self.parked_states.iter_mut() {
            if engine.apply_attached_mesh_input(surface_id, client_id, input.clone()) {
                return true;
            }
        }
        false
    }

    fn apply_mesh_full_resend_on_owning_engine(&mut self, surface_id: u32, client_id: u32) -> bool {
        for w in self.view.views.values_mut() {
            if let Some(main) = w.as_main_mut()
                && main
                    .core_state
                    .apply_attached_mesh_full_resend(surface_id, client_id)
            {
                return true;
            }
        }
        for (_, engine) in self.parked_states.iter_mut() {
            if engine.apply_attached_mesh_full_resend(surface_id, client_id) {
                return true;
            }
        }
        false
    }

    /// 구조 변경은 점유한 workspace를 가진 MainView에서만 실행한다.
    /// parked 상태에도 AppState는 있지만 현재 이 실행 루프의 대상은 아니다.
    fn apply_forwarded_structural_op(
        &mut self,
        client_id: u32,
        op_id: u64,
        op: &crate::ipc::stream::StructuralOp,
        origin: crate::ipc::stream::ForwardOrigin,
        hub: &tasty_ipc::stream_hub::StreamHub,
    ) {
        let anchor = op.anchor_surface_id();
        let core = &mut self.core;
        let mut handled = false;
        for w in self.view.views.values_mut() {
            let Some(main) = w.as_main_mut() else {
                continue;
            };
            let engine = &mut main.core_state;
            let Some(ws) = engine.attach.workspace_of_surface(anchor) else {
                continue;
            };
            handled = true;
            let (ok, reason, delta) = if engine.attach.workspace_holder(ws) != Some(client_id) {
                (false, Some("not workspace holder".to_string()), None)
            } else {
                match crate::core::attach_runtime::execute_forwarded_structural_op(
                    core,
                    &mut main.state,
                    engine,
                    op,
                    origin,
                ) {
                    Ok(delta) => (true, None, delta),
                    Err(reason) => (false, Some(reason), None),
                }
            };
            // 회신·구조 delta·새 터미널 출력 순서로 보내 mirror가 ID 매핑을 만든 뒤 출력을 받게 한다.
            reply_structural_result(hub, client_id, op_id, ok, reason);
            if let Some(fd) = delta {
                push_structural_delta(hub, client_id, &fd.delta);
                for sid in fd.added_terminals {
                    engine.tap_surface_for_stream(sid, client_id, hub);
                }
                // kind가 바뀌면 옛 mesh를 버려 새 surface 초기화를 유도한다.
                if let Some(sid) = fd.converted_surface
                    && let Some(mgr) = self.plugin_manager.as_mut()
                {
                    mgr.drop_egui_mesh_frame(sid);
                }
            }
            w.mark_dirty();
            break;
        }
        if !handled {
            // workspace 점유는 있지만 anchor가 사라진 경우도 구별해 거절한다.
            let engines = self
                .view
                .views
                .values()
                .filter_map(|w| w.as_main())
                .map(|m| &m.core_state)
                .chain(self.parked_states.iter().map(|(_, e)| e));
            let reason = crate::core::attach_structure_sync::unresolved_forward_reason(
                engines, client_id, op,
            );
            reply_structural_result(hub, client_id, op_id, false, Some(reason));
        }
    }

    /// 점유를 확인해 PTY 크기를 바꾼다. 변화가 있으면 tap이 Resize를 전송한다.
    /// 대상·점유 불일치는 별도 오류 회신이 없어 echo 부재만으로 원인을 알 수 없다.
    fn apply_forwarded_resize(
        &mut self,
        client_id: u32,
        remote_surface_id: u32,
        cols: usize,
        rows: usize,
    ) {
        for w in self.view.views.values_mut() {
            if let Some(main) = w.as_main_mut()
                && main.core_state.apply_attached_workspace_resize(
                    client_id,
                    remote_surface_id,
                    cols,
                    rows,
                )
            {
                w.mark_dirty();
                return;
            }
        }
        for (_, engine) in self.parked_states.iter_mut() {
            if engine.apply_attached_workspace_resize(client_id, remote_surface_id, cols, rows) {
                return;
            }
        }
    }

    /// workspace 점유자를 찾아 캡처 청크를 누적하거나 파일 저장·클립보드 갱신을 완료한다.
    fn apply_capture_upload_msg(
        &mut self,
        client_id: u32,
        msg: tasty_ipc::stream_hub::CaptureUploadMsg,
        hub: &tasty_ipc::stream_hub::StreamHub,
    ) {
        use tasty_ipc::stream_hub::CaptureUploadMsg;
        match msg {
            CaptureUploadMsg::CaptureChunk {
                upload_id,
                data_b64,
                ..
            } => {
                use base64::Engine as _;
                let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(&data_b64) else {
                    tracing::warn!(
                        "capture upload: invalid base64 chunk (client {client_id}, upload {upload_id})"
                    );
                    return;
                };
                match find_workspace_holder_engine_mut(
                    &mut self.view.views,
                    &mut self.parked_states,
                    client_id,
                ) {
                    Some(engine) => engine.capture_uploads.append(
                        client_id,
                        upload_id,
                        &bytes,
                        std::time::Instant::now(),
                    ),
                    None => tracing::warn!(
                        "capture upload: client {client_id} does not hold a workspace — dropping chunk"
                    ),
                }
            }
            CaptureUploadMsg::CaptureCommit {
                upload_id,
                file_name,
            } => {
                let core = &self.core;
                match find_workspace_holder_engine_mut(
                    &mut self.view.views,
                    &mut self.parked_states,
                    client_id,
                ) {
                    Some(engine) => {
                        crate::core::attach_runtime::finalize_capture_upload(
                            engine, core, hub, client_id, upload_id, &file_name,
                        );
                    }
                    None => {
                        let payload = serde_json::json!({
                            "event": "capture_result",
                            "upload_id": upload_id,
                            "ok": false,
                            "reason": "client does not hold a workspace attach",
                        });
                        let frame = crate::ipc::stream::StreamFrame::new(
                            crate::ipc::stream::StreamTag::Control,
                            serde_json::to_vec(&payload).unwrap_or_default(),
                        );
                        let _ = hub.push(client_id, frame); // 회신 전송 실패는 여기서 재시도하지 않는다.
                    }
                }
            }
        }
    }

    fn apply_list_dir_request_msg(
        &mut self,
        client_id: u32,
        msg: tasty_ipc::stream_hub::ListDirRequestMsg,
        hub: &tasty_ipc::stream_hub::StreamHub,
    ) {
        use tasty_ipc::stream_hub::ListDirRequestMsg;
        let ListDirRequestMsg::ListDirRequest { request_id, dir } = msg;
        for w in self.view.views.values_mut() {
            if let Some(main) = w.as_main_mut()
                && main.core_state.attach.client_holds_workspace(client_id)
            {
                crate::core::attach_runtime::handle_list_dir_request(
                    &mut main.core_state,
                    hub,
                    client_id,
                    request_id,
                    &dir,
                );
                return;
            }
        }
        for (_, engine) in self.parked_states.iter_mut() {
            if engine.attach.client_holds_workspace(client_id) {
                crate::core::attach_runtime::handle_list_dir_request(
                    engine, hub, client_id, request_id, &dir,
                );
                return;
            }
        }
        let payload = serde_json::json!({
            "event": "list_dir_result",
            "request_id": request_id,
            "ok": false,
            "reason": "client does not hold a workspace attach",
        });
        let frame = crate::ipc::stream::StreamFrame::new(
            crate::ipc::stream::StreamTag::Control,
            serde_json::to_vec(&payload).unwrap_or_default(),
        );
        let _ = hub.push(client_id, frame); // 회신 전송 실패는 여기서 재시도하지 않는다.
    }

    fn apply_markdown_content_request_msg(
        &mut self,
        client_id: u32,
        msg: tasty_ipc::stream_hub::MarkdownContentRequestMsg,
        hub: &tasty_ipc::stream_hub::StreamHub,
    ) {
        use tasty_ipc::stream_hub::MarkdownContentRequestMsg;
        let MarkdownContentRequestMsg::MarkdownContentRequest {
            request_id,
            surface_id,
        } = msg;
        for w in self.view.views.values_mut() {
            if let Some(main) = w.as_main_mut()
                && main.core_state.attach.client_holds_workspace(client_id)
            {
                crate::core::attach_runtime::handle_markdown_content_request(
                    &mut main.core_state,
                    hub,
                    client_id,
                    request_id,
                    surface_id,
                );
                return;
            }
        }
        for (_, engine) in self.parked_states.iter_mut() {
            if engine.attach.client_holds_workspace(client_id) {
                crate::core::attach_runtime::handle_markdown_content_request(
                    engine, hub, client_id, request_id, surface_id,
                );
                return;
            }
        }
        let payload = serde_json::json!({
            "event": "markdown_content_result",
            "request_id": request_id,
            "surface_id": surface_id,
            "ok": false,
            "reason": "client does not hold a workspace attach",
        });
        let frame = crate::ipc::stream::StreamFrame::new(
            crate::ipc::stream::StreamTag::Control,
            serde_json::to_vec(&payload).unwrap_or_default(),
        );
        let _ = hub.push(client_id, frame); // 회신 전송 실패는 여기서 재시도하지 않는다.
    }

    fn apply_git_query_request_msg(
        &mut self,
        client_id: u32,
        msg: tasty_ipc::stream_hub::GitQueryRequestMsg,
        hub: &tasty_ipc::stream_hub::StreamHub,
    ) {
        use tasty_ipc::stream_hub::GitQueryRequestMsg;
        let GitQueryRequestMsg::GitQueryRequest {
            request_id,
            surface_id,
            kind,
            worktree_path,
            diff_path,
        } = msg;
        for w in self.view.views.values_mut() {
            if let Some(main) = w.as_main_mut()
                && main.core_state.attach.client_holds_workspace(client_id)
            {
                crate::core::attach_runtime::handle_git_query_request(
                    &mut main.core_state,
                    hub,
                    client_id,
                    request_id,
                    surface_id,
                    kind,
                    worktree_path,
                    diff_path,
                );
                return;
            }
        }
        for (_, engine) in self.parked_states.iter_mut() {
            if engine.attach.client_holds_workspace(client_id) {
                crate::core::attach_runtime::handle_git_query_request(
                    engine,
                    hub,
                    client_id,
                    request_id,
                    surface_id,
                    kind,
                    worktree_path,
                    diff_path,
                );
                return;
            }
        }
        let payload = serde_json::json!({
            "event": "git_query_result",
            "request_id": request_id,
            "ok": false,
            "kind": kind.as_wire_str(),
            "reason": "client does not hold a workspace attach",
        });
        let frame = crate::ipc::stream::StreamFrame::new(
            crate::ipc::stream::StreamTag::Control,
            serde_json::to_vec(&payload).unwrap_or_default(),
        );
        let _ = hub.push(client_id, frame); // 회신 전송 실패는 여기서 재시도하지 않는다.
    }

    /// bulk 연결은 직접 점유자가 아니므로 결속된 workspace의 소유 engine으로 전달한다.
    /// begin·chunk·commit은 도착 순서대로 처리한다. 인가와 저장은 commit 경로가 검증한다.
    fn apply_bulk_event(
        &mut self,
        client_id: u32,
        event: tasty_ipc::stream_hub::BulkEvent,
        bulk_ws: u32,
        hub: &tasty_ipc::stream_hub::StreamHub,
    ) {
        use tasty_ipc::stream_hub::BulkEvent;
        match event {
            BulkEvent::Begin {
                transfer_id,
                filename,
                total_size,
            } => {
                if self
                    .with_bulk_ws_engine(bulk_ws, |engine| {
                        crate::core::attach_runtime::begin_bulk_transfer(
                            engine,
                            hub,
                            client_id,
                            transfer_id,
                            filename,
                            total_size,
                        );
                    })
                    .is_none()
                {
                    tracing::warn!(
                        "bulk transfer: no engine owns workspace {bulk_ws} — dropping begin"
                    );
                }
            }
            BulkEvent::Chunk {
                transfer_id,
                seq,
                bytes,
            } => {
                let found = self.with_bulk_ws_engine(bulk_ws, |engine| {
                    engine
                        .bulk_transfers
                        .append(client_id, transfer_id, seq, &bytes)
                });
                log_bulk_chunk_result(found, client_id, transfer_id, bulk_ws);
            }
            BulkEvent::Commit { transfer_id } => {
                let found = self.with_bulk_ws_engine(bulk_ws, |engine| {
                    // begin의 용량 검사와 같은 소유 engine 설정에서 저장 폴더를 구한다.
                    let dir =
                        crate::core::attach_runtime::resolve_bulk_transfer_dir(&engine.settings);
                    crate::core::attach_runtime::finalize_bulk_transfer(
                        engine,
                        hub,
                        client_id,
                        transfer_id,
                        bulk_ws,
                        dir,
                    );
                });
                if found.is_none() {
                    send_bulk_commit_failure(hub, client_id, transfer_id);
                }
            }
        }
    }

    fn with_bulk_ws_engine<R>(
        &mut self,
        bulk_ws: u32,
        f: impl FnOnce(&mut crate::core::CoreState) -> R,
    ) -> Option<R> {
        for w in self.view.views.values_mut() {
            if let Some(main) = w.as_main_mut()
                && main
                    .core_state
                    .find_workspace_index_for_id(bulk_ws)
                    .is_some()
            {
                return Some(f(&mut main.core_state));
            }
        }
        for (_, engine) in self.parked_states.iter_mut() {
            if engine.find_workspace_index_for_id(bulk_ws).is_some() {
                return Some(f(engine));
            }
        }
        None
    }
}

/// 창·parked 상태에서 workspace 점유자를 찾는다. 다른 App 필드와 나눠 빌릴 수 있도록 독립 함수로 둔다.
fn find_workspace_holder_engine_mut<'a>(
    views: &'a mut std::collections::HashMap<WindowId, Box<dyn View>>,
    parked_states: &'a mut [(crate::state::AppState, crate::core::CoreState)],
    client_id: u32,
) -> Option<&'a mut crate::core::CoreState> {
    for w in views.values_mut() {
        if let Some(main) = w.as_main_mut()
            && main.core_state.attach.client_holds_workspace(client_id)
        {
            return Some(&mut main.core_state);
        }
    }
    parked_states
        .iter_mut()
        .map(|(_, engine)| engine)
        .find(|engine| engine.attach.client_holds_workspace(client_id))
}

/// transfer가 등록되지 않은 경우와 workspace 소유 engine이 없는 경우를 구별해 기록한다.
fn log_bulk_chunk_result(found: Option<bool>, client_id: u32, transfer_id: u64, bulk_ws: u32) {
    match found {
        Some(true) => {}
        Some(false) => tracing::warn!(
            "bulk transfer: chunk for unknown transfer (client {client_id}, transfer {transfer_id}) — no begin? dropping"
        ),
        None => {
            tracing::warn!("bulk transfer: no engine owns workspace {bulk_ws} — dropping chunk")
        }
    }
}

/// 결속된 workspace가 사라졌으면 bulk 실패를 회신한다.
fn send_bulk_commit_failure(
    hub: &tasty_ipc::stream_hub::StreamHub,
    client_id: u32,
    transfer_id: u64,
) {
    let reply = crate::ipc::stream::StreamControl::BulkResult {
        transfer_id,
        ok: false,
        path: None,
        reason: Some("no engine owns the bound workspace".to_string()),
    };
    let frame = crate::ipc::stream::StreamFrame::new(
        crate::ipc::stream::StreamTag::Control,
        serde_json::to_vec(&reply).unwrap_or_default(),
    );
    let _ = hub.push(client_id, frame); // 회신 전송 실패는 여기서 재시도하지 않는다.
}

fn reply_structural_result(
    hub: &tasty_ipc::stream_hub::StreamHub,
    client_id: u32,
    op_id: u64,
    ok: bool,
    reason: Option<String>,
) {
    let reply = crate::ipc::stream::StreamControl::StructuralResult { op_id, ok, reason };
    let frame = crate::ipc::stream::StreamFrame::new(
        crate::ipc::stream::StreamTag::Control,
        serde_json::to_vec(&reply).unwrap_or_default(),
    );
    let _ = hub.push(client_id, frame); // 회신 전송 실패는 여기서 재시도하지 않는다.
}

fn push_structural_delta(
    hub: &tasty_ipc::stream_hub::StreamHub,
    client_id: u32,
    delta: &crate::ipc::stream::StreamControl,
) {
    let frame = crate::ipc::stream::StreamFrame::new(
        crate::ipc::stream::StreamTag::Control,
        serde_json::to_vec(delta).unwrap_or_default(),
    );
    let _ = hub.push(client_id, frame); // 회신 전송 실패는 여기서 재시도하지 않는다.
}

fn reply_mesh_error(
    hub: &tasty_ipc::stream_hub::StreamHub,
    client_id: u32,
    surface_id: u32,
    reason: &str,
) {
    let reply = crate::ipc::stream::StreamControl::MeshError {
        surface_id,
        reason: reason.to_string(),
    };
    let frame = crate::ipc::stream::StreamFrame::new(
        crate::ipc::stream::StreamTag::Control,
        serde_json::to_vec(&reply).unwrap_or_default(),
    );
    let _ = hub.push(client_id, frame); // 회신 전송 실패는 여기서 재시도하지 않는다.
}

/// 기존 대기와 새 시각 중 이른 쪽을 유지한다. Poll은 이미 즉시 처리하므로 그대로 둔다.
fn merge_wakeup(
    current: winit::event_loop::ControlFlow,
    at: std::time::Instant,
) -> winit::event_loop::ControlFlow {
    use winit::event_loop::ControlFlow;
    match current {
        ControlFlow::Poll => ControlFlow::Poll,
        ControlFlow::WaitUntil(cur) if cur <= at => current,
        _ => ControlFlow::WaitUntil(at),
    }
}

impl App {
    /// 상한 때문에 미룬 repaint를 실행하고 가장 이른 미완료 요청의 시각을 예약한다.
    /// 다른 이벤트가 오지 않아도 요청한 화면을 다시 그릴 수 있게 한다.
    fn drive_deferred_repaints(&mut self, event_loop: &ActiveEventLoop) {
        let now = std::time::Instant::now();
        let mut earliest: Option<std::time::Instant> = None;
        for w in self.view.views.values_mut() {
            let base = w.base_mut();
            if base.repaint.take_due(now) {
                base.winit.request_redraw();
            } else if let Some(at) = base.repaint.deferred_deadline() {
                earliest = Some(earliest.map_or(at, |cur: std::time::Instant| cur.min(at)));
            }
        }
        if let Some(at) = earliest {
            event_loop.set_control_flow(merge_wakeup(event_loop.control_flow(), at));
        }
    }

    fn any_pending_native_menu(&self) -> bool {
        self.view
            .views
            .values()
            .any(|w| w.as_main().is_some_and(|m| m.has_pending_native_menu()))
    }

    /// 본체·플러그인 타이머의 가장 이른 기한을 WaitUntil과 별도 waker에 함께 전달한다.
    fn sync_timer_control_flow(&mut self, event_loop: &ActiveEventLoop) {
        let deadline = min_deadline(
            self.timers.next_deadline(),
            self.plugin_manager.as_ref().and_then(|m| m.next_deadline()),
        );
        self.timer_waker.set_deadline(deadline);
        event_loop.set_control_flow(match deadline {
            Some(at) => winit::event_loop::ControlFlow::WaitUntil(at),
            None => winit::event_loop::ControlFlow::Wait,
        });
    }

    /// 보이는 DAG만 폴링을 예약하고 숨겨진 뷰의 기존 예약은 취소한다.
    fn sync_dag_poll_timers(&mut self, now: std::time::Instant) {
        use crate::adapters::ui::popup::dag_list::DAG_LIST_POPUP_ID;

        let mut active: Vec<(u32, std::time::Instant)> = Vec::new();
        let mut popup_next: Option<std::time::Instant> = None;
        for w in self.view.views.values() {
            let Some(main) = w.as_main() else {
                continue;
            };
            active.extend(main.state.dag_graph_views.pending_poll_deadlines(now));
            if main.state.popups.is_open(DAG_LIST_POPUP_ID) {
                let at = main.state.dialogs.dag_list.next_poll_at(now);
                popup_next = Some(popup_next.map_or(at, |p: std::time::Instant| p.min(at)));
            }
        }
        crate::app::timers::sync_dag_graph_timers(&mut self.timers, &active, now);
        crate::app::timers::sync_dag_list_popup_timer(&mut self.timers, popup_next, now);
    }

    fn mark_dag_graph_window_dirty(&mut self, surface_id: u32) {
        let now = std::time::Instant::now();
        for w in self.view.views.values_mut() {
            let showing = w.as_main().is_some_and(|m| {
                m.state
                    .dag_graph_views
                    .pending_poll_deadlines(now)
                    .iter()
                    .any(|(sid, _)| *sid == surface_id)
            });
            if showing {
                w.mark_dirty();
            }
        }
    }

    fn mark_dag_list_popup_windows_dirty(&mut self) {
        use crate::adapters::ui::popup::dag_list::DAG_LIST_POPUP_ID;
        for w in self.view.views.values_mut() {
            if w.as_main()
                .is_some_and(|m| m.state.popups.is_open(DAG_LIST_POPUP_ID))
            {
                w.mark_dirty();
            }
        }
    }

    /// 메뉴 결과가 마지막 workspace를 닫을 수 있어 처리 직후 빈 창의 닫기 요청도 소비한다.
    fn poll_pending_native_menus(&mut self) {
        for w in self.view.views.values_mut() {
            if let Some(main) = w.as_main_mut() {
                main.poll_pending_native_menu();
            }
        }
        self.close_self_requesting_windows();
    }
}

#[cfg(test)]
mod tests {
    use super::merge_wakeup;
    use winit::event_loop::ControlFlow;

    #[test]
    fn merge_wakeup_keeps_the_earlier_deadline() {
        let now = std::time::Instant::now();
        let early = now + std::time::Duration::from_millis(5);
        let late = now + std::time::Duration::from_millis(50);
        assert_eq!(
            merge_wakeup(ControlFlow::WaitUntil(late), early),
            ControlFlow::WaitUntil(early)
        );
        assert_eq!(
            merge_wakeup(ControlFlow::WaitUntil(early), late),
            ControlFlow::WaitUntil(early)
        );
    }

    #[test]
    fn merge_wakeup_overrides_indefinite_wait() {
        let at = std::time::Instant::now() + std::time::Duration::from_millis(5);
        assert_eq!(
            merge_wakeup(ControlFlow::Wait, at),
            ControlFlow::WaitUntil(at)
        );
    }

    #[test]
    fn merge_wakeup_leaves_poll_alone() {
        let at = std::time::Instant::now() + std::time::Duration::from_millis(5);
        assert_eq!(merge_wakeup(ControlFlow::Poll, at), ControlFlow::Poll);
    }
}
