mod cli;
mod font;
mod gpu;
mod hooks;
mod ipc;
mod model;
mod notification;
mod renderer;
mod settings;
mod settings_ui;
mod state;
mod terminal;
mod ui;

use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use tracing_subscriber::EnvFilter;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{CursorIcon, Window, WindowAttributes, WindowId};

use gpu::GpuState;
use ipc::server::IpcServer;
use model::{DividerInfo, Rect, SplitDirection};
use state::AppState;

/// Wrapper for the system clipboard (arboard).
struct ClipboardContext {
    inner: arboard::Clipboard,
}

impl ClipboardContext {
    fn new() -> Option<Self> {
        arboard::Clipboard::new().ok().map(|c| Self { inner: c })
    }

    fn get_text(&mut self) -> Option<String> {
        self.inner.get_text().ok()
    }

    fn set_text(&mut self, text: &str) {
        let _ = self.inner.set_text(text.to_string());
    }
}

/// Custom events sent to the winit event loop from background threads.
#[derive(Debug)]
enum AppEvent {
    /// PTY reader thread produced output -- wake up and redraw.
    TerminalOutput,
}

/// Tracks an active divider drag operation.
#[derive(Clone, Copy)]
enum DividerDragKind {
    /// Dragging a pane-level split divider.
    Pane,
    /// Dragging a surface-level split divider (within a SurfaceGroup).
    Surface,
}

#[derive(Clone, Copy)]
struct DividerDrag {
    info: DividerInfo,
    kind: DividerDragKind,
}

struct App {
    // gpu must drop before window so the wgpu surface is released first
    gpu: Option<GpuState>,
    state: Option<AppState>,
    window: Option<Arc<Window>>,
    dirty: bool,
    modifiers: ModifiersState,
    /// Whether the window currently has OS focus.
    window_focused: bool,
    /// IPC server for CLI communication.
    ipc_server: Option<IpcServer>,
    /// Current cursor position in physical pixels.
    cursor_position: Option<winit::dpi::PhysicalPosition<f64>>,
    /// Active divider drag state.
    dragging_divider: Option<DividerDrag>,
    /// Proxy to send events from background threads to the winit event loop.
    proxy: EventLoopProxy<AppEvent>,
    /// System clipboard for copy/paste.
    clipboard: Option<ClipboardContext>,
}

impl App {
    fn new(proxy: EventLoopProxy<AppEvent>) -> Self {
        Self {
            gpu: None,
            state: None,
            window: None,
            dirty: true,
            modifiers: ModifiersState::empty(),
            window_focused: true,
            ipc_server: None,
            cursor_position: None,
            dragging_divider: None,
            proxy,
            clipboard: ClipboardContext::new(),
        }
    }

    /// Compute the terminal rect without borrowing self (takes gpu ref directly).
    fn compute_terminal_rect_with_sidebar(gpu: &GpuState, sidebar_logical_width: f32) -> Rect {
        let size = gpu.size();
        let sf = gpu.scale_factor();
        let sidebar_w = sidebar_logical_width * sf;
        Rect {
            x: sidebar_w,
            y: 0.0,
            width: (size.width as f32 - sidebar_w).max(1.0),
            height: size.height as f32,
        }
    }

    /// Paste clipboard text into the focused terminal.
    fn paste_to_terminal(&mut self) {
        let text = match &mut self.clipboard {
            Some(cb) => cb.get_text(),
            None => None,
        };
        if let Some(text) = text {
            if text.is_empty() {
                return;
            }
            if let Some(state) = &mut self.state {
                if let Some(terminal) = state.focused_terminal_mut() {
                    if terminal.bracketed_paste() {
                        terminal.send_bytes(b"\x1b[200~");
                        terminal.send_key(&text);
                        terminal.send_bytes(b"\x1b[201~");
                    } else {
                        terminal.send_key(&text);
                    }
                }
            }
        }
    }

