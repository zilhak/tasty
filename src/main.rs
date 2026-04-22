#![allow(private_interfaces)]
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod bookmarks;
mod cli;
mod click_cursor;
mod clipboard_history;
mod clipboard_viewer_ui;
mod crash_report;
#[cfg(debug_assertions)]
mod debug_info;
mod double_tap;
mod empty_ui;
pub mod engine;
pub mod engine_state;
mod event_handler;
mod explorer_ui;
mod file_clipboard;
mod font;
mod global_hooks;
mod gpu;
mod html_ui;
mod i18n;
mod image_ui;
mod ipc;
#[cfg(windows)]
mod jump_list;
mod markdown_ui;
mod model;
mod native_menu;
mod notification;
mod recent_files;
mod renderer;
mod selection;
mod settings;
mod settings_ui;
mod shortcuts;
mod state;
mod storage;
mod surface_meta;
#[cfg(windows)]
mod system_tray;
pub mod theme;
mod ui;
mod webview;
pub mod window;

#[cfg(target_os = "macos")]
mod macos_delegate;

// Re-export tasty_terminal as terminal for backward compatibility within the crate
use tasty_terminal as terminal;

use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use winit::event_loop::{EventLoop, EventLoopProxy};
use winit::window::Window;

use gpu::GpuState;
use model::DividerInfo;
use window::Window as _;

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
    /// Request to shut down the entire application.
    Shutdown,
    /// Request to minimize (park state, close windows).
    Minimize,
    /// Request quit following the close_behavior setting.
    QuitRequested,
    /// Request to show window from system tray (Windows only).
    #[cfg(windows)]
    TrayShowWindow,
    /// 주기적으로 시스템 클립보드를 폴링해 히스토리에 반영. 폴링 스레드가 발송.
    ClipboardTick,
}

/// Tracks an active divider drag operation.
#[derive(Clone, Copy)]
enum DividerDragKind {
    /// Dragging a pane-level split divider.
    Pane,
    /// Dragging a surface-level split divider (within a tab).
    Surface,
}

#[derive(Clone, Copy)]
struct DividerDrag {
    info: DividerInfo,
    kind: DividerDragKind,
}

struct App {
    engine: engine::Engine,
    /// 모든 윈도우(모달 포함). `engine.active_modal_id`로 현재 활성 모달을 식별한다.
    /// 모달도 여기에 들어가며, 모달은 엔진 전역에 최대 1개라는 불변식을 유지한다.
    windows: std::collections::HashMap<WindowId, Box<dyn window::Window>>,
    /// Parked AppStates: preserved when all windows are closed so PTY sessions survive.
    /// Moved into new windows when created, or used directly for IPC.
    parked_states: Vec<state::AppState>,
    // Shell setup mode (before terminal is created)
    shell_setup_mode: bool,
    shell_setup_path: String,
    shell_setup_gpu: Option<GpuState>,
    shell_setup_window: Option<Arc<Window>>,
    /// System tray icon (Windows only). Must be kept alive for the tray to remain visible.
    #[cfg(windows)]
    tray_icon: Option<tray_icon::TrayIcon>,
    /// Tray menu item IDs for event matching (Windows only).
    #[cfg(windows)]
    tray_menu_ids: Option<system_tray::TrayMenuIds>,
}

use winit::window::WindowId;

impl App {
    fn new(proxy: EventLoopProxy<AppEvent>, port_file: Option<String>) -> Self {
        Self {
            engine: engine::Engine::new(proxy.clone(), port_file),
            windows: std::collections::HashMap::new(),
            parked_states: Vec::new(),
            shell_setup_mode: false,
            shell_setup_path: String::new(),
            shell_setup_gpu: None,
            shell_setup_window: None,
            #[cfg(windows)]
            tray_icon: None,
            #[cfg(windows)]
            tray_menu_ids: None,
        }
    }

    /// Get the focused main window, if any.
    /// 모달이 아닌 MainWindow만 반환한다 — IPC/키보드 라우팅의 일반적 대상.
    fn focused_window(&self) -> Option<&window::main::MainWindow> {
        self.engine
            .focused_window_id
            .and_then(|id| self.windows.get(&id))
            .and_then(|w| w.as_main())
    }

