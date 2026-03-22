use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, NamedKey};
use winit::window::{CursorIcon, WindowId};

use crate::model::SplitDirection;
use crate::{App, AppEvent, DividerDrag, DividerDragKind};

impl ApplicationHandler<AppEvent> for App {
    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::TerminalOutput => {
                self.mark_dirty();
            }
            AppEvent::IpcReady => {
                if self.process_ipc() {
                    self.mark_dirty();
                }
            }
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        use std::sync::Arc;
        use winit::window::WindowAttributes;

        let attrs = WindowAttributes::default()
            .with_title("Tasty")
            .with_inner_size(winit::dpi::LogicalSize::new(1280, 720));

        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .expect("failed to create window"),
        );

        // Load settings before GPU init so font_size/font_family/theme are wired.
        let init_settings = crate::settings::Settings::load();

        let gpu = pollster::block_on(crate::gpu::GpuState::new(window.clone(), &init_settings.appearance))
            .expect("failed to initialize GPU");

        let sidebar_logical_width = init_settings.appearance.sidebar_width;
        let startup_command = init_settings.general.startup_command.clone();
        drop(init_settings);

        // Compute terminal grid size from the terminal area (excluding sidebar)
        let sf = gpu.scale_factor();
        let size = gpu.size();
        let sidebar_w = sidebar_logical_width * sf;
        let terminal_rect = crate::model::Rect {
            x: sidebar_w,
            y: 0.0,
            width: (size.width as f32 - sidebar_w).max(1.0),
            height: size.height as f32,
        };
        let (cols, rows) = gpu.grid_size_for_rect(&terminal_rect);

        // Create a waker that sends AppEvent::TerminalOutput to wake the event loop.
        let proxy = self.proxy.clone();
        let waker: crate::terminal::Waker = Arc::new(move || {
            let _ = proxy.send_event(AppEvent::TerminalOutput);
        });

        let mut state = crate::state::AppState::new(cols, rows, waker).expect("failed to create app state");

        // Execute startup_command if configured
        if !startup_command.is_empty() {
            if let Some(terminal) = state.focused_terminal_mut() {
                terminal.send_key(&startup_command);
                terminal.send_bytes(b"\r");
            }
        }

        // Start IPC server (use port_file for test isolation if provided)
        // Pass an IPC waker that sends AppEvent::IpcReady to wake the event loop.
        let ipc_proxy = self.proxy.clone();
        let ipc_waker: crate::ipc::server::IpcWaker = std::sync::Arc::new(move || {
            let _ = ipc_proxy.send_event(AppEvent::IpcReady);
        });
        match crate::ipc::server::IpcServer::start_with_port_file(self.port_file.take(), Some(ipc_waker)) {
            Ok(ipc) => {
                tracing::info!("IPC server started on port {}", ipc.port());
                self.ipc_server = Some(ipc);
            }
            Err(e) => {
                tracing::warn!("Failed to start IPC server: {}", e);
            }
        }

        self.window = Some(window);
        self.gpu = Some(gpu);
        self.state = Some(state);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        // Let egui handle the event first
        let egui_consumed = if let (Some(gpu), Some(window)) = (&mut self.gpu, &self.window) {
            gpu.handle_egui_event(window, &event)
        } else {
            false
        };

        // Track whether dirty was already set before this event.
        let was_dirty = self.dirty;

        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::Resized(new_size) => {
                if let (Some(gpu), Some(state)) = (&mut self.gpu, &mut self.state) {
                    gpu.resize(new_size);

                    // Resize terminal grid to match new window (accounting for sidebar)
                    let terminal_rect = Self::compute_terminal_rect_with_sidebar(gpu, state.sidebar_width);
                    let (cols, rows) = gpu.grid_size_for_rect(&terminal_rect);
                    let cw = gpu.cell_width();
                    let ch = gpu.cell_height();
                    state.update_grid_size(cols, rows);
                    state.resize_all(terminal_rect, cw, ch);
                }
                self.mark_dirty();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                if let Some(gpu) = &mut self.gpu {
                    gpu.update_scale_factor(scale_factor as f32);
                }
                self.mark_dirty();
            }
            WindowEvent::Focused(focused) => {
                self.window_focused = focused;
                self.mark_dirty();
            }
            WindowEvent::Occluded(false) => {
                self.mark_dirty();
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed {
                    return;
                }

                // Always handle Escape to close overlays (settings, notifications)
                if event.logical_key == Key::Named(NamedKey::Escape) {
                    if let Some(state) = &mut self.state {
                        if state.settings_open {
                            state.settings_open = false;
                            state.settings_ui_state = crate::settings_ui::SettingsUiState::new();
                            self.mark_dirty();
                            return;
                        }
                        if state.notification_panel_open {
                            state.notification_panel_open = false;
                            self.mark_dirty();
                            return;
                        }
                    }
                }

                // Always handle app-level shortcuts (e.g. Ctrl+, to toggle settings)
                // even when egui has focus
                if self.handle_shortcut(&event.logical_key, self.modifiers) {
                    self.mark_dirty();
                    return;
                }

                // If egui consumed the event OR an overlay is open, don't send to terminal
                let overlay_open = self.state.as_ref()
                    .map(|s| s.settings_open || s.notification_panel_open)
                    .unwrap_or(false);
                if egui_consumed || overlay_open {
                    // egui handled the input — still need to redraw so the UI updates.
                    if egui_consumed {
                        self.mark_dirty();
                    }
                    return;
                }

                // Forward to terminal

                if let Some(state) = &mut self.state {
                    if let Some(terminal) = state.focused_terminal_mut() {
                        // event.text includes modifier transformations (e.g. Ctrl+C -> \x03)
                        if let Some(text) = &event.text {
                            let s = text.as_str();
                            if !s.is_empty() {
                                terminal.send_key(s);
                                return;
                            }
                        }
                        // Handle special keys that don't produce text
                        let app_cursor = terminal.application_cursor_keys();
                        match event.logical_key.as_ref() {
                            Key::Named(NamedKey::Enter) => terminal.send_bytes(b"\r"),
                            Key::Named(NamedKey::Backspace) => terminal.send_bytes(b"\x7f"),
                            Key::Named(NamedKey::Tab) => terminal.send_bytes(b"\t"),
                            Key::Named(NamedKey::Escape) => terminal.send_bytes(b"\x1b"),
                            Key::Named(NamedKey::ArrowUp) => {
                                if app_cursor { terminal.send_bytes(b"\x1bOA") }
                                else { terminal.send_bytes(b"\x1b[A") }
                            }
                            Key::Named(NamedKey::ArrowDown) => {
                                if app_cursor { terminal.send_bytes(b"\x1bOB") }
                                else { terminal.send_bytes(b"\x1b[B") }
                            }
                            Key::Named(NamedKey::ArrowRight) => {
                                if app_cursor { terminal.send_bytes(b"\x1bOC") }
                                else { terminal.send_bytes(b"\x1b[C") }
                            }
                            Key::Named(NamedKey::ArrowLeft) => {
                                if app_cursor { terminal.send_bytes(b"\x1bOD") }
                                else { terminal.send_bytes(b"\x1b[D") }
                            }
                            Key::Named(NamedKey::Home) => terminal.send_bytes(b"\x1b[H"),
                            Key::Named(NamedKey::End) => terminal.send_bytes(b"\x1b[F"),
                            Key::Named(NamedKey::PageUp) => terminal.send_bytes(b"\x1b[5~"),
                            Key::Named(NamedKey::PageDown) => terminal.send_bytes(b"\x1b[6~"),
                            Key::Named(NamedKey::Insert) => terminal.send_bytes(b"\x1b[2~"),
                            Key::Named(NamedKey::Delete) => terminal.send_bytes(b"\x1b[3~"),
                            Key::Named(NamedKey::F1) => terminal.send_bytes(b"\x1bOP"),
                            Key::Named(NamedKey::F2) => terminal.send_bytes(b"\x1bOQ"),
                            Key::Named(NamedKey::F3) => terminal.send_bytes(b"\x1bOR"),
                            Key::Named(NamedKey::F4) => terminal.send_bytes(b"\x1bOS"),
                            Key::Named(NamedKey::F5) => terminal.send_bytes(b"\x1b[15~"),
                            Key::Named(NamedKey::F6) => terminal.send_bytes(b"\x1b[17~"),
                            Key::Named(NamedKey::F7) => terminal.send_bytes(b"\x1b[18~"),
                            Key::Named(NamedKey::F8) => terminal.send_bytes(b"\x1b[19~"),
                            Key::Named(NamedKey::F9) => terminal.send_bytes(b"\x1b[20~"),
                            Key::Named(NamedKey::F10) => terminal.send_bytes(b"\x1b[21~"),
                            Key::Named(NamedKey::F11) => terminal.send_bytes(b"\x1b[23~"),
                            Key::Named(NamedKey::F12) => terminal.send_bytes(b"\x1b[24~"),
                            _ => {}
                        }
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_position = Some(position);
                let overlay_open = self.state.as_ref()
                    .map(|s| s.settings_open || s.notification_panel_open)
                    .unwrap_or(false);
                if egui_consumed || overlay_open {
                    // egui needs redraws for hover effects (button highlights, etc.)
                    if egui_consumed {
                        self.mark_dirty();
                    }
                    return;
                }
                if let (Some(gpu), Some(state), Some(window)) = (&self.gpu, &mut self.state, &self.window) {
                    let terminal_rect = Self::compute_terminal_rect_with_sidebar(gpu, state.sidebar_width);
                    let x = position.x as f32;
                    let y = position.y as f32;

                    if let Some(drag) = self.dragging_divider {
                        let changed = match drag.kind {
                            DividerDragKind::Pane => state.update_pane_divider(&drag.info, x, y, terminal_rect),
                            DividerDragKind::Surface => state.update_surface_divider(&drag.info, x, y, terminal_rect),
                        };
                        if changed {
                            let cw = gpu.cell_width();
                            let ch = gpu.cell_height();
                            state.resize_all(terminal_rect, cw, ch);
                            self.mark_dirty();
                        }
                    } else if !egui_consumed {
                        let threshold = 4.0;
                        let pane_divider = state.find_pane_divider_at(x, y, terminal_rect, threshold);
                        let surface_divider = state.find_surface_divider_at(x, y, terminal_rect, threshold);
                        let divider = pane_divider.or(surface_divider);
                        match divider {
                            Some(info) => {
                                let cursor = match info.direction {
                                    SplitDirection::Vertical => CursorIcon::ColResize,
                                    SplitDirection::Horizontal => CursorIcon::RowResize,
                                };
                                window.set_cursor(cursor);
                            }
                            None => {
                                window.set_cursor(CursorIcon::Default);
                            }
                        }
                    }
                }
            }
            WindowEvent::CursorLeft { .. } => {
                self.cursor_position = None;
                if let Some(window) = &self.window {
                    window.set_cursor(CursorIcon::Default);
                }
            }
            WindowEvent::MouseInput { state: button_state, button, .. } => {
                let overlay_open = self.state.as_ref()
                    .map(|s| s.settings_open || s.notification_panel_open)
                    .unwrap_or(false);
                if egui_consumed || overlay_open {
                    if button_state == ElementState::Released {
                        self.dragging_divider = None;
                    }
                    // egui handled the click — redraw so UI reflects the change.
                    if egui_consumed {
                        self.mark_dirty();
                    }
                    return;
                }
                if button == MouseButton::Left {
                    if let (Some(gpu), Some(state)) = (&self.gpu, &mut self.state) {
                        let terminal_rect = Self::compute_terminal_rect_with_sidebar(gpu, state.sidebar_width);

                        if let Some(pos) = self.cursor_position {
                            let x = pos.x as f32;
                            let y = pos.y as f32;

                            if button_state == ElementState::Pressed {
                                let threshold = 4.0;
                                let pane_divider = state.find_pane_divider_at(x, y, terminal_rect, threshold);
                                let surface_divider = state.find_surface_divider_at(x, y, terminal_rect, threshold);

                                if let Some(info) = pane_divider {
                                    self.dragging_divider = Some(DividerDrag {
                                        info,
                                        kind: DividerDragKind::Pane,
                                    });
                                } else if let Some(info) = surface_divider {
                                    self.dragging_divider = Some(DividerDrag {
                                        info,
                                        kind: DividerDragKind::Surface,
                                    });
                                } else {
                                    if state.focus_pane_at_position(x, y, terminal_rect) {
                                        // Can't call self.mark_dirty() — state is mutably borrowed.
                                        // Catch-all at end of window_event ensures request_redraw.
                                        self.dirty = true;
                                    }
                                    if state.focus_surface_at_position(x, y, terminal_rect) {
                                        self.dirty = true;
                                    }
                                }
                            } else if button_state == ElementState::Released {
                                if self.dragging_divider.is_some() {
                                    self.dragging_divider = None;
                                    let cw = gpu.cell_width();
                                    let ch = gpu.cell_height();
                                    state.resize_all(terminal_rect, cw, ch);
                                    self.dirty = true;
                                }
                            }
                        }
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let overlay_open = self.state.as_ref()
                    .map(|s| s.settings_open || s.notification_panel_open)
                    .unwrap_or(false);
                if egui_consumed {
                    self.mark_dirty();
                }
                if !egui_consumed && !overlay_open {
                    if let Some(state) = &mut self.state {
                        if let Some(terminal) = state.focused_terminal_mut() {
                            let lines = match delta {
                                MouseScrollDelta::LineDelta(_, y) => y as i32,
                                MouseScrollDelta::PixelDelta(pos) => {
                                    (pos.y / 20.0) as i32
                                }
                            };
                            if lines > 0 {
                                for _ in 0..lines.unsigned_abs() {
                                    terminal.send_bytes(b"\x1b[A");
                                }
                            } else if lines < 0 {
                                for _ in 0..lines.unsigned_abs() {
                                    terminal.send_bytes(b"\x1b[B");
                                }
                            }
                        }
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                // Process IPC commands
                self.process_ipc();

                // Process PTY output
                let changed = if let Some(state) = &mut self.state {
                    state.process_all()
                } else {
                    false
                };

                if changed {
                    self.mark_dirty();
                }

                // Collect terminal events and process notifications + hooks
                // Collect terminal events and process them.
                // We borrow self.state mutably here, so we can't call mark_dirty() —
                // use self.dirty = true instead. The catch-all at the end of
                // window_event() ensures request_redraw() is called.
                if let Some(state) = &mut self.state {
                    let events = state.collect_events();
                    for event in &events {
                        let surface_id = event.surface_id;
                        match &event.kind {
                            crate::terminal::TerminalEventKind::Notification { title, body } => {
                                if state.settings.notification.enabled
                                    && state.settings.notification.system_notification
                                    && !self.window_focused
                                    && state.notifications.should_send_system_notification()
                                {
                                    crate::notification::send_system_notification(title, body);
                                }
                                if state.settings.notification.enabled {
                                    let ws_id = state.active_workspace().id;
                                    state.notifications.add(
                                        ws_id, surface_id, title.clone(), body.clone(),
                                    );
                                }
                                let hook_events = vec![tasty_hooks::HookEvent::Notification];
                                state.hook_manager.check_and_fire(surface_id, &hook_events);
                                self.dirty = true;
                            }
                            crate::terminal::TerminalEventKind::BellRing => {
                                if state.settings.notification.enabled {
                                    let ws_id = state.active_workspace().id;
                                    state.notifications.add(
                                        ws_id, surface_id, "Bell".to_string(), String::new(),
                                    );
                                }
                                if state.settings.notification.enabled
                                    && state.settings.notification.system_notification
                                    && !self.window_focused
                                    && state.notifications.should_send_system_notification()
                                {
                                    crate::notification::send_system_notification("Tasty", "Bell");
                                }
                                let hook_events = vec![tasty_hooks::HookEvent::Bell];
                                state.hook_manager.check_and_fire(surface_id, &hook_events);
                                self.dirty = true;
                            }
                            crate::terminal::TerminalEventKind::TitleChanged(_) => {
                                self.dirty = true;
                            }
                            crate::terminal::TerminalEventKind::CwdChanged(_) => {
                                self.dirty = true;
                            }
                            crate::terminal::TerminalEventKind::ClipboardSet(data) => {
                                if let Some(cb) = &mut self.clipboard {
                                    cb.set_text(data);
                                }
                            }
                            crate::terminal::TerminalEventKind::ProcessExited => {
                                let hook_events = vec![tasty_hooks::HookEvent::ProcessExit];
                                state.hook_manager.check_and_fire(surface_id, &hook_events);
                                self.dirty = true;
                            }
                        }
                    }
                }

                if self.dirty {
                    self.dirty = false;
                    if let (Some(gpu), Some(state), Some(window)) =
                        (&mut self.gpu, &mut self.state, &self.window)
                    {
                        match gpu.render(state, window) {
                            Ok(_) => {}
                            Err(wgpu::SurfaceError::Lost) => {
                                gpu.resize(window.inner_size());
                            }
                            Err(wgpu::SurfaceError::OutOfMemory) => {
                                tracing::error!("GPU out of memory");
                                event_loop.exit();
                            }
                            Err(e) => {
                                tracing::warn!("surface error: {e}");
                            }
                        }
                    }
                }

                if self.dirty {
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                }
            }
            _ => {}
        }

        // If any non-RedrawRequested event made us dirty, request a redraw.
        if self.dirty && !was_dirty {
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        // Process IPC commands outside of RedrawRequested so they respond
        // even when the window is idle (no redraws happening).
        if self.process_ipc() {
            self.mark_dirty();
        }
    }
}
