use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::WindowId;

use crate::window::{Window, WindowAction, WindowCtx};
use crate::{App, AppEvent};

impl ApplicationHandler<AppEvent> for App {
    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::CreateWindow => {
                self.create_new_window(event_loop);
            }
            AppEvent::OpenSettings => {
                self.open_settings_modal(event_loop);
            }
            AppEvent::OpenPlugins => {
                self.open_plugins_modal(event_loop);
            }
            AppEvent::TerminalOutput(surface_id) => {
                if let Some(sid) = surface_id {
                    // Targeted polling: process only the specific terminal, then wake its window
                    let mut found = false;
                    if self.engine_state().find_terminal_by_id(sid).is_some() {
                        self.engine_state_mut().process_surface(sid);
                        for w in self.windows.values_mut() {
                            let Some(main) = w.as_main_mut() else {
                                continue;
                            };
                            main.recalc_ime_preedit_anchor();
                            main.mark_dirty();
                            found = true;
                            break;
                        }
                    }
                    let _ = found;
                } else {
                    // Fallback: window_id not matched — wake all windows and process all
                    for w in self.windows.values_mut() {
                        w.mark_dirty();
                    }
                    self.engine_state_mut().process_all();
                }
            }
            AppEvent::IpcReady => {
                if self.process_ipc() {
                    if let Some(w) = self.focused_window_mut() {
                        w.mark_dirty();
                    }
                }
            }
            AppEvent::EguiRepaint => {
                for w in self.windows.values_mut() {
                    w.mark_dirty();
                }
            }
            AppEvent::Shutdown => {
                self.flush_layout_persistence_final();
                if let Some(ref mut mgr) = self.plugin_manager {
                    // Event Bus 1.0: `system.shutdown_initiated`는 plugin 종료 전에
                    // broadcast해 구독자가 cleanup hook을 돌릴 시간을 준다.
                    use tasty_plugin_protocol::EventScope;
                    use tasty_plugin_protocol::events::payloads::SystemShutdownInitiated;
                    mgr.emit_host_event(
                        "system.shutdown_initiated",
                        &SystemShutdownInitiated {
                            reason: "user_quit".to_string(),
                        },
                        EventScope::System,
                    );
                    mgr.shutdown_all();
                }
                event_loop.exit();
            }
            AppEvent::Minimize => {
                #[cfg(target_os = "macos")]
                {
                    // macOS: destroy windows, park all MainWindow states (dock reopen restores).
                    // 모달은 파킹 대상이 아니므로 그냥 drop.
                    let drained: Vec<_> = self.windows.drain().map(|(_, w)| w).collect();
                    for w in drained {
                        if let Some(main_box) = crate::window::unbox_main(w) {
                            self.parked_states.push(main_box.state);
                        }
                    }
                    self.engine.focused_window_id = None;
                    self.engine.active_modal_id = None;
                    tracing::info!(
                        "minimized to background ({} states parked)",
                        self.parked_states.len()
                    );
                }
                #[cfg(windows)]
                {
                    if self.tray_icon.is_some() {
                        // Windows with tray: hide windows to tray (keep alive)
                        for w in self.windows.values() {
                            w.base().winit.set_visible(false);
                        }
                        tracing::info!("hid {} window(s) to system tray", self.windows.len());
                    } else {
                        // Windows without tray: minimize to taskbar
                        for w in self.windows.values() {
                            w.base().winit.set_minimized(true);
                        }
                        tracing::info!("minimized {} window(s) to taskbar", self.windows.len());
                    }
                }
                #[cfg(not(any(target_os = "macos", windows)))]
                {
                    // Linux: minimize windows to taskbar (keep alive)
                    for w in self.windows.values() {
                        w.base().winit.set_minimized(true);
                    }
                    tracing::info!("minimized {} window(s) to taskbar", self.windows.len());
                }
            }
            AppEvent::QuitRequested => {
                self.handle_quit_requested(event_loop);
            }
            #[cfg(windows)]
            AppEvent::TrayShowWindow => {
                for w in self.windows.values() {
                    w.base().winit.set_visible(true);
                    w.base().winit.set_minimized(false);
                    w.base().winit.focus_window();
                }
                tracing::info!("restored {} window(s) from system tray", self.windows.len());
            }
            AppEvent::ClipboardChanged(data) => {
                self.record_clipboard_data(data);
            }
            AppEvent::BusyPoll => {
                self.poll_busy_states();
            }
            AppEvent::IdentifyDone {
                request_id,
                target,
                detector,
            } => {
                tracing::debug!(
                    request_id = %request_id,
                    target = %target.display(),
                    detector = ?detector.as_ref().map(|d| d.as_str()),
                    "IdentifyDone",
                );
                if let Some(w) = self.focused_window_mut() {
                    if let Some(main) = w.as_main_mut() {
                        crate::file_dispatch::apply_identify_result(
                            &mut main.state,
                            &mut main.engine_state,
                            target,
                            detector,
                        );
                    }
                }
            }
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if !self.windows.is_empty() || self.shell_setup_gpu.is_some() {
            return;
        }