    fn focused_window_mut(&mut self) -> Option<&mut window::main::MainWindow> {
        self.engine
            .focused_window_id
            .and_then(|id| self.windows.get_mut(&id))
            .and_then(|w| w.as_main_mut())
    }

    /// 모든 MainWindow를 순회. 모달은 제외된다.
    fn main_windows_iter_mut(&mut self) -> impl Iterator<Item = &mut window::main::MainWindow> {
        self.windows.values_mut().filter_map(|w| w.as_main_mut())
    }

    /// Create an AppState from a GPU state, computing grid size from the sidebar width.
    fn create_app_state(
        &self,
        gpu: &GpuState,
        sidebar_width: crate::model::LogicalPx,
    ) -> crate::state::AppState {
        let sf = gpu.scale_factor();
        let size = gpu.size();
        let sidebar_w = sidebar_width.to_physical(sf);
        let terminal_rect = crate::model::Rect {
            x: sidebar_w,
            y: crate::model::PhysicalPx(0.0),
            width: (crate::model::PhysicalPx(size.width as f32) - sidebar_w)
                .max(crate::model::PhysicalPx(1.0)),
            height: crate::model::PhysicalPx(size.height as f32),
        };
        let (cols, rows) = gpu.grid_size_for_rect(&terminal_rect);

        let proxy = self.engine.proxy.clone();
        let waker: crate::terminal::Waker = Arc::new(move || {
            let _ = proxy.send_event(AppEvent::TerminalOutput(None));
        });

        let mut state =
            crate::state::AppState::new(cols, rows, waker).expect("failed to create app state");
        state.engine.waker_factory = Some(self.engine.proxy.clone());
        state
    }

    /// Register a MainWindow and set it as focused.
    fn register_window(
        &mut self,
        gpu: GpuState,
        state: crate::state::AppState,
        window: Arc<Window>,
    ) {
        let window_id = window.id();
        let main = window::main::MainWindow::new(gpu, state, window, self.engine.proxy.clone());
        self.windows.insert(window_id, Box::new(main));
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

        let title = if cfg!(debug_assertions) {
            "Tasty (Debug)"
        } else {
            "Tasty"
        };
        let attrs = WindowAttributes::default()
            .with_title(title)
            .with_inner_size(winit::dpi::LogicalSize::new(1280, 720))
            .with_min_inner_size(winit::dpi::LogicalSize::new(640, 480));

        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .expect("failed to create window"),
        );
        window.set_ime_allowed(true);

        let mut settings = crate::settings::Settings::load();
        // Migrate legacy theme names
        match settings.appearance.theme.as_str() {
            "dark" => settings.appearance.theme = "catppuccin-mocha".to_string(),
            "light" => settings.appearance.theme = "catppuccin-latte".to_string(),
            _ => {}
        }
        // Apply saved theme preset at startup
        let presets = crate::theme::presets();
        if let Some(preset) = presets.iter().find(|p| p.id == settings.appearance.theme) {
            crate::theme::set_theme(preset.theme);
        }
        let gpu = pollster::block_on(crate::gpu::GpuState::new(
            window.clone(),
            &settings.appearance,
            self.engine.proxy.clone(),
        ))
        .expect("failed to initialize GPU");

        // Reuse parked state if available (restoring previous session)
        let mut state = if !self.parked_states.is_empty() {
            let parked = self.parked_states.remove(0);
            tracing::info!(
                "restoring parked state with {} workspace(s), {} remaining",
                parked.engine.workspaces.len(),
                self.parked_states.len()
            );
            parked
        } else {
            self.create_app_state(&gpu, settings.appearance.sidebar_width)
        };

        // Ensure at least one workspace exists for the new window
        state.ensure_workspace_exists();

