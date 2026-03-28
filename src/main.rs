mod cli;
pub mod engine;
pub mod engine_state;
mod event_handler;
mod explorer_ui;
mod font;
mod global_hooks;
mod gpu;
mod i18n;
mod ipc;
mod markdown_ui;
mod model;
mod notification;
mod renderer;
mod settings;
mod settings_ui;
mod shortcuts;
mod state;
mod surface_meta;
pub mod tasty_window;
pub mod theme;
mod ui;

// Re-export tasty_terminal as terminal for backward compatibility within the crate
use tasty_terminal as terminal;

use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use tracing_subscriber::EnvFilter;
use winit::event_loop::{EventLoop, EventLoopProxy};
use winit::window::Window;

use gpu::GpuState;
use model::DividerInfo;

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
    /// IPC command arrived -- wake up and process.
    IpcReady,
    /// egui requested a repaint (new window, animation, cursor blink).
    EguiRepaint,
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
    engine: engine::Engine,
    primary_window: Option<tasty_window::TastyWindow>,
    // Shell setup mode (before terminal is created)
    shell_setup_mode: bool,
    shell_setup_path: String,
    shell_setup_gpu: Option<GpuState>,
    shell_setup_window: Option<Arc<Window>>,
}

impl App {
    fn new(proxy: EventLoopProxy<AppEvent>, port_file: Option<String>) -> Self {
        Self {
            engine: engine::Engine::new(proxy.clone(), port_file),
            primary_window: None,
            shell_setup_mode: false,
            shell_setup_path: String::new(),
            shell_setup_gpu: None,
            shell_setup_window: None,
        }
    }

    /// Initialize the full app state (terminal, IPC server, etc.) after shell is confirmed.
    fn init_app_state(
        &mut self,
        window: Arc<Window>,
        gpu: GpuState,
        settings: crate::settings::Settings,
    ) {
        let sidebar_logical_width = settings.appearance.sidebar_width;
        let startup_command = settings.general.startup_command.clone();

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

        let proxy = self.engine.proxy.clone();
        let waker: crate::terminal::Waker = Arc::new(move || {
            let _ = proxy.send_event(AppEvent::TerminalOutput);
        });

        let mut state = crate::state::AppState::new(cols, rows, waker).expect("failed to create app state");

        if !startup_command.is_empty() {
            if let Some(terminal) = state.focused_terminal_mut() {
                terminal.send_key(&startup_command);
                terminal.send_bytes(b"\r");
            }
        }

        self.engine.start_ipc();

        self.primary_window = Some(tasty_window::TastyWindow::new(gpu, state, window));
    }

    /// Process pending IPC commands. Returns true if any commands were processed.
    fn process_ipc(&mut self) -> bool {
        let ipc = match &self.engine.ipc_server {
            Some(ipc) => ipc,
            None => return false,
        };
        let w = match &mut self.primary_window {
            Some(w) => w,
            None => return false,
        };

        let mut processed = false;
        while let Ok(cmd) = ipc.try_recv() {
            if cmd.request.method == "ui.screenshot" {
                let path = cmd.request.params
                    .get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("screenshot.png")
                    .to_string();
                w.gpu.pending_screenshot = Some(std::path::PathBuf::from(&path));
                w.mark_dirty();
                let response = ipc::protocol::JsonRpcResponse::success(
                    cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
                    serde_json::json!({"path": path, "scheduled": true}),
                );
                let _ = cmd.response_tx.send(response);
                processed = true;
                continue;
            }

            let response = ipc::handler::handle(&mut w.state, &cmd.request);
            let _ = cmd.response_tx.send(response);
            w.dirty = true;
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

    // Initialize i18n
    let lang_settings = settings::Settings::load();
    i18n::init(&lang_settings.general.language);

    // If a subcommand was provided, run in CLI client mode
    if let Some(command) = cli.command {
        return cli::run_client(command);
    }

    // Run the GUI
    let event_loop = EventLoop::<AppEvent>::with_user_event().build()?;
    let proxy = event_loop.create_proxy();
    let mut app = App::new(proxy, cli.port_file);
    event_loop.run_app(&mut app)?;

    Ok(())
}
