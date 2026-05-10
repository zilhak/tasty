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
            AppEvent::TerminalOutput(surface_id) => {
                if let Some(sid) = surface_id {
                    // Targeted polling: process only the specific terminal, then wake its window
                    let mut found = false;
                    for w in self.windows.values_mut() {
                        let Some(main) = w.as_main_mut() else {
                            continue;
                        };
                        if main.state.engine.find_terminal_by_id(sid).is_some() {
                            main.state.engine.process_surface(sid);
                            main.recalc_ime_preedit_anchor();
                            main.mark_dirty();
                            found = true;
                            break;
                        }
                    }
                    // If no window has this surface, it might be in the parked states
                    if !found {
                        for state in &mut self.parked_states {
                            if state.engine.find_terminal_by_id(sid).is_some() {
                                state.engine.process_surface(sid);
                                break;
                            }
                        }
                    }
                } else {
                    // Fallback: window_id not matched — wake all windows
                    for w in self.windows.values_mut() {
                        w.mark_dirty();
                    }
                    // Also process parked state terminals
                    for state in &mut self.parked_states {
                        state.engine.process_all();
                    }
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
                let action = if let Some(modal) = self.windows.get_mut(&id) {
                    let mut ctx = WindowCtx {
                        event_loop,
                        modal_active: false,
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
                        let _ = self.engine.proxy.send_event(app_event);
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

        if let Some(w) = self.windows.get_mut(&id) {
            let modal_active = self.engine.is_modal_active();
            let mut ctx = WindowCtx {
                event_loop,
                modal_active,
            };
            let _ = w.handle_event(event, &mut ctx);

            // Check if the window requested to close (e.g. last workspace removed)
            if w.base().close_requested {
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

        // Poll system tray menu events (Windows only)
        #[cfg(windows)]
        if let Some(ref ids) = self.tray_menu_ids {
            if let Some(menu_id) = crate::system_tray::poll_menu_event() {
                if menu_id == ids.show_window {
                    let _ = self.engine.proxy.send_event(AppEvent::TrayShowWindow);
                } else if menu_id == ids.new_window {
                    let _ = self.engine.proxy.send_event(AppEvent::CreateWindow);
                } else if menu_id == ids.quit {
                    let _ = self.engine.proxy.send_event(AppEvent::Shutdown);
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
        let mut any_pending = false;
        for w in self.windows.values_mut() {
            let Some(main) = w.as_main_mut() else {
                continue;
            };
            if main.state.engine.flush_all_pty_resizes() {
                any_pending = true;
            }
        }
        for state in &mut self.parked_states {
            if state.engine.flush_all_pty_resizes() {
                any_pending = true;
            }
        }
        if any_pending {
            for w in self.windows.values() {
                w.base().winit.request_redraw();
            }
        }
    }
}

impl App {
    /// Flush layout persistence if debounce timer has elapsed.
    fn flush_layout_persistence(&mut self) {
        for w in self.windows.values_mut() {
            let Some(main) = w.as_main_mut() else {
                continue;
            };
            if main.state.engine.settings.general.restore_layout
                && main.state.engine.layout_dirty.should_flush()
            {
                crate::layout_persistence::save_to_disk(
                    &main.state.engine,
                    main.state.active_workspace,
                );
                main.state.engine.layout_dirty.clear();
            }
        }
        for state in &mut self.parked_states {
            if state.engine.settings.general.restore_layout
                && state.engine.layout_dirty.should_flush()
            {
                crate::layout_persistence::save_to_disk(&state.engine, 0);
                state.engine.layout_dirty.clear();
            }
        }
    }

    /// Force flush layout persistence on shutdown (ignore debounce).
    fn flush_layout_persistence_final(&mut self) {
        for w in self.windows.values_mut() {
            let Some(main) = w.as_main_mut() else {
                continue;
            };
            if main.state.engine.settings.general.restore_layout
                && main.state.engine.layout_dirty.is_dirty()
            {
                crate::layout_persistence::save_to_disk(
                    &main.state.engine,
                    main.state.active_workspace,
                );
                main.state.engine.layout_dirty.clear();
            }
        }
        for state in &mut self.parked_states {
            if state.engine.settings.general.restore_layout
                && state.engine.layout_dirty.is_dirty()
            {
                crate::layout_persistence::save_to_disk(&state.engine, 0);
                state.engine.layout_dirty.clear();
            }
        }
    }

    /// Start a modal shake animation. No-op if already shaking.
    fn trigger_modal_shake(&mut self) {
        if self.modal_shake.is_some() {
            return;
        }
        let modal_id = match self.engine.active_modal_id {
            Some(id) => id,
            None => return,
        };
        let origin = match self.windows.get(&modal_id) {
            Some(w) => match w.base().winit.outer_position() {
                Ok(pos) => pos,
                Err(_) => return,
            },
            None => return,
        };
        self.modal_shake = Some(crate::ModalShake {
            start: std::time::Instant::now(),
            origin,
        });
    }

    /// Advance the modal shake animation. Called from about_to_wait.
    fn tick_modal_shake(&mut self) {
        const SHAKE_DURATION_MS: u128 = 300;
        const SHAKE_AMPLITUDE: f64 = 8.0;
        const SHAKE_FREQUENCY: f64 = 3.0; // full oscillations

        let shake = match &self.modal_shake {
            Some(s) => s,
            None => return,
        };
        let elapsed_ms = shake.start.elapsed().as_millis();
        if elapsed_ms >= SHAKE_DURATION_MS {
            // Animation done — restore original position
            let origin = shake.origin;
            let modal_id = self.engine.active_modal_id;
            self.modal_shake = None;
            if let Some(id) = modal_id {
                if let Some(w) = self.windows.get(&id) {
                    w.base()
                        .winit
                        .set_outer_position(winit::dpi::PhysicalPosition::new(origin.x, origin.y));
                }
            }
            return;
        }

        // Damped sine wave: amplitude * sin(freq * t) * (1 - t)
        let t = elapsed_ms as f64 / SHAKE_DURATION_MS as f64;
        let offset_x =
            (SHAKE_AMPLITUDE * (t * SHAKE_FREQUENCY * 2.0 * std::f64::consts::PI).sin() * (1.0 - t))
                as i32;
        let origin = shake.origin;
        if let Some(id) = self.engine.active_modal_id {
            if let Some(w) = self.windows.get(&id) {
                w.base().winit.set_outer_position(
                    winit::dpi::PhysicalPosition::new(origin.x + offset_x, origin.y),
                );
                w.base().winit.request_redraw();
            }
        }
    }

    /// Refresh the busy-surface cache for every live AppState. Triggered ~1s
    /// from the background ticker via `AppEvent::BusyPoll`. Marks any window
    /// whose set actually changed as dirty so the indicators redraw.
    fn poll_busy_states(&mut self) {
        for w in self.windows.values_mut() {
            let changed = match w.as_main_mut() {
                Some(main) => {
                    // Drain pending restore commands (queued during layout restore).
                    drain_restore_commands(&mut main.state);
                    main.state.refresh_busy_surfaces()
                }
                None => false,
            };
            if changed {
                w.mark_dirty();
            }
        }
        for state in &mut self.parked_states {
            drain_restore_commands(state);
            let _ = state.refresh_busy_surfaces();
        }
    }

    /// Record clipboard data from the background polling thread into all engines.
    fn record_clipboard_data(&mut self, data: crate::ClipboardData) {
        let source = crate::clipboard_history::ClipboardSource::System;
        let all_engines: Vec<&mut crate::engine_state::EngineState> = self
            .windows
            .values_mut()
            .filter_map(|w| w.as_main_mut())
            .map(|m| &mut m.state.engine)
            .chain(self.parked_states.iter_mut().map(|s| &mut s.engine))
            .collect();
        for engine in all_engines {
            if !engine.settings.clipboard.history_enabled {
                continue;
            }
            match &data {
                crate::ClipboardData::Text(text) => {
                    engine.clipboard_history.record(text.clone(), source);
                }
                crate::ClipboardData::Image(img) => {
                    engine.clipboard_history.record_image(img.clone(), source);
                }
            }
        }
    }


    fn handle_quit_requested(&mut self, event_loop: &ActiveEventLoop) {
        // If a quit modal is already open, treat as immediate quit
        let quit_modal_open = self
            .engine
            .active_modal_id
            .and_then(|id| self.windows.get(&id))
            .map(|m| {
                m.as_any()
                    .downcast_ref::<crate::window::QuitWindow>()
                    .is_some()
            })
            .unwrap_or(false);
        if quit_modal_open {
            self.close_active_modal();
            self.flush_layout_persistence_final();
            event_loop.exit();
            return;
        }

        // Get close behavior from settings
        let behavior = self
            .focused_window()
            .map(|w| w.state.engine.settings.general.close_behavior.clone())
            .or_else(|| {
                self.parked_states
                    .first()
                    .map(|s| s.engine.settings.general.close_behavior.clone())
            })
            .unwrap_or_else(|| "ask".to_string());

        match behavior.as_str() {
            "quit" => {
                self.flush_layout_persistence_final();
                event_loop.exit();
            }
            "minimize" => {
                let _ = self.engine.proxy.send_event(AppEvent::Minimize);
            }
            _ => {
                // "ask" — close any existing modal, then show quit modal
                self.close_active_modal();
                self.open_quit_modal(event_loop);
            }
        }
    }

    fn open_quit_modal(&mut self, event_loop: &ActiveEventLoop) {
        use winit::window::WindowAttributes;

        let mut attrs = WindowAttributes::default()
            .with_title("Tasty")
            .with_inner_size(winit::dpi::LogicalSize::new(400, 200))
            .with_resizable(false)
            .with_visible(false);
        if let Some(icon) = crate::app_icon::winit_window_icon() {
            attrs = attrs.with_window_icon(Some(icon));
        }

        let window = std::sync::Arc::new(
            event_loop
                .create_window(attrs)
                .expect("failed to create quit modal window"),
        );

        let gpu = pollster::block_on(crate::gpu::GpuState::new(
            window.clone(),
            &crate::settings::Settings::load().appearance,
            self.engine.proxy.clone(),
        ))
        .expect("failed to initialize GPU for quit modal");

        let window_id = window.id();
        let mut modal = crate::window::QuitWindow::new(gpu, window);
        // On Windows, hidden windows do not receive RedrawRequested events,
        // so render the first frame immediately to make the modal visible.
        // On other platforms, mark_dirty() + request_redraw() is sufficient.
        #[cfg(windows)]
        {
            use crate::window::Window as _;
            modal.render();
        }
        #[cfg(not(windows))]
        {
            use crate::window::Window as _;
            modal.mark_dirty();
        }
        self.open_modal(Box::new(modal), window_id);
    }
}

/// Encode arboard image data to PNG for clipboard history storage.
pub(crate) fn encode_clipboard_image(
    img: &arboard::ImageData<'_>,
) -> Option<crate::clipboard_history::ImageData> {
    use image::ImageEncoder;
    let w = img.width as u32;
    let h = img.height as u32;
    let mut png_buf = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new_with_quality(
        &mut png_buf,
        image::codecs::png::CompressionType::Fast,
        image::codecs::png::FilterType::Sub,
    );
    if let Err(e) = encoder.write_image(&img.bytes, w, h, image::ExtendedColorType::Rgba8) {
        tracing::warn!("Failed to encode clipboard image to PNG: {e}");
        return None;
    }
    Some(crate::clipboard_history::ImageData {
        png_bytes: png_buf,
        width: w,
        height: h,
    })
}

/// Send queued restore commands to their target terminals.
/// Commands are sent once and removed from the queue.
fn drain_restore_commands(state: &mut crate::state::AppState) {
    let commands: Vec<(u32, String)> = state.engine.pending_restore_commands.drain(..).collect();
    for (surface_id, cmd) in commands {
        if let Some(terminal) = state.find_terminal_by_id_mut(surface_id) {
            terminal.send_key(&format!("{}\r", cmd));
        }
    }
}
