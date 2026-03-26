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
            AppEvent::EguiRepaint => {
                self.mark_dirty();
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

        let gpu = pollster::block_on(crate::gpu::GpuState::new(window.clone(), &init_settings.appearance, self.proxy.clone()))
            .expect("failed to initialize GPU");

        // If configured shell is invalid, try auto-detect before showing setup dialog
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
                self.window = Some(window);
                self.gpu = Some(gpu);
                return;
            }
        }

        self.init_app_state(window, gpu, init_settings);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        // Let egui handle the event first
        let (egui_consumed, egui_repaint) = if let (Some(gpu), Some(window)) = (&mut self.gpu, &self.window) {
            gpu.handle_egui_event(window, &event)
        } else {
            (false, false)
        };
        if egui_repaint {
            self.mark_dirty();
        }

        // Track whether dirty was already set before this event.
        let was_dirty = self.dirty;

        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::Resized(new_size) => {
                if let Some(gpu) = &mut self.gpu {
                    gpu.resize(new_size);
                }
                if let (Some(gpu), Some(state)) = (&self.gpu, &mut self.state) {
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
                if !focused {
                    // Reset modifier state to prevent stuck keys after Alt+Tab etc.
                    self.modifiers = winit::keyboard::ModifiersState::empty();
                }
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

                // If an overlay is open, skip shortcuts so key events
                // stay within the overlay (e.g. keybinding capture).
                let overlay_open = self.state.as_ref()
                    .map(|s| s.settings_open || s.notification_panel_open)
                    .unwrap_or(false);

                if !overlay_open {
                    if self.handle_shortcut(&event.logical_key, self.modifiers) {
                        self.mark_dirty();
                        return;
                    }
                }
                if egui_consumed || overlay_open {
                    // egui handled the input — still need to redraw so the UI updates.
                    if egui_consumed {
                        self.mark_dirty();
                    }
                    return;
                }

                // Forward to terminal

                if let Some(state) = &mut self.state {
                    // Record typing before borrowing terminal mutably
                    let typing_surface_id = state.focused_surface_id();
                    if let Some(terminal) = state.focused_terminal_mut() {
                        // Handle special keys FIRST — these have well-defined byte
                        // sequences and must not be intercepted by event.text,
                        // which can contain unexpected control characters
                        // (e.g. Backspace → \x7f or \x08 depending on platform).
                        let app_cursor = terminal.application_cursor_keys();
                        let is_alt_screen = terminal.is_alternate_screen();

                        // Check if this key will be used for scrollback (not sent to PTY)
                        let is_scrollback_key = !is_alt_screen && matches!(
                            event.logical_key.as_ref(),
                            Key::Named(NamedKey::PageUp) | Key::Named(NamedKey::PageDown)
                        );

                        // Any keyboard input that goes to PTY resets scroll to bottom
                        if !is_scrollback_key && terminal.scroll_offset > 0 {
                            terminal.scroll_to_bottom();
                            self.dirty = true;
                        }

                        match event.logical_key.as_ref() {
                            Key::Named(NamedKey::Enter) => terminal.send_bytes(b"\r"),
                            Key::Named(NamedKey::Backspace) => terminal.send_bytes(b"\x7f"),
                            Key::Named(NamedKey::Tab) => {
                                if self.modifiers.shift_key() {
                                    terminal.send_bytes(b"\x1b[Z"); // Reverse Tab (CSI Z)
                                } else {
                                    terminal.send_bytes(b"\t");
                                }
                            }
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
                            Key::Named(NamedKey::PageUp) => {
                                if terminal.is_alternate_screen() {
                                    terminal.send_bytes(b"\x1b[5~");
                                } else {
                                    terminal.scroll_up(terminal.rows());
                                    self.dirty = true;
                                }
                            }
                            Key::Named(NamedKey::PageDown) => {
                                if terminal.is_alternate_screen() {
                                    terminal.send_bytes(b"\x1b[6~");
                                } else {
                                    terminal.scroll_down(terminal.rows());
                                    self.dirty = true;
                                }
                            }
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
                            _ => {
                                // Not a special key — use event.text for regular characters
                                // (includes Ctrl+key mappings like Ctrl+C → \x03).
                                if let Some(text) = &event.text {
                                    let s = text.as_str();
                                    if !s.is_empty() {
                                        terminal.send_key(s);
                                    }
                                }
                            }
                        }
                    }
                    // After terminal borrow ends, record typing for the surface
                    if let Some(sid) = typing_surface_id {
                        state.record_typing(sid);
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
                            if terminal.is_alternate_screen() {
                                // In alternate screen (vim, less, etc.) - send to PTY
                                if lines > 0 {
                                    for _ in 0..lines.unsigned_abs() {
                                        terminal.send_bytes(b"\x1b[A");
                                    }
                                } else if lines < 0 {
                                    for _ in 0..lines.unsigned_abs() {
                                        terminal.send_bytes(b"\x1b[B");
                                    }
                                }
                            } else {
                                // Normal mode - scroll the scrollback buffer
                                if lines > 0 {
                                    terminal.scroll_up(lines as usize);
                                } else if lines < 0 {
                                    terminal.scroll_down((-lines) as usize);
                                }
                                self.dirty = true;
                            }
                        }
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                // Shell setup mode: render only the setup dialog
                if self.shell_setup_mode {
                    if let (Some(gpu), Some(window)) = (&mut self.gpu, &self.window) {
                        let result = gpu.render_shell_setup(
                            window,
                            &mut self.shell_setup_path,
                        );
                        match result {
                            Ok(crate::gpu::ShellSetupAction::None) => {}
                            Ok(crate::gpu::ShellSetupAction::Confirmed) => {
                                // Save the shell path and initialize app
                                let mut settings = crate::settings::Settings::load();
                                settings.general.shell = self.shell_setup_path.clone();
                                if let Err(e) = settings.save() {
                                    tracing::error!("failed to save settings: {e}");
                                }
                                self.shell_setup_mode = false;
                                let window = self.window.take().unwrap();
                                let gpu = self.gpu.take().unwrap();
                                self.init_app_state(window, gpu, settings);
                                self.mark_dirty();
                            }
                            Ok(crate::gpu::ShellSetupAction::Exit) => {
                                event_loop.exit();
                            }
                            Err(e) => {
                                tracing::warn!("shell setup render error: {e}");
                            }
                        }
                    }
                    return;
                }

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
                            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
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
