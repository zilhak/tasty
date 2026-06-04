use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::WindowId;

use crate::view::ui::View;
use crate::view::{ViewAction, ViewCtx};
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
                use crate::app::dispatch_domain::DispatchSource;
                use crate::core::intent::CoreEvent;
                let core = &mut self.core;
                let views = &mut self.view.views;
                let parked_states = &mut self.parked_states;
                let mut pending: Vec<(DispatchSource, Vec<CoreEvent>)> = Vec::new();
                if let Some(sid) = surface_id {
                    // Targeted polling: 모든 view 의 engine 을 순회하며 해당 surface 보유 시 process
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
                            main.mark_dirty();
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
                    // Fallback: wake all views and process all terminals across engines
                    for (wid, w) in views.iter_mut() {
                        if let Some(main) = w.as_main_mut() {
                            let outcome = core.process_all_pty_output(&mut main.core_state);
                            if !outcome.events.is_empty() {
                                pending.push((DispatchSource::Main(*wid), outcome.events));
                            }
                        }
                        w.mark_dirty();
                    }
                    for (idx, (_, engine)) in parked_states.iter_mut().enumerate() {
                        let outcome = core.process_all_pty_output(engine);
                        if !outcome.events.is_empty() {
                            pending.push((DispatchSource::Parked(idx), outcome.events));
                        }
                    }
                }
                // borrow scope 종료 후 cascade dispatch.
                for (source, events) in pending {
                    for ev in events {
                        self.handle_core_event_system(source, ev);
                    }
                }
            }
            AppEvent::IpcReady => {
                if self.process_ipc()
                    && let Some(w) = self.focused_window_mut()
                {
                    w.mark_dirty();
                }
            }
            AppEvent::EguiRepaint { viewport_id } => {
                // viewport_id 가 매칭되는 view 한 개만 dirty 처리.
                // 매 frame 모든 view 를 dirty 로 만들면 한 window 의 repaint 요청이
                // 전체 fan-out 을 일으켜 single-window 환경에서도 불필요한 비용 발생.
                let matched = self
                    .view
                    .views
                    .values_mut()
                    .find(|w| w.base().gpu.egui_ctx.viewport_id() == viewport_id);
                if let Some(w) = matched {
                    w.mark_dirty();
                }
                // 매칭 실패 (shell_setup gpu 등 view 에 등록되지 않은 ctx 가 callback 을 보낸 경우)
                // 는 silently drop — 본 핸들러는 등록된 view 만 책임진다.
            }
            AppEvent::Shutdown => {
                self.flush_layout_persistence(true);
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
                    // macOS: destroy windows, park all MainView states (dock reopen restores).
                    // 모달은 파킹 대상이 아니므로 그냥 drop.
                    let drained: Vec<_> = self.view.views.drain().map(|(_, w)| w).collect();
                    for w in drained {
                        if let Some(main_box) = crate::view::unbox_main(w) {
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
                        // Windows with tray: hide windows to tray (keep alive)
                        for w in self.view.views.values() {
                            w.base().winit.set_visible(false);
                        }
                        tracing::info!("hid {} window(s) to system tray", self.view.views.len());
                    } else {
                        // Windows without tray: minimize to taskbar
                        for w in self.view.views.values() {
                            w.base().winit.set_minimized(true);
                        }
                        tracing::info!("minimized {} window(s) to taskbar", self.view.views.len());
                    }
                }
                #[cfg(not(any(target_os = "macos", windows)))]
                {
                    // Linux: minimize windows to taskbar (keep alive)
                    for w in self.view.views.values() {
                        w.base().winit.set_minimized(true);
                    }
                    tracing::info!("minimized {} window(s) to taskbar", self.view.views.len());
                }
            }
            AppEvent::QuitRequested => {
                self.handle_quit_requested(event_loop);
            }
            #[cfg(windows)]
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
                origin_surface_id,
            } => {
                tracing::debug!(
                    request_id = %request_id,
                    target = %target.display(),
                    detector = ?detector.as_ref().map(|d| d.as_str()),
                    origin_surface_id = ?origin_surface_id,
                    "IdentifyDone",
                );
                // Split borrow — focused_window_mut 는 &mut self 전체를 잡아
                // self.core 와 충돌하므로 인덱스로 직접 접근.
                if let Some(id) = self.view.focused_view_id
                    && let Some(main) = self.view.views.get_mut(&id).and_then(|w| w.as_main_mut())
                {
                    self.core.apply_identify_result(
                        &mut main.state,
                        &mut main.core_state,
                        target,
                        detector,
                        origin_surface_id,
                    );
                }
            }
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if !self.view.views.is_empty() || self.shell_setup_gpu.is_some() {
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

        let mut init_settings = crate::settings::Settings::load();
        // Validate enum-like fields up-front so GPU init and downstream consumers
        // see normalized values, and so disk reflects fallback (no recurring popups).
        let normalize_report = init_settings.normalize();
        if normalize_report.changed
            && let Err(e) = init_settings.save()
        {
            tracing::warn!("failed to persist normalized settings: {e}");
        }

        let gpu = pollster::block_on(crate::gpu::GpuState::new(
            window.clone(),
            &init_settings.appearance,
            self.view.proxy.clone(),
        ))
        .expect("failed to initialize GPU");

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
        // invalid_theme_name 보고는 init_app_state 가 직접 수행하므로 normalize_report 는 더
        // 이상 여기서 소비되지 않는다. 변수 자체는 normalize 호출 결과 보존용으로 둔다.
        drop(normalize_report);

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
                            let normalize_report = settings.normalize();
                            settings.general.shell = self.shell_setup_path.clone();
                            if let Err(e) = settings.save() {
                                tracing::error!("failed to save settings: {e}");
                            }
                            self.shell_setup_mode = false;
                            let window = self.shell_setup_window.take().unwrap();
                            let gpu = self.shell_setup_gpu.take().unwrap();
                            self.init_app_state(window, gpu, settings);
                            // invalid_theme_name 처리는 init_app_state 내부로 이동했으므로
                            // normalize_report 는 여기서 별도 소비할 필요 없음.
                            drop(normalize_report);
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
        if let Some(modal_id) = self.view.active_modal_id
            && id == modal_id
        {
            let action = if let Some(modal) = self.view.views.get_mut(&id) {
                let mut ctx = ViewCtx {
                    event_loop,
                    modal_active: false,
                    plugin_manager: self.plugin_manager.as_ref(),
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
            return;
        }

        // Normal mode — find the window by ID and delegate
        if let WindowEvent::CloseRequested = &event {
            // MainView 개수 기준으로 판단 (모달은 수에 포함되지 않음)
            let main_window_count = self
                .view
                .views
                .values()
                .filter(|w| w.as_main().is_some())
                .count();
            if main_window_count > 1 {
                // Multiple windows: just close this one
                self.view.views.remove(&id);
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
                if self.view.focused_view_id == Some(id) {
                    self.view.focused_view_id = self
                        .view
                        .views
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
            // 모달이 focus 이벤트를 받아도 focused_view_id는 MainView 전용
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
            // If a modal is active, bring it to the front so it's not buried
            if let Some(modal_id) = self.view.active_modal_id
                && let Some(modal) = self.view.views.get(&modal_id)
            {
                modal.base().winit.focus_window();
            }
        }

        // Trigger modal shake when clicking on a non-modal window while modal is active
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

        let modal_active = self.view.is_modal_active();
        let action = {
            if let Some(w) = self.view.views.get_mut(&id) {
                let mut ctx = ViewCtx {
                    event_loop,
                    modal_active,
                    plugin_manager: self.plugin_manager.as_ref(),
                };
                // MainView.handle_event는 항상 ViewAction::None을 반환한다.
                // PresetView (modeless editor) 는 CloseRequested 에서 Close 를 반환하므로
                // 이 경로에서 처리한다. 그 외 modal Close 는 위쪽 모달 경로에서 소비된다.
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

            // Check if the window requested to close (e.g. last workspace removed)
            let close_requested = self
                .view
                .views
                .get(&id)
                .map(|w| w.base().close_requested)
                .unwrap_or(false);
            if close_requested {
                if let Some(w) = self.view.views.remove(&id)
                    && self.view.views.values().all(|w| w.as_main().is_none())
                    && let Some(main_box) = crate::view::unbox_main(w)
                {
                    tracing::info!("last main window closed via request, parking state");
                    self.parked_states
                        .push((main_box.state, main_box.core_state));
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
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        event_loop.set_control_flow(winit::event_loop::ControlFlow::Wait);

        if self.process_ipc()
            && let Some(w) = self.focused_window_mut()
        {
            w.mark_dirty();
        }

        // Plugin host pump — process plugin events, run health checks, restart unresponsive.
        let hello_pairs = if let Some(ref mut mgr) = self.plugin_manager {
            mgr.pump()
        } else {
            Vec::new()
        };
        // hello 직후 surface_kind 등록 + PluginLoaded / PluginSurfaceKindRegistered
        // CoreEvent 발화. 큐 우회 sync 호출 (cascade 즉시).
        self.finalize_plugin_hello(hello_pairs);
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
        // Background update poller → "Tasty update available" notification.
        // None→Some 전이 시 1회만, notified_version 으로 중복 차단.
        self.dispatch_pending_update_notifications();
        // 호스트 내부 Intent 큐 drain — UI Intent 와 Domain Intent (Intent::Domain
        // wrapper) 모두 매 frame 일관 처리 (intent-ui-vs-domain.md §4.4).
        // dispatch_pending_intents 가 domain_batch 를 따로 모아 cascade 까지 일괄.
        self.dispatch_pending_intents();
        // 도구 메뉴 ToolAction::OpenPopup 클릭으로 enqueue된 popup open dispatch.
        self.dispatch_pending_popup_opens();
        // 파일 핸들러 IPC action 큐 drain (Phase C1: warn 로그만, Phase C3: 본격 dispatch).
        self.dispatch_pending_handler_ipc();
        // 파일 handler picker popup 의 result 슬롯 drain (D.3.C.G.3.c).
        self.dispatch_pending_picker_results();
        // 직전 프레임 plugin popup 렌더로 수집된 사용자 입력 / close 사유 forward.
        self.dispatch_plugin_popup_events();
        // PluginsView 모달의 사용자 액션을 manager에 적용 + 모달 snapshot 갱신.
        self.process_plugins_window_actions();
        // PresetView 열기 + (Intent::SavePreset cascade 시) 선택. preset 저장/적용/삭제/이름변경
        // 자체는 Intent 큐 (`dispatch_pending_intents`) 가 처리한다.
        self.process_pending_open_preset_window(event_loop);

        // Poll system tray menu events (Windows only)
        #[cfg(windows)]
        if let Some(ref ids) = self.tray_menu_ids {
            if let Some(menu_id) = crate::system_tray::poll_menu_event() {
                if menu_id == ids.show_window {
                    crate::shortcuts::send_app_event(&self.view.proxy, AppEvent::TrayShowWindow);
                } else if menu_id == ids.new_window {
                    crate::shortcuts::send_app_event(&self.view.proxy, AppEvent::CreateWindow);
                } else if menu_id == ids.quit {
                    crate::shortcuts::send_app_event(&self.view.proxy, AppEvent::Shutdown);
                }
            }
        }

        // Tick modal shake animation.
        self.tick_modal_shake();

        // Flush layout persistence (debounced).
        self.flush_layout_persistence(false);

        // Flush deferred PTY resizes (throttled to 100ms intervals).
        // If any terminal still has a pending resize (throttled), request a redraw
        // so we retry on the next frame.
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
}