    /// Handle keyboard shortcuts. Returns true if the event was consumed by a shortcut.
    fn handle_shortcut(&mut self, key: &Key, mods: ModifiersState) -> bool {
        let ctrl = mods.control_key();
        let shift = mods.shift_key();
        let alt = mods.alt_key();

        let state = match &mut self.state {
            Some(s) => s,
            None => return false,
        };

        // Ctrl+Shift combinations
        if ctrl && shift {
            if let Key::Character(c) = key {
                match c.as_str() {
                    // Ctrl+Shift+W: Close active pane (unsplit)
                    "W" | "w" => {
                        if state.close_active_pane() {
                            if let Some(gpu) = &self.gpu {
                                let rect = Self::compute_terminal_rect_with_sidebar(gpu, state.sidebar_width);
                                state.resize_all(rect, gpu.cell_width(), gpu.cell_height());
                            }
                            self.dirty = true;
                            return true;
                        }
                        return false;
                    }
                    // Ctrl+Shift+N: New workspace
                    "N" | "n" => {
                        let _ = state.add_workspace();
                        self.dirty = true;
                        return true;
                    }
                    // Ctrl+Shift+T: New tab in focused pane
                    "T" | "t" => {
                        let _ = state.add_tab();
                        self.dirty = true;
                        return true;
                    }
                    // Ctrl+Shift+E: Pane split vertical (new independent tab bar)
                    "E" | "e" => {
                        let _ = state.split_pane(SplitDirection::Vertical);
                        if let Some(gpu) = &self.gpu {
                            let rect = Self::compute_terminal_rect_with_sidebar(gpu, state.sidebar_width);
                            state.resize_all(rect, gpu.cell_width(), gpu.cell_height());
                        }
                        self.dirty = true;
                        return true;
                    }
                    // Ctrl+Shift+O: Pane split horizontal (new independent tab bar)
                    "O" | "o" => {
                        let _ = state.split_pane(SplitDirection::Horizontal);
                        if let Some(gpu) = &self.gpu {
                            let rect = Self::compute_terminal_rect_with_sidebar(gpu, state.sidebar_width);
                            state.resize_all(rect, gpu.cell_width(), gpu.cell_height());
                        }
                        self.dirty = true;
                        return true;
                    }
                    // Ctrl+Shift+D: SurfaceGroup split vertical (within current tab)
                    "D" | "d" => {
                        let _ = state.split_surface(SplitDirection::Vertical);
                        if let Some(gpu) = &self.gpu {
                            let rect = Self::compute_terminal_rect_with_sidebar(gpu, state.sidebar_width);
                            state.resize_all(rect, gpu.cell_width(), gpu.cell_height());
                        }
                        self.dirty = true;
                        return true;
                    }
                    // Ctrl+Shift+J: SurfaceGroup split horizontal (within current tab)
                    "J" | "j" => {
                        let _ = state.split_surface(SplitDirection::Horizontal);
                        if let Some(gpu) = &self.gpu {
                            let rect = Self::compute_terminal_rect_with_sidebar(gpu, state.sidebar_width);
                            state.resize_all(rect, gpu.cell_width(), gpu.cell_height());
                        }
                        self.dirty = true;
                        return true;
                    }
                    // Ctrl+Shift+I: Toggle notification panel
                    "I" | "i" => {
                        state.notification_panel_open = !state.notification_panel_open;
                        // Mark all as read when opening
                        if state.notification_panel_open {
                            state.notifications.mark_all_read();
                        }
                        self.dirty = true;
                        return true;
                    }
                    _ => {}
                }
            }

            // Ctrl+Shift+Tab: previous tab in focused pane
            if let Key::Named(NamedKey::Tab) = key {
                state.prev_tab_in_pane();
                self.dirty = true;
                return true;
            }
        }

        // Ctrl+W: Close active tab (if >1 tabs)
        if ctrl && !shift && !alt {
            if let Key::Character(c) = key {
                let s = c.as_str();
                if s == "w" || s == "W" || s == "\u{17}" {
                    if state.close_active_tab() {
                        self.dirty = true;
                        return true;
                    }
                    return false;
                }
            }
        }

        // Ctrl+,: Toggle settings window
        if ctrl && !shift && !alt {
            if let Key::Character(c) = key {
                if c.as_str() == "," {
                    state.settings_open = !state.settings_open;
                    self.dirty = true;
                    return true;
                }
            }
        }

        // Ctrl+Tab: next tab in focused pane
        if ctrl && !shift && !alt {
            if let Key::Named(NamedKey::Tab) = key {
                state.next_tab_in_pane();
                self.dirty = true;
                return true;
            }
        }

        // Clipboard paste shortcuts
        // Ctrl+Shift+V (Linux style)
        if ctrl && shift {
            if let Key::Character(c) = key {
                if (c.as_str() == "V" || c.as_str() == "v")
                    && state.settings.clipboard.linux_style
                {
                    self.paste_to_terminal();
                    self.dirty = true;
                    return true;
                }
            }
        }
        // Ctrl+V (Windows style) — only when no text selection exists
        if ctrl && !shift && !alt {
            if let Key::Character(c) = key {
                if (c.as_str() == "v" || c.as_str() == "V" || c.as_str() == "\u{16}")
                    && state.settings.clipboard.windows_style
                {
                    self.paste_to_terminal();
                    self.dirty = true;
                    return true;
                }
            }
        }
        // Alt+V (macOS style)
        if alt && !ctrl && !shift {
            if let Key::Character(c) = key {
                if (c.as_str() == "v" || c.as_str() == "V")
                    && state.settings.clipboard.macos_style
                {
                    self.paste_to_terminal();
                    self.dirty = true;
                    return true;
                }
            }
        }

        // Alt+1~9: switch workspace
        if alt && !ctrl && !shift {
            if let Key::Character(c) = key {
                if let Some(digit) = c.chars().next().and_then(|ch| ch.to_digit(10)) {
                    if digit >= 1 && digit <= 9 {
                        state.switch_workspace((digit - 1) as usize);
                        self.dirty = true;
                        return true;
                    }
                }
            }
        }

        // Alt+Arrow: move focus between panes
        if alt && !ctrl && !shift {
            match key {
                Key::Named(NamedKey::ArrowRight) | Key::Named(NamedKey::ArrowDown) => {
                    state.move_focus_next_pane();
                    self.dirty = true;
                    return true;
                }
                Key::Named(NamedKey::ArrowLeft) | Key::Named(NamedKey::ArrowUp) => {
                    state.move_focus_prev_pane();
                    self.dirty = true;
                    return true;
                }
                _ => {}
            }
        }

        false
    }