        self.register_window(gpu, state, window);
        tracing::info!("created new window {:?}", self.engine.focused_window_id);
    }

    /// Open settings as a modal window.
    fn open_settings_modal(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if self.engine.is_modal_active() {
            return; // Another modal is already open
        }

        use winit::window::WindowAttributes;

        let attrs = WindowAttributes::default()
            .with_title("Tasty Settings")
            .with_inner_size(winit::dpi::LogicalSize::new(960, 640))
            .with_min_inner_size(winit::dpi::LogicalSize::new(960, 640))
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
        let mut modal = window::SettingsWindow::new(gpu, window, settings);
        // On Windows, hidden windows do not receive RedrawRequested events,
        // so render the first frame immediately instead of waiting for the event loop.
        // On other platforms, mark_dirty() + request_redraw() is sufficient.
        #[cfg(windows)]
        {
            use window::Window as _;
            modal.render();
        }
        #[cfg(not(windows))]
        {
            use window::Window as _;
            modal.mark_dirty();
        }
        self.open_modal(Box::new(modal), modal_window_id);
        tracing::info!("opened settings modal {:?}", modal_window_id);
    }

    /// Open a modal, registering it in the unified window map.
    /// 모달도 일반 윈도우와 같은 `windows` 맵에 저장되며, `active_modal_id`로 식별된다.
    fn open_modal(&mut self, modal: Box<dyn window::Window>, window_id: WindowId) {
        self.windows.insert(window_id, modal);
        self.engine.active_modal_id = Some(window_id);
    }

    /// Close the active modal and handle modal-specific cleanup.
    fn close_active_modal(&mut self) {
        let Some(modal_id) = self.engine.active_modal_id.take() else {
            return;
        };
        let Some(modal) = self.windows.remove(&modal_id) else {
            return;
        };
        // If it was a settings modal, apply settings to all main windows
        if let Some(settings_modal) = modal.as_any().downcast_ref::<window::SettingsWindow>() {
            let new_settings = settings_modal.settings.clone();
            for main in self.main_windows_iter_mut() {
                main.state.engine.settings = new_settings.clone();
                main.state.settings_open = false;
                main.mark_dirty();
            }
            if let Err(e) = new_settings.save() {
                tracing::warn!("failed to save settings: {e}");
            }
        }
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
            #[cfg(debug_assertions)]
            if cmd.request.method == "system.shutdown" {
                let response = ipc::protocol::JsonRpcResponse::success(
                    cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
                    serde_json::json!({"shutdown": true}),
                );
                let _ = cmd.response_tx.send(response);
                let _ = self.engine.proxy.send_event(AppEvent::Shutdown);
                return true;
            }
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
                // Focus a specific MainWindow by searching for matching ID string
                let target = cmd
                    .request
                    .params
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let mut found = false;
                for (id, w) in &self.windows {
                    if w.as_main().is_none() {
                        continue; // 모달은 focus 대상이 아님
                    }
                    if format!("{:?}", id) == target {
                        w.base().winit.focus_window();
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
                let list: Vec<_> = self
                    .windows
                    .iter()
                    .filter_map(|(id, w)| {
                        let main = w.as_main()?;
                        Some(serde_json::json!({
                            "id": format!("{:?}", id),
                            "focused": focused_id == Some(*id),
                            "title": main.state.active_workspace().name,
                        }))
                    })
                    .collect();
                let response = ipc::protocol::JsonRpcResponse::success(
                    cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
                    serde_json::json!(list),
                );
                let _ = cmd.response_tx.send(response);
                processed = true;
                continue;
            }

            // Window-required IPC methods (GPU, IME, debug)
            #[allow(unused_mut)]
            let mut is_window_required = cmd.request.method.starts_with("surface.ime_");
            #[cfg(debug_assertions)]
            {
                is_window_required = is_window_required
                    || cmd.request.method == "debug.info"
                    || cmd.request.method == "ui.screenshot";
            }
            if is_window_required {
                let focused_id = match self.engine.focused_window_id {
                    Some(id) => id,
                    None => {
                        let response = ipc::protocol::JsonRpcResponse::error(
                            cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
                            -32000,
                            "No window available for this command",
                        );
                        let _ = cmd.response_tx.send(response);
                        processed = true;
                        continue;
                    }
                };
                let w = match self
                    .windows
                    .get_mut(&focused_id)
                    .and_then(|w| w.as_main_mut())
                {
                    Some(w) => w,
                    None => continue,
                };

                #[cfg(debug_assertions)]
                if cmd.request.method == "debug.info" {
                    let debug_data = debug_info::collect(&w.state, Some(&w.base.gpu), w.ime_active);
                    let response = ipc::protocol::JsonRpcResponse::success(
                        cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
                        debug_data,
                    );
                    let _ = cmd.response_tx.send(response);
                    processed = true;
                    continue;
                }
                #[cfg(debug_assertions)]
                if cmd.request.method == "ui.screenshot" {
                    let path = cmd
                        .request
                        .params
                        .get("path")
                        .and_then(|v| v.as_str())
                        .unwrap_or("screenshot.png")
                        .to_string();
                    w.base.gpu.pending_screenshot = Some(std::path::PathBuf::from(&path));
                    w.mark_dirty();
                    let response = ipc::protocol::JsonRpcResponse::success(
                        cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
                        serde_json::json!({"path": path, "scheduled": true}),
                    );
                    let _ = cmd.response_tx.send(response);
                    processed = true;
                    continue;
                }
                if cmd.request.method.starts_with("surface.ime_") {
                    let id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
                    let response = ipc::handler::ime::handle_ime_method(
                        w,
                        &cmd.request.method,
                        &cmd.request.params,
                        id,
                    );
                    let _ = cmd.response_tx.send(response);
                    w.base.dirty = true;
                }
                processed = true;
                continue;
            }

            // All other commands: route to focused MainWindow or parked state
            let focused_id = self.engine.focused_window_id;
            if let Some(id) = focused_id {
                if let Some(w) = self.windows.get_mut(&id).and_then(|w| w.as_main_mut()) {
                    let response = ipc::handler::handle(&mut w.state, &cmd.request);
                    let _ = cmd.response_tx.send(response);
                    w.base.dirty = true;
                    processed = true;
                    continue;
                }
            }
            if let Some(state) = self.parked_states.first_mut() {
                let response = ipc::handler::handle(state, &cmd.request);
                let _ = cmd.response_tx.send(response);
                processed = true;
            }
        }
        processed
    }
}

