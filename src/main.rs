#![allow(private_interfaces)]

mod cli;
mod click_cursor;
mod crash_report;
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
pub mod modal_window;
mod model;
mod notification;
mod renderer;
mod selection;
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

    fn get_image(&mut self) -> Option<arboard::ImageData<'static>> {
        self.inner.get_image().ok()
    }

    fn set_text(&mut self, text: &str) {
        let _ = self.inner.set_text(text.to_string());
    }
}

/// Custom events sent to the winit event loop from background threads.
#[derive(Debug)]
enum AppEvent {
    /// PTY reader thread produced output. If targeted_pty_polling is enabled,
    /// contains the surface_id that has new data. Otherwise None (poll all).
    TerminalOutput(Option<u32>),
    /// IPC command arrived -- wake up and process.
    IpcReady,
    /// egui requested a repaint (new window, animation, cursor blink).
    EguiRepaint,
    /// Request to create a new window (triggered by IPC or shortcut).
    CreateWindow,
    /// Request to open settings modal.
    OpenSettings,
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
    windows: std::collections::HashMap<WindowId, tasty_window::TastyWindow>,
    /// Active modal window (settings, etc). Max 1 at a time.
    modal: Option<modal_window::ModalWindow>,
    // Shell setup mode (before terminal is created)
    shell_setup_mode: bool,
    shell_setup_path: String,
    shell_setup_gpu: Option<GpuState>,
    shell_setup_window: Option<Arc<Window>>,
}

use winit::window::WindowId;

impl App {
    fn new(proxy: EventLoopProxy<AppEvent>, port_file: Option<String>) -> Self {
        Self {
            engine: engine::Engine::new(proxy.clone(), port_file),
            windows: std::collections::HashMap::new(),
            modal: None,
            shell_setup_mode: false,
            shell_setup_path: String::new(),
            shell_setup_gpu: None,
            shell_setup_window: None,
        }
    }

    /// Get the focused window, if any.
    fn focused_window(&self) -> Option<&tasty_window::TastyWindow> {
        self.engine.focused_window_id.and_then(|id| self.windows.get(&id))
    }

    fn focused_window_mut(&mut self) -> Option<&mut tasty_window::TastyWindow> {
        self.engine.focused_window_id.and_then(|id| self.windows.get_mut(&id))
    }

    /// Create an AppState from a GPU state, computing grid size from the sidebar width.
    fn create_app_state(&self, gpu: &GpuState, sidebar_width: f32) -> crate::state::AppState {
        let sf = gpu.scale_factor();
        let size = gpu.size();
        let sidebar_w = sidebar_width * sf;
        let terminal_rect = crate::model::Rect {
            x: sidebar_w,
            y: 0.0,
            width: (size.width as f32 - sidebar_w).max(1.0),
            height: size.height as f32,
        };
        let (cols, rows) = gpu.grid_size_for_rect(&terminal_rect);

        let proxy = self.engine.proxy.clone();
        let waker: crate::terminal::Waker = Arc::new(move || {
            let _ = proxy.send_event(AppEvent::TerminalOutput(None));
        });

        let mut state = crate::state::AppState::new(cols, rows, waker).expect("failed to create app state");
        state.engine.waker_factory = Some(self.engine.proxy.clone());
        state
    }

    /// Register a TastyWindow and set it as focused.
    fn register_window(&mut self, gpu: GpuState, state: crate::state::AppState, window: Arc<Window>) {
        let window_id = window.id();
        self.windows.insert(window_id, tasty_window::TastyWindow::new(gpu, state, window, self.engine.proxy.clone()));
        self.engine.focused_window_id = Some(window_id);
    }

    /// Initialize the full app state (terminal, IPC server, etc.) after shell is confirmed.
    fn init_app_state(
        &mut self,
        window: Arc<Window>,
        gpu: GpuState,
        settings: crate::settings::Settings,
    ) {
        let startup_command = settings.general.startup_command.clone();
        let mut state = self.create_app_state(&gpu, settings.appearance.sidebar_width);

        if !startup_command.is_empty() {
            if let Some(terminal) = state.focused_terminal_mut() {
                terminal.send_key(&startup_command);
                terminal.send_bytes(b"\r");
            }
        }

        self.engine.start_ipc();
        self.register_window(gpu, state, window);
    }

    /// Create a new window with its own terminal.
    fn create_new_window(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        use winit::window::WindowAttributes;

        let title = if cfg!(debug_assertions) { "Tasty (Debug)" } else { "Tasty" };
        let attrs = WindowAttributes::default()
            .with_title(title)
            .with_inner_size(winit::dpi::LogicalSize::new(1280, 720));

        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .expect("failed to create window"),
        );
        window.set_ime_allowed(true);

