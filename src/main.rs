mod gpu;
mod terminal;

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
    window: Option<Window>,
    gpu: Option<GpuState>,
    terminal: Option<Terminal>,
}

impl App {
    fn new() -> Self {
        Self {
            window: None,
            gpu: None,
            terminal: None,
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

        let window = event_loop
            .create_window(attrs)
            .expect("failed to create window");

        let gpu = pollster::block_on(GpuState::new(&window))
            .expect("failed to initialize GPU");

        let terminal = Terminal::new(80, 24).expect("failed to create terminal");

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
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed {
                    return;
                }
                if let Some(terminal) = &mut self.terminal {
                    match event.logical_key.as_ref() {
                        Key::Named(NamedKey::Enter) => terminal.send_bytes(b"\r"),
                        Key::Named(NamedKey::Backspace) => terminal.send_bytes(b"\x7f"),
                        Key::Named(NamedKey::Tab) => terminal.send_bytes(b"\t"),
                        Key::Named(NamedKey::Escape) => terminal.send_bytes(b"\x1b"),
                        Key::Named(NamedKey::ArrowUp) => terminal.send_bytes(b"\x1b[A"),
                        Key::Named(NamedKey::ArrowDown) => terminal.send_bytes(b"\x1b[B"),
                        Key::Named(NamedKey::ArrowRight) => terminal.send_bytes(b"\x1b[C"),
                        Key::Named(NamedKey::ArrowLeft) => terminal.send_bytes(b"\x1b[D"),
                        Key::Character(c) => {
                            terminal.send_key(c);
                        }
                        _ => {}
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                // Process PTY output
                if let Some(terminal) = &mut self.terminal {
                    terminal.process();
                }

                if let Some(gpu) = &mut self.gpu {
                    match gpu.render() {
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
