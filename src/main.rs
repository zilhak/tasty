mod font;
mod gpu;
mod model;
mod renderer;
mod state;
mod terminal;
mod ui;

use std::sync::Arc;

use anyhow::Result;
use tracing_subscriber::EnvFilter;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Window, WindowAttributes, WindowId};

use gpu::GpuState;
use model::{Rect, SplitDirection};
use state::AppState;

struct App {
    // gpu must drop before window so the wgpu surface is released first
    gpu: Option<GpuState>,
    state: Option<AppState>,
    window: Option<Arc<Window>>,
    dirty: bool,
    modifiers: ModifiersState,
}

impl App {
    fn new() -> Self {
        Self {
            gpu: None,
            state: None,
            window: None,
            dirty: true,
            modifiers: ModifiersState::empty(),
        }
    }


    /// Compute the terminal rect without borrowing self (takes gpu ref directly).
    fn compute_terminal_rect(gpu: &GpuState) -> Rect {
        let size = gpu.size();
        let sf = gpu.scale_factor();
        let sidebar_w = 180.0 * sf;
        let tab_h = 32.0 * sf;
        Rect {
            x: sidebar_w,
            y: tab_h,
            width: (size.width as f32 - sidebar_w).max(1.0),
            height: (size.height as f32 - tab_h).max(1.0),
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
                    "N" | "n" => {
                        let _ = state.add_workspace();
                        self.dirty = true;
                        return true;
                    }
                    "T" | "t" => {
                        let _ = state.add_pane();
                        self.dirty = true;
                        return true;
                    }
                    "E" | "e" => {
                        let _ = state.split_focused(SplitDirection::Vertical);
                        if let Some(gpu) = &self.gpu {
                            let rect = Self::compute_terminal_rect(gpu);
                            state.resize_active_pane(rect, gpu.cell_width(), gpu.cell_height());
                        }
                        self.dirty = true;
                        return true;
                    }
                    "O" | "o" => {
                        let _ = state.split_focused(SplitDirection::Horizontal);
                        if let Some(gpu) = &self.gpu {
                            let rect = Self::compute_terminal_rect(gpu);
                            state.resize_active_pane(rect, gpu.cell_width(), gpu.cell_height());
                        }
                        self.dirty = true;
                        return true;
                    }
                    _ => {}
                }
            }
        }

        // Ctrl+Tab: switch pane
        if ctrl && !shift && !alt {
            if let Key::Named(NamedKey::Tab) = key {
                state.next_pane();
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

        // Alt+Arrow: focus between splits
        if alt && !ctrl && !shift {
            match key {
                Key::Named(NamedKey::ArrowRight) | Key::Named(NamedKey::ArrowDown) => {
                    state.move_focus_forward();
                    self.dirty = true;
                    return true;
                }
                Key::Named(NamedKey::ArrowLeft) | Key::Named(NamedKey::ArrowUp) => {
                    state.move_focus_backward();
                    self.dirty = true;
                    return true;
                }
                _ => {}
            }
        }

        false
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

        // Compute terminal grid size from the terminal area (excluding sidebar + tab bar)
        let sf = gpu.scale_factor();
        let size = gpu.size();
        let sidebar_w = 180.0 * sf;
        let tab_h = 32.0 * sf;
        let terminal_rect = Rect {
            x: sidebar_w,
            y: tab_h,
            width: (size.width as f32 - sidebar_w).max(1.0),
            height: (size.height as f32 - tab_h).max(1.0),
        };
        let (cols, rows) = gpu.grid_size_for_rect(&terminal_rect);
        let state = AppState::new(cols, rows).expect("failed to create app state");

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
                if let Some(gpu) = &mut self.gpu {
                    gpu.resize(new_size);

                    // Resize terminal grid to match new window (accounting for UI panels)
                    let terminal_rect = Self::compute_terminal_rect(gpu);
                    let (cols, rows) = gpu.grid_size_for_rect(&terminal_rect);
                    let cw = gpu.cell_width();
                    let ch = gpu.cell_height();
                    if let Some(state) = &mut self.state {
                        state.update_grid_size(cols, rows);
                        state.resize_active_pane(terminal_rect, cw, ch);
                    }
                }
                self.dirty = true;
            }
            WindowEvent::Focused(true) | WindowEvent::Occluded(false) => {
                self.dirty = true;
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if egui_consumed {
                    return;
                }

                if event.state != ElementState::Pressed {
                    return;
                }

                // Check shortcuts first
                if self.handle_shortcut(&event.logical_key, self.modifiers) {
                    return;
                }

                if let Some(state) = &mut self.state {
                    let terminal = state.focused_terminal_mut();
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
            WindowEvent::RedrawRequested => {
                // Process PTY output
                let changed = if let Some(state) = &mut self.state {
                    state.process_all()
                } else {
                    false
                };

                if changed {
                    self.dirty = true;
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

    let event_loop = EventLoop::new()?;
    let mut app = App::new();
    event_loop.run_app(&mut app)?;

    Ok(())
}
