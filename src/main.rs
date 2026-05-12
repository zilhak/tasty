#![allow(private_interfaces)]
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app_icon;
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
mod file_clipboard;
mod file_drag;
mod global_hooks;
mod gpu;
mod html_ui;
mod image_ui;
mod ipc;
mod layout_persistence;
#[cfg(windows)]
mod jump_list;
mod markdown_ui;
mod native_menu;
mod notification;
mod plugin;
mod plugins_ui;
mod recent_files;
mod renderer;
mod search_state;
mod selection;
mod terminal_link;
mod settings_ui;
mod shortcuts;
mod state;
mod theme_bridge;
mod storage;
mod surface_meta;
mod surface_registry;
mod waker_factory_winit;
#[cfg(windows)]
mod system_tray;
mod ui;
mod webview;
pub mod window;

#[cfg(target_os = "macos")]
mod macos_delegate;

// Re-export tasty_terminal as terminal for backward compatibility within the crate
use tasty_terminal as terminal;
// Re-export tasty_core modules so existing `crate::model::...` etc. paths keep working
pub use tasty_core::{i18n, model, paths, theme};
// Re-export tasty_settings as `crate::settings` to keep existing reverse imports
pub use tasty_settings as settings;
// Re-export tasty_font as `crate::font` to keep existing reverse imports
pub use tasty_font as font;

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

/// Clipboard data detected by the background polling thread.
enum ClipboardData {
    Text(String),
    Image(crate::clipboard_history::ImageData),
}

impl std::fmt::Debug for ClipboardData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClipboardData::Text(t) => write!(f, "Text({}B)", t.len()),
            ClipboardData::Image(img) => write!(f, "Image({}x{})", img.width, img.height),
        }
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
    /// Request to open plugins modal.
    OpenPlugins,
    /// Request to shut down the entire application.
    Shutdown,
    /// Request to minimize (park state, close windows).
    Minimize,
    /// Request quit following the close_behavior setting.
    QuitRequested,
    /// Request to show window from system tray (Windows only).
    #[cfg(windows)]
    TrayShowWindow,
    /// 백그라운드 스레드에서 클립보드 변경을 감지하여 데이터를 전달.
    ClipboardChanged(ClipboardData),
    /// ~1초 간격 ticker. 모든 surface의 busy 상태를 다시 평가한다.
    BusyPoll,
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
    /// Modal shake animation state.
    modal_shake: Option<ModalShake>,
    /// Whether input simulation IPC is enabled (debug builds only).
    #[cfg(debug_assertions)]
    input_simulation_enabled: bool,
    /// Plugin host manager. None until the first AppState is created
    /// (which provides the WakerFactory).
    plugin_manager: Option<plugin::PluginManager>,
}

/// State for the modal window shake animation.
struct ModalShake {
    start: std::time::Instant,
    /// Original window position before shake began.
    origin: winit::dpi::PhysicalPosition<i32>,
}

use winit::window::WindowId;