fn main() -> Result<()> {
    #[cfg(all(windows, not(debug_assertions)))]
    {
        use windows::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
        // 부모 프로세스에 콘솔이 있으면 attach (ConPTY 포함). 부모가 GUI 셸이면
        // 호출은 실패하지만 무해하므로 결과를 의도적으로 무시한다.
        unsafe {
            let _ = AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }

    crash_report::init();

    // Handle -a/--all before clap parsing (clap's -h exits before we can check -a)
    {
        let args: Vec<String> = std::env::args().collect();
        if args.iter().any(|a| a == "-a" || a == "--all") {
            cli::print_command_tree();
            return Ok(());
        }
    }

    // Parse CLI arguments (custom error handling for contextual messages)
    let cli = match cli::Cli::try_parse() {
        Ok(cli) => cli,
        Err(err) => {
            cli::format_parse_error(err);
            unreachable!();
        }
    };

    // Initialize i18n
    let lang_settings = settings::Settings::load();
    i18n::init(&lang_settings.general.language);

    // Initialize state.db (SQLite). 실패 시 인메모리 폴백.
    storage::init();
    // Legacy JSON → SQLite 1회성 마이그레이션. CLI 클라이언트 모드에서는
    // 불필요하지만 싸게 끝나므로 동일하게 돌린다.
    bookmarks::migrate_from_json();
    recent_files::migrate_from_json();

    // If a subcommand was provided, run in CLI client mode
    if let Some(command) = cli.command {
        return cli::run_client(command);
    }

    // Inside a tasty terminal without subcommand: show help instead of launching GUI
    if !cli.launch && std::env::var("TASTY_SURFACE_ID").is_ok() {
        use clap::CommandFactory;
        let mut cmd = cli::Cli::command();
        cmd.print_help()?;
        println!();
        return Ok(());
    }

    // Run the GUI
    let event_loop = EventLoop::<AppEvent>::with_user_event().build()?;
    let proxy = event_loop.create_proxy();

    #[cfg(target_os = "macos")]
    macos_delegate::store_proxy(proxy.clone());

    // 시스템 클립보드 폴링 스레드. interval은 앱 시작 시점의 설정 값을 사용하며,
    // runtime 변경은 앱 재시작 후 반영된다.
    {
        let poll_interval_ms = crate::settings::Settings::load()
            .clipboard
            .poll_interval_ms
            .max(100);
        let tick_proxy = proxy.clone();
        std::thread::spawn(move || {
            let interval = std::time::Duration::from_millis(poll_interval_ms);
            loop {
                std::thread::sleep(interval);
                if tick_proxy.send_event(AppEvent::ClipboardTick).is_err() {
                    break; // event loop exited
                }
            }
        });
    }

    let mut app = App::new(proxy, cli.port_file);
    event_loop.run_app(&mut app)?;

    Ok(())
}
