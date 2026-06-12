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
                // Early reset: 채널 drain 직전에 dedup 게이트를 풀어, drain 과 경합하는
                // reader wake 가 스킵되어 유실되는 것을 막는다 (research §8). Some →
                // 해당 surface 게이트, None → 글로벌 게이트. surface 가 없는 factory 의
                // note_drained 는 무해한 no-op 이므로 전 view/parked 를 순회한다.
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
            AppEvent::StreamReady => {
                // 스트림 클라 inbound 를 분류해 attach 결선(단계 4). 렌더 상태와 무관해
                // dirty 처리 불필요. 끊긴 client lock 은 전 engine 에서 자동 free 환원.
                let outcome = self.stream_hub.pump_inbound(&self.stream_inbound_rx);
                self.apply_stream_outcome(outcome);
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
                self.shutdown_lifecycle_cascade(event_loop);
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
            AppEvent::AttachPoll => {
                self.poll_attach_views();
            }
            // 단계 7 — 자동 attach 워커가 SSH 터널 수립을 마쳤다(wake). 결과를 drain 해
            // mirror 를 띄운다(idle 상태에서도 즉시 반영).
            AppEvent::AutoAttachReady => {
                self.drain_auto_attach_results();
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
            // PresetView (modeless editor) — 바로 닫힌다. quit 흐름 거치지 않음.
            // 메인 윈도우 개수와 무관하게 자기 자신만 닫는 게 의도된 동작.
            if self.preset_view_id == Some(id) {
                self.on_preset_window_closed(id);
                return;
            }
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

        // attach/detach 작업 J — IPC 가 쌓은 GUI attach 트리거 실행(원격 워크스페이스
        // mirror 재구성). process_ipc 직후라야 같은 frame 에 반영된다.
        self.dispatch_pending_gui_attach();

        // attach/detach 단계 7 — 매핑된 워크스페이스 자동 attach. 활성 ws 가 매핑 Some &
        // 미attach 면 SSH 터널 워커를 spawn(무블록)하고, 완료된 결과를 drain 해 mirror
        // 를 띄운다(원격 워크스페이스 = 로컬 워크스페이스 매핑의 종착점).
        self.poll_auto_attach();

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

impl App {
    /// stream client 들이 끊겼을 때 그들이 잡고 있던 attach lock 을 모든 engine
    /// (활성 main view + parked)에서 자동 해제한다(attach/detach 단계 3 EOF 해제).
    /// 한 client_id 는 한 engine 에만 lock 을 가지므로 전 engine 순회는 멱등·안전.
    pub(crate) fn release_attach_for_disconnected(&mut self, clients: &[u32]) {
        for w in self.view.views.values_mut() {
            if let Some(main) = w.as_main_mut() {
                for &cid in clients {
                    main.core_state.attach.release_all_for_client(cid);
                }
            }
        }
        for (_, engine) in self.parked_states.iter_mut() {
            for &cid in clients {
                engine.attach.release_all_for_client(cid);
            }
        }
    }

    /// `pump_inbound` 가 분류한 stream inbound 를 적용한다(attach/detach 단계 4).
    /// gui 는 engine 이 여럿(활성 main view + parked)이라, 각 요청을 *대상 surface 를
    /// 소유한 engine* 에 라우팅한다. 끊김은 전 engine 해제(멱등).
    pub(crate) fn apply_stream_outcome(
        &mut self,
        outcome: crate::adapters::production::stream_hub::PumpOutcome,
    ) {
        // StreamHub 는 Arc clone(저렴) — 필드 동시 차용 회피용.
        let hub = self.stream_hub.clone();

        for (client_id, surface_id) in outcome.attach_requests {
            if !self.attach_on_owning_engine(surface_id, client_id, &hub) {
                // 어떤 engine 도 이 surface 를 소유하지 않음 → 거부.
                crate::core::attach_runtime::reject_attach(&hub, client_id, "not_found", None);
            }
        }

        for (client_id, workspace_id) in outcome.workspace_attach_requests {
            if !self.attach_workspace_on_owning_engine(workspace_id, client_id, &hub) {
                crate::core::attach_runtime::reject_attach(
                    &hub,
                    client_id,
                    "workspace_not_found",
                    None,
                );
            }
        }

        for (client_id, bytes) in outcome.input_frames {
            let routed = self.feed_stream_input(client_id, &bytes);
            #[cfg(debug_assertions)]
            if !routed {
                // 단계 1 echo client(점유 surface 없음): debug 빌드 회신.
                let _ = self.stream_hub.push(
                    client_id,
                    crate::ipc::stream::StreamFrame::new(
                        crate::ipc::stream::StreamTag::Data,
                        bytes,
                    ),
                );
            }
            #[cfg(not(debug_assertions))]
            let _ = routed;
        }

        if !outcome.disconnected.is_empty() {
            self.release_attach_for_disconnected(&outcome.disconnected);
        }

        // 작업 J: attach/detach 직후 즉시 서버 readonly display mirror 를 채워(또는
        // 해제분 정리) 첫 3초 tick 전 blank 를 없앤다. 점유 mirror 있는 window 만 dirty.
        for w in self.view.views.values_mut() {
            if let Some(main) = w.as_main_mut()
                && main.core_state.refresh_readonly_views()
            {
                w.mark_dirty();
            }
        }
    }

    /// 대상 surface 를 소유한 engine 을 찾아 attach 결선. 소유 engine 없으면 false.
    fn attach_on_owning_engine(
        &mut self,
        surface_id: u32,
        client_id: u32,
        hub: &crate::adapters::production::stream_hub::StreamHub,
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

    /// 대상 workspace 를 소유한 engine 을 찾아 workspace attach 결선(단계 6). 없으면 false.
    fn attach_workspace_on_owning_engine(
        &mut self,
        workspace_id: u32,
        client_id: u32,
        hub: &crate::adapters::production::stream_hub::StreamHub,
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

    /// stream client 입력을 적절한 engine 으로. workspace mode(단계 6)면 입력은
    /// surface-prefixed → demux 후 지정 surface; 아니면 단계 4 의 bare 단일 surface.
    fn feed_stream_input(&mut self, client_id: u32, bytes: &[u8]) -> bool {
        // workspace 점유 engine 우선(client_holds_workspace).
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
        // surface 단위(단계 4) 폴백.
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

    /// client 가 점유한 surface 를 가진 engine 에 입력 전달. 없으면 false.
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
}