impl App {
    fn new(
        proxy: EventLoopProxy<AppEvent>,
        port_file: Option<String>,
        #[cfg(debug_assertions)] input_simulation_enabled: bool,
    ) -> Self {
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
            modal_shake: None,
            #[cfg(debug_assertions)]
            input_simulation_enabled,
            plugin_manager: None,
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
        &mut self,
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

        let factory: tasty_core::SharedWakerFactory = Arc::new(
            crate::waker_factory_winit::WinitWakerFactory::new(self.engine.proxy.clone()),
        );
        let waker: crate::terminal::Waker = factory.make_default_waker();

        let mut state =
            crate::state::AppState::new(cols, rows, waker).expect("failed to create app state");
        state.engine.waker_factory = Some(factory.clone());

        // 첫 윈도우 생성 시 plugin manager 한 번만 초기화.
        if self.plugin_manager.is_none() {
            let mut mgr = plugin::PluginManager::new(factory);
            mgr.set_surface_registry(state.engine.surface_registry.clone());
            // 기본 제공 플러그인이 설치되지 않았으면 번들에서 복사. 사용자가
            // 명시적으로 제거한 항목 (`removed_builtins`)은 건드리지 않는다.
            plugin::install_builtins_if_needed(&mut mgr);
            mgr.packages = plugin::discover();
            mgr.discover_and_start();
            self.plugin_manager = Some(mgr);
        }

        // pending_layout_restore가 있으면, plugin이 surface_kinds를 등록할 시간을
        // 잠깐 주고 적용한다. 시간 내에 hello가 도착하지 않은 plugin이 제공하는
        // kind는 복원에서 일단 skip되며, 추후 정상 흐름으로 새로 만들 수 있다.
        if let Some(saved) = state.engine.pending_layout_restore.take() {
            if let Some(mgr) = self.plugin_manager.as_mut() {
                use std::time::{Duration, Instant};
                let deadline = Instant::now() + Duration::from_millis(300);
                let needed: Vec<String> = saved.required_plugin_kinds();
                while Instant::now() < deadline {
                    mgr.pump();
                    let registered_all = needed
                        .iter()
                        .all(|k| state.engine.surface_registry.get(k).is_some());
                    if registered_all {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
            if saved.restore(&mut state.engine) {
                tracing::info!("Layout restored from layout.json (deferred)");
            }
        }
        #[cfg(debug_assertions)]
        {
            state.engine.input_simulation_enabled = self.input_simulation_enabled;
        }
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
        let mut attrs = WindowAttributes::default()
            .with_title(title)
            .with_inner_size(winit::dpi::LogicalSize::new(1280, 720))
            .with_min_inner_size(winit::dpi::LogicalSize::new(640, 480));
        if let Some(icon) = crate::app_icon::winit_window_icon() {
            attrs = attrs.with_window_icon(Some(icon));
        }

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

        let mut attrs = WindowAttributes::default()
            .with_title("Tasty Settings")
            .with_inner_size(winit::dpi::LogicalSize::new(960, 640))
            .with_min_inner_size(winit::dpi::LogicalSize::new(960, 640))
            .with_visible(false); // Start hidden, show after first render
        if let Some(icon) = crate::app_icon::winit_window_icon() {
            attrs = attrs.with_window_icon(Some(icon));
        }

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
        modal.set_plugin_shortcuts(self.snapshot_plugin_shortcuts());
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

    /// SettingsWindow가 회수해 온 plugin shortcut override draft를 PluginsConfig에
    /// 반영하고 디스크에 저장. 값이 `Some(ov)`이면 set, `None`이면 clear.
    fn apply_plugin_shortcut_draft(
        &mut self,
        draft: std::collections::BTreeMap<
            (String, String),
            Option<plugin::registry_state::ShortcutOverride>,
        >,
    ) {
        if draft.is_empty() {
            return;
        }
        let Some(mgr) = self.plugin_manager.as_mut() else {
            tracing::warn!("plugin shortcut draft dropped: plugin manager not initialized");
            return;
        };
        let mut changed = false;
        for ((plugin_id, command_id), value) in draft {
            match value {
                Some(ov) => {
                    mgr.config.set_shortcut_override(&plugin_id, &command_id, ov);
                    changed = true;
                }
                None => {
                    if mgr.config.clear_shortcut_override(&plugin_id, &command_id) {
                        changed = true;
                    }
                }
            }
        }
        if changed {
            if let Err(e) = mgr.config.save() {
                tracing::warn!("plugins.toml save failed after shortcut update: {e}");
            }
        }
    }

    /// Plugins 키바인딩 서브탭에 표시할 snapshot.
    fn snapshot_plugin_shortcuts(&self) -> settings_ui::PluginShortcutSnapshot {
        let Some(mgr) = self.plugin_manager.as_ref() else {
            return settings_ui::PluginShortcutSnapshot::default();
        };
        // plugin_id → display name map (매니페스트의 name).
        let name_for: std::collections::HashMap<&str, &str> = mgr
            .packages
            .iter()
            .map(|p| (p.manifest.id.as_str(), p.manifest.name.as_str()))
            .collect();

        let rows: Vec<settings_ui::PluginShortcutRow> = mgr
            .command_registry
            .iter_all()
            .map(|e| settings_ui::PluginShortcutRow {
                plugin_id: e.plugin_id.clone(),
                plugin_name: name_for
                    .get(e.plugin_id.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| e.plugin_id.clone()),
                command_id: e.command_id.clone(),
                title_i18n_key: e.title_i18n_key.clone(),
                binding_mode: e.binding_mode.clone(),
                manifest_default: e.manifest_default.clone(),
                current_override: mgr
                    .config
                    .shortcut_override(&e.plugin_id, &e.command_id)
                    .cloned(),
            })
            .collect();
        settings_ui::PluginShortcutSnapshot { rows }
    }

    /// Build a snapshot of currently installed plugins for the plugins modal.
    fn snapshot_plugins(&self) -> plugins_ui::PluginsSnapshot {
        let Some(mgr) = self.plugin_manager.as_ref() else {
            return plugins_ui::PluginsSnapshot::default();
        };
        let plugins = mgr
            .packages
            .iter()
            .map(|pkg| {
                let id = &pkg.manifest.id;
                let granted: Vec<String> = mgr
                    .config
                    .granted_permissions(id)
                    .into_iter()
                    .collect();
                plugins_ui::PluginEntry {
                    id: id.clone(),
                    name: pkg.manifest.name.clone(),
                    version: pkg.manifest.version.clone(),
                    description: pkg.manifest.description.clone(),
                    authors: pkg.manifest.authors.clone(),
                    homepage: pkg.manifest.homepage.clone(),
                    enabled: !mgr.config.is_disabled(id),
                    running: mgr.is_running(id),
                    builtin: plugin::is_builtin_plugin(id),
                    surface_kinds: pkg
                        .manifest
                        .surface_kinds
                        .iter()
                        .map(|k| k.kind.clone())
                        .collect(),
                    manifest_permissions: pkg.manifest.permissions.clone(),
                    granted_permissions: granted,
                    log_path: mgr.log_path(id).to_string_lossy().into_owned(),
                }
            })
            .collect();
        plugins_ui::PluginsSnapshot { plugins }
    }

    /// Open the plugins modal window.
    fn open_plugins_modal(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if self.engine.is_modal_active() {
            return;
        }

        use winit::window::WindowAttributes;

        let mut attrs = WindowAttributes::default()
            .with_title("Tasty Plugins")
            .with_inner_size(winit::dpi::LogicalSize::new(880, 560))
            .with_min_inner_size(winit::dpi::LogicalSize::new(720, 480))
            .with_visible(false);
        if let Some(icon) = crate::app_icon::winit_window_icon() {
            attrs = attrs.with_window_icon(Some(icon));
        }

        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .expect("failed to create plugins window"),
        );

        let appearance = self
            .focused_window()
            .map(|w| w.state.engine.settings.appearance.clone())
            .unwrap_or_else(|| crate::settings::Settings::load().appearance);

        let gpu = pollster::block_on(crate::gpu::GpuState::new(
            window.clone(),
            &appearance,
            self.engine.proxy.clone(),
        ))
        .expect("failed to initialize GPU for plugins window");

        let snapshot = self.snapshot_plugins();
        let modal_window_id = window.id();
        let mut modal = window::PluginsWindow::new(gpu, window, snapshot);
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
        tracing::info!("opened plugins modal {:?}", modal_window_id);
    }

    /// Drain pending actions from the plugins modal and apply them to the manager.
    /// Refreshes the modal's snapshot after applying.
    fn process_plugins_window_actions(&mut self) {
        let Some(modal_id) = self.engine.active_modal_id else {
            return;
        };
        let Some(modal) = self.windows.get_mut(&modal_id) else {
            return;
        };
        let Some(plugins_window) = modal.as_any_mut().downcast_mut::<window::PluginsWindow>()
        else {
            return;
        };
        let actions = std::mem::take(&mut plugins_window.pending_actions);
        if actions.is_empty() {
            return;
        }

        let Some(mgr) = self.plugin_manager.as_mut() else {
            return;
        };

        for action in actions {
            match action {
                plugins_ui::PluginsAction::SetEnabled { id, enabled } => {
                    let result = if enabled { mgr.enable(&id) } else { mgr.disable(&id) };
                    if let Err(e) = result {
                        tracing::warn!("plugins modal: set_enabled({id}, {enabled}) failed: {e}");
                    }
                }
                plugins_ui::PluginsAction::Grant { id, permission } => {
                    let resp = ipc::handler::plugin::handle_grant(
                        Some(mgr),
                        serde_json::json!(0),
                        &serde_json::json!({ "id": id, "permission": permission }),
                    );
                    if resp.error.is_some() {
                        tracing::warn!(
                            "plugins modal: grant({id}, {permission}) failed: {:?}",
                            resp.error
                        );
                    }
                }
                plugins_ui::PluginsAction::Revoke { id, permission } => {
                    let resp = ipc::handler::plugin::handle_revoke(
                        Some(mgr),
                        serde_json::json!(0),
                        &serde_json::json!({ "id": id, "permission": permission }),
                    );
                    if resp.error.is_some() {
                        tracing::warn!(
                            "plugins modal: revoke({id}, {permission}) failed: {:?}",
                            resp.error
                        );
                    }
                }
                plugins_ui::PluginsAction::Uninstall { id } => {
                    let resp = ipc::handler::plugin::handle_remove(
                        Some(mgr),
                        serde_json::json!(0),
                        &serde_json::json!({ "id": id }),
                    );
                    if resp.error.is_some() {
                        tracing::warn!("plugins modal: uninstall({id}) failed: {:?}", resp.error);
                    } else {
                        plugin::mark_builtin_removed(mgr, &id);
                    }
                }
            }
        }

        let snapshot = self.snapshot_plugins();
        if let Some(modal) = self.windows.get_mut(&modal_id) {
            if let Some(plugins_window) =
                modal.as_any_mut().downcast_mut::<window::PluginsWindow>()
            {
                plugins_window.refresh_snapshot(snapshot);
            }
        }
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
        let Some(mut modal) = self.windows.remove(&modal_id) else {
            return;
        };
        // If it was a settings modal, apply settings to all main windows
        if let Some(settings_modal) = modal.as_any_mut().downcast_mut::<window::SettingsWindow>() {
            let new_settings = settings_modal.settings.clone();
            // Plugin shortcut override draft 회수 — 변경된 키만 plugins.toml에 반영.
            let plugin_draft = settings_modal.take_plugin_shortcut_draft();
            for main in self.main_windows_iter_mut() {
                main.state.engine.settings = new_settings.clone();
                main.state.settings_open = false;
                main.mark_dirty();
            }
            if let Err(e) = new_settings.save() {
                tracing::warn!("failed to save settings: {e}");
            }
            self.apply_plugin_shortcut_draft(plugin_draft);
        } else if modal.as_any().is::<window::PluginsWindow>() {
            for main in self.main_windows_iter_mut() {
                main.state.plugins_open = false;
                main.mark_dirty();
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
            // ── Plugin management (App-level — App holds the PluginManager) ──
            if cmd.request.method.starts_with("plugin.") {
                let id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
                let response = match cmd.request.method.as_str() {
                    "plugin.list" => {
                        ipc::handler::plugin::handle_list(self.plugin_manager.as_ref(), id)
                    }
                    "plugin.install" => ipc::handler::plugin::handle_install(
                        self.plugin_manager.as_mut(),
                        id,
                        &cmd.request.params,
                    ),
                    "plugin.remove" => ipc::handler::plugin::handle_remove(
                        self.plugin_manager.as_mut(),
                        id,
                        &cmd.request.params,
                    ),
                    "plugin.enable" => ipc::handler::plugin::handle_enable(
                        self.plugin_manager.as_mut(),
                        id,
                        &cmd.request.params,
                    ),
                    "plugin.disable" => ipc::handler::plugin::handle_disable(
                        self.plugin_manager.as_mut(),
                        id,
                        &cmd.request.params,
                    ),
                    "plugin.permissions" => ipc::handler::plugin::handle_permissions(
                        self.plugin_manager.as_ref(),
                        id,
                        &cmd.request.params,
                    ),
                    "plugin.grant" => ipc::handler::plugin::handle_grant(
                        self.plugin_manager.as_mut(),
                        id,
                        &cmd.request.params,
                    ),
                    "plugin.revoke" => ipc::handler::plugin::handle_revoke(
                        self.plugin_manager.as_mut(),
                        id,
                        &cmd.request.params,
                    ),
                    other => ipc::protocol::JsonRpcResponse::method_not_found(id, other),
                };
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

            // Plugin namespace IPC: 메서드가 plugin이 contribute한 prefix에 매칭되면
            // owner plugin에 forward. 응답은 plugin이 줄 때까지 보류되며 main loop
            // 다음 tick에서 `plugin_manager.handle_plugin_response`가 client에 회신.
            // 정적/GUI 분기를 모두 통과하지 못한 메서드만 여기 도달하므로, plugin이
            // 호스트 명령을 가릴 수 없다.
            if let Some(mgr) = self.plugin_manager.as_mut() {
                if mgr.ipc_namespaces.resolve(&cmd.request.method).is_some() {
                    let id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
                    mgr.forward_namespace_call(
                        &cmd.request.method,
                        cmd.request.params.clone(),
                        None, // CLI/사용자 호출. plugin → plugin 호출은 step 04에서.
                        id,
                        cmd.response_tx.clone(),
                    );
                    processed = true;
                    continue;
                }
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

    /// plugin process가 보낸 IPC 호출들을 라우터로 디스패치하고 결과를 plugin에 회신.
    /// CallerContext::Plugin으로 들어가므로 권한 게이트가 적용된다.
    fn process_plugin_ipc_calls(&mut self) {
        let calls = match self.plugin_manager.as_mut() {
            Some(mgr) => mgr.take_pending_plugin_calls(),
            None => return,
        };
        for call in calls {
            let caller = ipc::caller::CallerContext::Plugin {
                plugin_id: call.plugin_id.clone(),
                permissions: call.permissions.clone(),
            };
            let request = ipc::protocol::JsonRpcRequest {
                jsonrpc: "2.0".to_string(),
                id: Some(serde_json::Value::from(call.call_id)),
                method: call.method.clone(),
                params: call.params.clone(),
            };
            let response = self.dispatch_with_caller(&request, &caller);
            let (result, error) = match response.error {
                Some(err) => (None, Some(err.message)),
                None => (response.result, None),
            };
            if let Some(mgr) = self.plugin_manager.as_mut() {
                mgr.send_ipc_result(&call.plugin_id, call.call_id, result, error);
            }
        }
    }

    /// 단계 F: focused surface가 plugin RemoteSurface인 경우 plugin command와
    /// 키 매칭. 매칭 성공 시 `dispatch_plugin_command`를 호출하고 `true` 반환 →
    /// 호출자(event_handler)는 normal window dispatch를 skip해 host action이
    /// trigger되지 않게 한다.
    pub(crate) fn try_plugin_shortcut(
        &mut self,
        id: winit::window::WindowId,
        ke: &winit::event::KeyEvent,
    ) -> bool {
        use winit::event::ElementState;
        if ke.state != ElementState::Pressed {
            return false;
        }
        // Modal이 활성화되면 plugin shortcut은 동작하지 않는다.
        if self.engine.is_modal_active() {
            return false;
        }
        let Some(w) = self.windows.get(&id) else {
            return false;
        };
        let Some(main) = w.as_main() else {
            return false;
        };
        // overlay/popup이 키를 가져갈 상태면 patcher
        if main.state.settings_open
            || main.state.has_input_dialog_open()
            || main.state.popups.has_focused()
        {
            return false;
        }
        let Some((plugin_id, surface_id)) =
            plugin::key_dispatch::focused_plugin_surface(&main.state)
        else {
            return false;
        };
        // physical key fallback (IME 영향 회피) — keyboard.rs와 동일 규칙
        let mods = main.base.modifiers;
        let shortcut_key = if mods.control_key() || mods.super_key() || mods.alt_key() {
            shortcuts::physical_key_to_logical(&ke.physical_key)
                .unwrap_or_else(|| ke.logical_key.clone())
        } else {
            ke.logical_key.clone()
        };
        let host_kb = main.state.engine.settings.keybindings.clone();
        let cmd_id = {
            let Some(mgr) = self.plugin_manager.as_ref() else {
                return false;
            };
            plugin::key_dispatch::match_plugin_shortcut(
                mgr,
                &plugin_id,
                &shortcut_key,
                mods,
                &host_kb,
            )
        };
        let Some(cmd_id) = cmd_id else {
            return false;
        };
        if let Some(mgr) = self.plugin_manager.as_mut() {
            plugin::key_dispatch::dispatch_plugin_command(mgr, &plugin_id, &cmd_id, surface_id);
        }
        true
    }

    /// caller를 명시한 라우터 디스패치. Plugin caller도 처리할 수 있도록 핸들러
    /// 진입점에 caller를 주입한다. 호스트 자체 메서드(window.*/plugin.*)는
    /// `process_ipc`가 별도로 처리하므로 여기서는 라우터에만 위임한다.
    fn dispatch_with_caller(
        &mut self,
        request: &ipc::protocol::JsonRpcRequest,
        caller: &ipc::caller::CallerContext,
    ) -> ipc::protocol::JsonRpcResponse {
        let focused_id = self.engine.focused_window_id;
        if let Some(id) = focused_id {
            if let Some(w) = self.windows.get_mut(&id).and_then(|w| w.as_main_mut()) {
                let response = ipc::handler::handle_with_caller(&mut w.state, request, caller);
                w.base.dirty = true;
                return response;
            }
        }
        if let Some(state) = self.parked_states.first_mut() {
            return ipc::handler::handle_with_caller(state, request, caller);
        }
        let id = request.id.clone().unwrap_or(serde_json::Value::Null);
        ipc::protocol::JsonRpcResponse::error(id, -32000, "no application state available")
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

    // Parse CLI arguments. 정적 `Cli`가 알 수 없는 서브커맨드라고 실패하면 plugin
    // CLI 동적 등록에서 한 번 더 매칭 시도. 정적이 항상 우선이므로 plugin이 호스트
    // 명령을 가릴 수 없다.
    let cli = match cli::Cli::try_parse() {
        Ok(cli) => cli,
        Err(err) => {
            if matches!(err.kind(), clap::error::ErrorKind::InvalidSubcommand) {
                if let Some(result) = cli::try_run_plugin_cli() {
                    return result;
                }
            }
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
            let mut last_text: Option<String> = None;
            loop {
                std::thread::sleep(interval);
                let Some(mut cb) = arboard::Clipboard::new().ok() else {
                    continue;
                };
                // Try text first, then image
                if let Ok(text) = cb.get_text() {
                    if !text.is_empty() {
                        let changed = last_text.as_ref() != Some(&text);
                        if changed {
                            last_text = Some(text.clone());
                            if tick_proxy
                                .send_event(AppEvent::ClipboardChanged(ClipboardData::Text(text)))
                                .is_err()
                            {
                                break;
                            }
                        }
                        continue;
                    }
                }
                if let Ok(img) = cb.get_image() {
                    if let Some(data) = event_handler::encode_clipboard_image(&img) {
                        last_text = None;
                        if tick_proxy
                            .send_event(AppEvent::ClipboardChanged(ClipboardData::Image(data)))
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }
        });
    }

    // 1초 간격 busy ticker. 메인 스레드가 받아서 모든 surface의 foreground
    // 프로세스를 다시 조회하고 캐시를 갱신한다. PID 조회 자체는 가볍지만
    // 매 프레임 호출하면 과하므로 별도 스레드에서 ticking만 한다.
    {
        let busy_proxy = proxy.clone();
        std::thread::spawn(move || {
            let interval = std::time::Duration::from_secs(1);
            loop {
                std::thread::sleep(interval);
                if busy_proxy.send_event(AppEvent::BusyPoll).is_err() {
                    break;
                }
            }
        });
    }

    // CWD는 OSC 7 시퀀스에만 의존한다. 모든 플랫폼 공통.
    // zsh/fish는 기본 지원, bash는 PROMPT_COMMAND 설정 필요.

    let mut app = App::new(
        proxy,
        cli.port_file,
        #[cfg(debug_assertions)]
        cli.enable_input_simulation,
    );
    event_loop.run_app(&mut app)?;

    Ok(())
}
