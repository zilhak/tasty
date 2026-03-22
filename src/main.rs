mod cli;
mod event_handler;
mod font;
mod gpu;
mod ipc;
mod model;
mod notification;
mod renderer;
mod settings;
mod settings_ui;
mod shortcuts;
mod state;
mod ui;

// Re-export tasty_terminal as terminal for backward compatibility within the crate
use tasty_terminal as terminal;

use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use tracing_subscriber::EnvFilter;
use winit::event_loop::{EventLoop, EventLoopProxy};
use winit::keyboard::ModifiersState;
use winit::window::Window;

use gpu::GpuState;
use ipc::server::IpcServer;
use model::{DividerInfo, Rect};
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
        model::compute_terminal_rect(size.width as f32, size.height as f32, sidebar_logical_width, gpu.scale_factor())
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
