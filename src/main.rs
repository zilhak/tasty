mod font;
mod gpu;
mod renderer;
mod terminal;

use std::sync::Arc;

use anyhow::Result;
use tracing_subscriber::EnvFilter;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowAttributes, WindowId};

use gpu::GpuState;
use terminal::Terminal;

struct App {
    // gpu must drop before window so the wgpu surface is released first
    gpu: Option<GpuState>,
    terminal: Option<Terminal>,
    window: Option<Arc<Window>>,
    dirty: bool,
}

impl App {
    fn new() -> Self {
        Self {
            gpu: None,
            terminal: None,
            window: None,
            dirty: true,
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

        // Compute terminal grid size from window dimensions
        let (cols, rows) = gpu.grid_size();
        let terminal = Terminal::new(cols, rows).expect("failed to create terminal");

        self.window = Some(window);
        self.gpu = Some(gpu);
        self.terminal = Some(terminal);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::Resized(new_size) => {
                if let Some(gpu) = &mut self.gpu {
                    gpu.resize(new_size);

                    // Resize terminal grid to match new window
                    let (cols, rows) = gpu.grid_size();
                    if let Some(terminal) = &mut self.terminal {
                        terminal.resize(cols, rows);
                    }
                }
                self.dirty = true;
            }
            WindowEvent::Focused(true) | WindowEvent::Occluded(false) => {
                // Resume render loop after system menu or other modal interruption
                self.dirty = true;
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed {
                    return;
                }
                if let Some(terminal) = &mut self.terminal {
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
                let changed = if let Some(terminal) = &mut self.terminal {
                    terminal.process()
                } else {
                    false
                };

                if changed {
                    self.dirty = true;
                }

                if self.dirty {
                    self.dirty = false;
                    if let (Some(gpu), Some(terminal)) = (&mut self.gpu, &self.terminal) {
                        match gpu.render(terminal.surface()) {
                            Ok(_) => {}
                            Err(wgpu::SurfaceError::Lost) => {
                                if let Some(window) = &self.window {
                                    gpu.resize(window.inner_size());
                                }
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
