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
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Window, WindowAttributes, WindowId};

use gpu::GpuState;
use ipc::server::IpcServer;
use model::{Rect, SplitDirection};
use state::AppState;

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
}

impl App {
    fn new() -> Self {
        Self {
            gpu: None,
            state: None,
            window: None,
            dirty: true,
            modifiers: ModifiersState::empty(),
            window_focused: true,
            ipc_server: None,
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

    /// Process pending IPC commands.
    fn process_ipc(&mut self) {
        let ipc = match &self.ipc_server {
            Some(ipc) => ipc,
            None => return,
        };
        let state = match &mut self.state {
            Some(s) => s,
            None => return,
        };

        while let Ok(cmd) = ipc.try_recv() {
            let response = ipc::handler::handle(state, &cmd.request);
            let _ = cmd.response_tx.send(response);
            self.dirty = true;
        }
    }
}

impl ApplicationHandler for App {
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

        let gpu = pollster::block_on(GpuState::new(window.clone()))
            .expect("failed to initialize GPU");

        // We need settings to determine sidebar width, but state isn't created yet.
        // Load settings temporarily to get sidebar width for initial grid calculation.
        let init_settings = crate::settings::Settings::load();
        let sidebar_logical_width = init_settings.appearance.sidebar_width;
        let font_size = init_settings.appearance.font_size;
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
        let state = AppState::new(cols, rows).expect("failed to create app state");
        let _ = font_size; // font_size wired via CellRenderer::new in GpuState::new

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

                // If egui consumed the event (e.g. typing in text field), don't send to terminal
                if egui_consumed {
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
                        match event.logical_key.as_ref() {
                            Key::Named(NamedKey::Enter) => terminal.send_bytes(b"\r"),
                            Key::Named(NamedKey::Backspace) => terminal.send_bytes(b"\x7f"),
                            Key::Named(NamedKey::Tab) => terminal.send_bytes(b"\t"),
                            Key::Named(NamedKey::Escape) => terminal.send_bytes(b"\x1b"),
                            Key::Named(NamedKey::ArrowUp) => terminal.send_bytes(b"\x1b[A"),
                            Key::Named(NamedKey::ArrowDown) => terminal.send_bytes(b"\x1b[B"),
                            Key::Named(NamedKey::ArrowRight) => terminal.send_bytes(b"\x1b[C"),
                            Key::Named(NamedKey::ArrowLeft) => terminal.send_bytes(b"\x1b[D"),
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

                // Request next frame. VSync (PresentMode::Fifo) limits to monitor refresh
                // rate (~60fps) so this is not a true busy-loop CPU-wise. The GPU work only
                // happens when self.dirty is true. For true event-driven redraw we would need
                // EventLoopProxy to wake the main thread from the PTY reader, which is a
                // larger refactor.
                // TODO: use EventLoopProxy for event-driven redraw instead of continuous request_redraw
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            _ => {}
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
    let event_loop = EventLoop::new()?;
    let mut app = App::new();
    event_loop.run_app(&mut app)?;

    Ok(())
}