        let settings = crate::settings::Settings::load();
        let gpu = pollster::block_on(crate::gpu::GpuState::new(
            window.clone(),
            &settings.appearance,
            self.engine.proxy.clone(),
        ))
        .expect("failed to initialize GPU");

        let state = self.create_app_state(&gpu, settings.appearance.sidebar_width);
        self.register_window(gpu, state, window);
        tracing::info!("created new window {:?}", self.engine.focused_window_id);
    }

    /// Open settings as a modal window.
    fn open_settings_modal(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if self.modal.is_some() {
            return; // Already open
        }

        use winit::window::WindowAttributes;

        let attrs = WindowAttributes::default()
            .with_title("Tasty Settings")
            .with_inner_size(winit::dpi::LogicalSize::new(700, 500))
            .with_visible(false); // Start hidden, show after first render

        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .expect("failed to create settings window"),
        );

        let settings = if let Some(w) = self.focused_window() {
            w.state.engine.settings.clone()
        } else {
            crate::settings::Settings::load()
        };

        let gpu = pollster::block_on(crate::gpu::GpuState::new(
            window.clone(),
            &settings.appearance,
            self.engine.proxy.clone(),
        ))
        .expect("failed to initialize GPU for settings");

        let modal_window_id = window.id();
        let mut modal = modal_window::ModalWindow::new(gpu, window, settings);
        // Render first frame before showing to avoid layout flash
        modal.mark_dirty();
        self.modal = Some(modal);
        self.engine.modal_window_id = Some(modal_window_id);
        tracing::info!("opened settings modal {:?}", modal_window_id);
    }

    /// Close the settings modal and apply settings to all windows.
    fn close_settings_modal(&mut self) {
        if let Some(modal) = self.modal.take() {
            // Apply settings to all windows
            let new_settings = modal.settings;
            for w in self.windows.values_mut() {
                w.state.engine.settings = new_settings.clone();
                w.state.settings_open = false;
                w.mark_dirty();
            }
            // Save to disk
            if let Err(e) = new_settings.save() {
                tracing::warn!("failed to save settings: {e}");
            }
        }
        self.engine.modal_window_id = None;
    }

    /// Process pending IPC commands. Returns true if any commands were processed.
    fn process_ipc(&mut self) -> bool {
        let ipc = match &self.engine.ipc_server {
            Some(ipc) => ipc,
            None => return false,
        };

        let mut processed = false;
        while let Ok(cmd) = ipc.try_recv() {
            // App-level IPC methods (don't need focused window)
            if cmd.request.method == "window.create" {
                let _ = self.engine.proxy.send_event(AppEvent::CreateWindow);
                let response = ipc::protocol::JsonRpcResponse::success(
                    cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
                    serde_json::json!({"scheduled": true}),
                );
                let _ = cmd.response_tx.send(response);
                processed = true;
                continue;
            }
            if cmd.request.method == "window.close" {
                // Close the focused window
                if let Some(focused_id) = self.engine.focused_window_id {
                    self.windows.remove(&focused_id);
                    self.engine.focused_window_id = self.windows.keys().next().copied();
                }
                let response = ipc::protocol::JsonRpcResponse::success(
                    cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
                    serde_json::json!({"closed": true}),
                );
                let _ = cmd.response_tx.send(response);
                processed = true;
                continue;
            }
            if cmd.request.method == "window.focus" {
                // Focus a specific window by searching for matching ID string
                let target = cmd.request.params.get("id").and_then(|v| v.as_str()).unwrap_or("");
                let mut found = false;
                for (id, w) in &self.windows {
                    if format!("{:?}", id) == target {
                        w.window.focus_window();
                        self.engine.focused_window_id = Some(*id);
                        found = true;
                        break;
                    }
                }
                let response = ipc::protocol::JsonRpcResponse::success(
                    cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
                    serde_json::json!({"focused": found}),
                );
                let _ = cmd.response_tx.send(response);
                processed = true;
                continue;
            }
            if cmd.request.method == "window.list" {
                let focused_id = self.engine.focused_window_id;
                let list: Vec<_> = self.windows.iter().map(|(id, w)| {
                    serde_json::json!({
                        "id": format!("{:?}", id),
                        "focused": focused_id == Some(*id),
                        "title": w.state.active_workspace().name,
                    })
                }).collect();
                let response = ipc::protocol::JsonRpcResponse::success(
                    cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
                    serde_json::json!(list),
                );
                let _ = cmd.response_tx.send(response);
                processed = true;
                continue;
            }

            // Focused-window IPC methods
            let focused_id = match self.engine.focused_window_id {
                Some(id) => id,
                None => continue,
            };
            let w = match self.windows.get_mut(&focused_id) {
                Some(w) => w,
                None => continue,
            };

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
    crash_report::init();

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