        #[cfg(target_os = "macos")]
        crate::macos_delegate::inject_delegate_methods();

        use std::sync::Arc;
        use winit::window::WindowAttributes;

        let mut attrs = WindowAttributes::default()
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

        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .expect("failed to create window"),
        );

        let init_settings = crate::settings::Settings::load();

        let gpu = pollster::block_on(crate::gpu::GpuState::new(
            window.clone(),
            &init_settings.appearance,
            self.engine.proxy.clone(),
        ))
        .expect("failed to initialize GPU");

        let mut init_settings = init_settings;
        if !init_settings.general.is_shell_valid() {
            if let Some(detected) = crate::settings::GeneralSettings::detect_bash() {
                tracing::info!("configured shell invalid; auto-detected bash at {detected}");
                init_settings.general.shell = detected;
                if let Err(e) = init_settings.save() {
                    tracing::warn!("failed to save auto-detected shell: {e}");
                }
            } else {
                tracing::warn!("bash not found; entering shell setup mode");
                self.shell_setup_mode = true;
                self.shell_setup_path = String::new();
                self.shell_setup_gpu = Some(gpu);
                self.shell_setup_window = Some(window);
                return;
            }
        }

        window.set_ime_allowed(true);
        self.init_app_state(window, gpu, init_settings);

        #[cfg(windows)]
        crate::jump_list::setup_jump_list();

        #[cfg(windows)]
        {
            if let Some((tray, ids)) = crate::system_tray::create_tray_icon() {
                self.tray_icon = Some(tray);
                self.tray_menu_ids = Some(ids);
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        // Shell setup mode — handled by App directly
        if self.shell_setup_mode {
            if let WindowEvent::RedrawRequested = &event {
                if let (Some(gpu), Some(window)) =
                    (&mut self.shell_setup_gpu, &self.shell_setup_window)
                {
                    let result = gpu.render_shell_setup(window, &mut self.shell_setup_path);
                    match result {
                        Ok(crate::gpu::ShellSetupAction::None) => {}
                        Ok(crate::gpu::ShellSetupAction::Confirmed) => {
                            let mut settings = crate::settings::Settings::load();
                            settings.general.shell = self.shell_setup_path.clone();
                            if let Err(e) = settings.save() {
                                tracing::error!("failed to save settings: {e}");
                            }
                            self.shell_setup_mode = false;
                            let window = self.shell_setup_window.take().unwrap();
                            let gpu = self.shell_setup_gpu.take().unwrap();
                            self.init_app_state(window, gpu, settings);
                            if let Some(w) = self.focused_window_mut() {
                                w.mark_dirty();
                            }
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
                if let (Some(gpu), Some(window)) =
                    (&mut self.shell_setup_gpu, &self.shell_setup_window)
                {
                    gpu.handle_egui_event(window, &event);
                }
                return;
            }
            if let (Some(gpu), Some(window)) = (&mut self.shell_setup_gpu, &self.shell_setup_window)
            {
                gpu.handle_egui_event(window, &event);
                if let WindowEvent::CloseRequested = &event {
                    event_loop.exit();
                }
            }
            return;
        }

        // Modal handling — 활성 모달을 대상으로 한 이벤트
        if let Some(modal_id) = self.engine.active_modal_id {
            if id == modal_id {
                let Self {
                    windows,
                    plugin_manager,
                    engine_state,
                    ..
                } = self;
                let action = if let Some(modal) = windows.get_mut(&id) {
                    let mut ctx = WindowCtx {
                        event_loop,
                        modal_active: false,
                        plugin_manager: plugin_manager.as_ref(),
                        engine_state: engine_state
                            .as_mut()
                            .expect("App.engine_state must be initialized"),
                    };
                    modal.handle_event(event, &mut ctx)
                } else {
                    WindowAction::None
                };

                match action {
                    WindowAction::None => {}
                    WindowAction::Close => {
                        self.close_active_modal();
                    }
                    WindowAction::CloseWithEvent(app_event) => {
                        self.close_active_modal();
                        crate::shortcuts::send_app_event(&self.engine.proxy, app_event);
                    }
                }
                return;
            }
        }

        // Normal mode — find the window by ID and delegate
        if let WindowEvent::CloseRequested = &event {
            // MainWindow 개수 기준으로 판단 (모달은 수에 포함되지 않음)
            let main_window_count = self
                .windows
                .values()
                .filter(|w| w.as_main().is_some())
                .count();
            if main_window_count > 1 {
                // Multiple windows: just close this one
                self.windows.remove(&id);
                if let Some(mgr) = self.plugin_manager.as_mut() {
                    use tasty_plugin_protocol::events::payloads::WindowClosed;
                    use tasty_plugin_protocol::{EventScope, LifecycleReason};
                    let payload = WindowClosed {
                        window_id: u64::from(id),
                        reason: LifecycleReason::User,
                    };
                    mgr.emit_host_event("window.closed", &payload, EventScope::System);
                    crate::hooks::lua::fire(
                        self.lua_engine.as_ref(),
                        "window.delete.post",
                        &payload,
                    );
                }
                if self.engine.focused_window_id == Some(id) {
                    self.engine.focused_window_id = self
                        .windows
                        .iter()
                        .find(|(_, w)| w.as_main().is_some())
                        .map(|(id, _)| *id);
                }
            } else {
                // Last main window: route through quit logic
                self.handle_quit_requested(event_loop);
            }
            return;
        }

        // Track focused window on focus events
        if let WindowEvent::Focused(true) = &event {
            // 모달이 focus 이벤트를 받아도 focused_window_id는 MainWindow 전용
            let is_main = self
                .windows
                .get(&id)
                .map(|w| w.as_main().is_some())
                .unwrap_or(false);
            if is_main {
                self.engine.focused_window_id = Some(id);
            }
            if let Some(mgr) = self.plugin_manager.as_mut() {
                use tasty_plugin_protocol::EventScope;
                use tasty_plugin_protocol::events::payloads::WindowFocused;
                let payload = WindowFocused {
                    window_id: u64::from(id),
                };
                mgr.emit_host_event("window.focused", &payload, EventScope::System);
            }
            // If a modal is active, bring it to the front so it's not buried
            if let Some(modal_id) = self.engine.active_modal_id {
                if let Some(modal) = self.windows.get(&modal_id) {
                    modal.base().winit.focus_window();
                }
            }
        }

        // Trigger modal shake when clicking on a non-modal window while modal is active
        if self.engine.is_modal_active() {
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

        // Plugin shortcut interception (단계 F): focused surface가 plugin
        // RemoteSurface면 host action 매칭 전에 plugin command와 비교한다.
        // 매칭 시 plugin에 dispatch + 이벤트 소모 → window.handle_event로 흐르지 않음.
        let plugin_consumed = if let WindowEvent::KeyboardInput { event: ke, .. } = &event {
            self.try_plugin_shortcut(id, ke)
        } else {
            false
        };
        if plugin_consumed {
            return;
        }

        let modal_active = self.engine.is_modal_active();
        let action = {
            let Self {
                windows,
                plugin_manager,
                engine_state,
                ..
            } = self;
            if let Some(w) = windows.get_mut(&id) {
                let mut ctx = WindowCtx {
                    event_loop,
                    modal_active,
                    plugin_manager: plugin_manager.as_ref(),
                    engine_state: engine_state
                        .as_mut()
                        .expect("App.engine_state must be initialized"),
                };
                // MainWindow.handle_event는 항상 WindowAction::None을 반환한다.
                // PresetWindow (modeless editor) 는 CloseRequested 에서 Close 를 반환하므로
                // 이 경로에서 처리한다. 그 외 modal Close 는 위쪽 모달 경로에서 소비된다.
                w.handle_event(event, &mut ctx)
            } else {
                WindowAction::None
            }
        };
        if self.windows.contains_key(&id) {
            match action {
                WindowAction::None => {}
                WindowAction::Close => {
                    if self.preset_window_id == Some(id) {
                        self.on_preset_window_closed(id);
                        return;
                    }
                    debug_assert!(false, "non-modal window returned Close unexpectedly");
                }
                WindowAction::CloseWithEvent(app_event) => {
                    if self.preset_window_id == Some(id) {
                        self.on_preset_window_closed(id);
                        crate::shortcuts::send_app_event(&self.engine.proxy, app_event);
                        return;
                    }
                    debug_assert!(
                        false,
                        "non-modal window returned CloseWithEvent unexpectedly"
                    );
                }
            }

            // Check if the window requested to close (e.g. last workspace removed)
            let close_requested = self
                .windows
                .get(&id)
                .map(|w| w.base().close_requested)
                .unwrap_or(false);
            if close_requested {
                if let Some(w) = self.windows.remove(&id) {
                    if self.windows.values().all(|w| w.as_main().is_none()) {
                        if let Some(main_box) = crate::window::unbox_main(w) {
                            tracing::info!("last main window closed via request, parking state");
                            self.parked_states.push(main_box.state);
                        }
                    }
                }
                if self.engine.focused_window_id == Some(id) {
                    self.engine.focused_window_id = self
                        .windows
                        .iter()
                        .find(|(_, w)| w.as_main().is_some())
                        .map(|(id, _)| *id);
                }
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        event_loop.set_control_flow(winit::event_loop::ControlFlow::Wait);

        if self.process_ipc() {
            if let Some(w) = self.focused_window_mut() {
                w.mark_dirty();
            }
        }

        // Plugin host pump — process plugin events, run health checks, restart unresponsive.
        if let Some(ref mut mgr) = self.plugin_manager {
            mgr.pump();
        }
        // plugin이 보낸 IPC 호출들을 라우터로 디스패치 (권한 게이트 적용).
        self.process_plugin_ipc_calls();
        // surface close lifecycle 알림 drain → 구독 plugin에 broadcast.
        self.dispatch_pending_surface_lifecycle();
        // Event Bus 1.0 호스트 자동 발화 큐 drain (focus 변화 감지 포함).
        self.dispatch_pending_host_events();
        // tasty-memory regular 변경 → memory.changed host event.
        self.dispatch_pending_memory_changes();
        // 도구 메뉴 클릭으로 enqueue된 이벤트 publish.
        self.dispatch_pending_tool_events();
        // 호스트 내부 Intent 큐 drain (plugin popup 큐 발화 가능하므로 plugin drain 앞).
        self.dispatch_pending_intents();
        // 도구 메뉴 ToolAction::OpenPopup 클릭으로 enqueue된 popup open dispatch.
        self.dispatch_pending_popup_opens();
        // 파일 핸들러 IPC action 큐 drain (Phase C1: warn 로그만, Phase C3: 본격 dispatch).
        self.dispatch_pending_handler_ipc();
        // 직전 프레임 plugin popup 렌더로 수집된 사용자 입력 / close 사유 forward.
        self.dispatch_plugin_popup_events();
        // PluginsWindow 모달의 사용자 액션을 manager에 적용 + 모달 snapshot 갱신.
        self.process_plugins_window_actions();
        // PresetWindow 열기 + (Intent::SavePreset cascade 시) 선택. preset 저장/적용/삭제/이름변경
        // 자체는 Intent 큐 (`dispatch_pending_intents`) 가 처리한다.
        self.process_pending_open_preset_window(event_loop);

        // Poll system tray menu events (Windows only)
        #[cfg(windows)]
        if let Some(ref ids) = self.tray_menu_ids {
            if let Some(menu_id) = crate::system_tray::poll_menu_event() {
                if menu_id == ids.show_window {
                    crate::shortcuts::send_app_event(&self.engine.proxy, AppEvent::TrayShowWindow);
                } else if menu_id == ids.new_window {
                    crate::shortcuts::send_app_event(&self.engine.proxy, AppEvent::CreateWindow);
                } else if menu_id == ids.quit {
                    crate::shortcuts::send_app_event(&self.engine.proxy, AppEvent::Shutdown);
                }
            }
        }

        // Tick modal shake animation.
        self.tick_modal_shake();

        // Flush layout persistence (debounced).
        self.flush_layout_persistence();

        // Flush deferred PTY resizes (throttled to 100ms intervals).
        // If any terminal still has a pending resize (throttled), request a redraw
        // so we retry on the next frame.
        let any_pending = self.engine_state_mut().flush_all_pty_resizes();
        if any_pending {
            for w in self.windows.values() {
                w.base().winit.request_redraw();
            }
        }
    }
}