    /// Process pending IPC commands. Returns true if any commands were processed.
    fn process_ipc(&mut self) -> bool {
        let ipc = match &self.ipc_server {
            Some(ipc) => ipc,
            None => return false,
        };
        let state = match &mut self.state {
            Some(s) => s,
            None => return false,
        };

        let mut processed = false;
        while let Ok(cmd) = ipc.try_recv() {
            let response = ipc::handler::handle(state, &cmd.request);
            let _ = cmd.response_tx.send(response);
            self.dirty = true;
            processed = true;
        }
        processed
    }
}

impl ApplicationHandler<AppEvent> for App {
    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::TerminalOutput => {
                self.dirty = true;
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

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

        let gpu = pollster::block_on(GpuState::new(window.clone(), &init_settings.appearance))
            .expect("failed to initialize GPU");

        let sidebar_logical_width = init_settings.appearance.sidebar_width;
        let startup_command = init_settings.general.startup_command.clone();
        drop(init_settings);

        // Compute terminal grid size from the terminal area (excluding sidebar)
        let sf = gpu.scale_factor();
        let size = gpu.size();
        let sidebar_w = sidebar_logical_width * sf;
        let terminal_rect = Rect {
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

        let mut state = AppState::new(cols, rows, waker).expect("failed to create app state");

        // Execute startup_command if configured
        if !startup_command.is_empty() {
            if let Some(terminal) = state.focused_terminal_mut() {
                terminal.send_key(&startup_command);
                terminal.send_bytes(b"\r");
            }
        }

        // Start IPC server
        match IpcServer::start() {
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
                self.dirty = true;
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                if let Some(gpu) = &mut self.gpu {
                    gpu.update_scale_factor(scale_factor as f32);
                }
                self.dirty = true;
            }
            WindowEvent::Focused(focused) => {
                self.window_focused = focused;
                self.dirty = true;
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::Occluded(false) => {
                self.dirty = true;
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
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
                            self.dirty = true;
                            return;
                        }
                        if state.notification_panel_open {
                            state.notification_panel_open = false;
                            self.dirty = true;
                            return;
                        }
                    }
                }

                // Always handle app-level shortcuts (e.g. Ctrl+, to toggle settings)
                // even when egui has focus
                if self.handle_shortcut(&event.logical_key, self.modifiers) {
                    self.dirty = true;
                    return;
                }

                // If egui consumed the event OR an overlay is open, don't send to terminal
                let overlay_open = self.state.as_ref()
                    .map(|s| s.settings_open || s.notification_panel_open)
                    .unwrap_or(false);
                if egui_consumed || overlay_open {
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
                    // Don't do terminal area mouse handling when overlay is open
                    return;
                }
                if let (Some(gpu), Some(state), Some(window)) = (&self.gpu, &mut self.state, &self.window) {
                    let terminal_rect = Self::compute_terminal_rect_with_sidebar(gpu, state.sidebar_width);
                    let x = position.x as f32;
                    let y = position.y as f32;

                    if let Some(drag) = self.dragging_divider {
                        // Dragging a divider -- update ratio
                        let changed = match drag.kind {
                            DividerDragKind::Pane => state.update_pane_divider(&drag.info, x, y, terminal_rect),
                            DividerDragKind::Surface => state.update_surface_divider(&drag.info, x, y, terminal_rect),
                        };
                        if changed {
                            let cw = gpu.cell_width();
                            let ch = gpu.cell_height();
                            state.resize_all(terminal_rect, cw, ch);
                            self.dirty = true;
                        }
                    } else if !egui_consumed {
                        // Not dragging -- check for divider hover to set cursor icon
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
                    // egui handled the click or overlay is open
                    if button_state == ElementState::Released {
                        self.dragging_divider = None;
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
                                // Check if clicking on a divider to start drag
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
                                    // Click to focus pane
                                    if state.focus_pane_at_position(x, y, terminal_rect) {
                                        self.dirty = true;
                                    }
                                    // Click to focus surface within SurfaceGroup
                                    if state.focus_surface_at_position(x, y, terminal_rect) {
                                        self.dirty = true;
                                    }
                                }
                            } else if button_state == ElementState::Released {
                                if self.dragging_divider.is_some() {
                                    self.dragging_divider = None;
                                    // Resize terminals after drag ends
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
                if !egui_consumed && !overlay_open {
                    if let Some(state) = &mut self.state {
                        if let Some(terminal) = state.focused_terminal_mut() {
                            let lines = match delta {
                                MouseScrollDelta::LineDelta(_, y) => y as i32,
                                MouseScrollDelta::PixelDelta(pos) => {
                                    // Convert pixel delta to approximate line count
                                    (pos.y / 20.0) as i32
                                }
                            };
                            // Send scroll sequences to the terminal
                            // Scroll up = arrow up sequences, scroll down = arrow down
                            if lines > 0 {
                                // Scroll up
                                for _ in 0..lines.unsigned_abs() {
                                    terminal.send_bytes(b"\x1b[A");
                                }
                            } else if lines < 0 {
                                // Scroll down
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
                    self.dirty = true;
                }

                // Collect terminal events and process notifications + hooks
                if let Some(state) = &mut self.state {
                    let events = state.collect_events();
                    for event in &events {
                        let surface_id = event.surface_id;
                        match &event.kind {
                            terminal::TerminalEventKind::Notification { title, body } => {
                                if state.settings.notification.enabled
                                    && state.settings.notification.system_notification
                                    && !self.window_focused
                                    && state.notifications.should_send_system_notification()
                                {
                                    notification::send_system_notification(title, body);
                                }
                                if state.settings.notification.enabled {
                                    let ws_id = state.active_workspace().id;
                                    state.notifications.add(
                                        ws_id,
                                        surface_id,
                                        title.clone(),
                                        body.clone(),
                                    );
                                }
                                // Fire Notification hooks on the source surface
                                let hook_events = vec![hooks::HookEvent::Notification];
                                state.hook_manager.check_and_fire(surface_id, &hook_events);
                                self.dirty = true;
                            }
                            terminal::TerminalEventKind::BellRing => {
                                if state.settings.notification.enabled {
                                    let ws_id = state.active_workspace().id;
                                    state.notifications.add(
                                        ws_id,
                                        surface_id,
                                        "Bell".to_string(),
                                        String::new(),
                                    );
                                }
                                if state.settings.notification.enabled
                                    && state.settings.notification.system_notification
                                    && !self.window_focused
                                    && state.notifications.should_send_system_notification()
                                {
                                    notification::send_system_notification("Tasty", "Bell");
                                }
                                // Fire Bell hooks on the source surface
                                let hook_events = vec![hooks::HookEvent::Bell];
                                state.hook_manager.check_and_fire(surface_id, &hook_events);
                                self.dirty = true;
                            }
                            terminal::TerminalEventKind::TitleChanged(_title) => {
                                // Could update tab names here in the future
                                self.dirty = true;
                            }
                            terminal::TerminalEventKind::CwdChanged(_path) => {
                                // Could update sidebar metadata here in the future
                                self.dirty = true;
                            }
                            terminal::TerminalEventKind::ClipboardSet(data) => {
                                // Terminal requested clipboard set via OSC 52
                                if let Some(cb) = &mut self.clipboard {
                                    cb.set_text(data);
                                }
                            }
                            terminal::TerminalEventKind::ProcessExited => {
                                // Fire ProcessExit hooks on the source surface
                                let hook_events = vec![hooks::HookEvent::ProcessExit];
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

                // If events processed during this frame dirtied us again, request
                // another frame so those changes are rendered promptly.
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
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("TASTY_LOG")
                .unwrap_or_else(|_| EnvFilter::new("warn,wgpu_hal=error,wgpu_core=error,naga=error")),
        )
        .init();

    // Parse CLI arguments
    let cli = cli::Cli::parse();

    // If a subcommand was provided, run in CLI client mode
    if let Some(command) = cli.command {
        return cli::run_client(command);
    }

    // Otherwise, run the GUI
    let event_loop = EventLoop::<AppEvent>::with_user_event().build()?;
    let proxy = event_loop.create_proxy();
    let mut app = App::new(proxy);
    event_loop.run_app(&mut app)?;

    Ok(())
}
