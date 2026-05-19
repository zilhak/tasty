#![allow(private_interfaces)]
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app_icon;
mod cli;
mod click_cursor;
mod clipboard_history;
mod command_index;
mod command_palette;
mod crash_report;
#[cfg(debug_assertions)]
mod debug_info;
mod diff_ui;
mod double_tap;
mod empty_ui;
pub mod engine;
pub mod engine_state;
mod event_handler;
mod file_dispatch;
mod file_drag;
mod file_format;
mod file_handler;
mod file_handler_recent;
mod file_handlers_save;
mod git_viewer;
mod global_hooks;
mod gpu;
mod html_ui;
mod identify_worker;
mod image_ui;
mod intent;
mod ipc;
mod layout_persistence;
#[cfg(windows)]
mod jump_list;
mod markdown_ui;
mod native_menu;
mod notification;
mod output_observer;
mod plugin;
mod plugins_ui;
mod preset_ui;
mod recent_files;
mod renderer;
mod scrollback_store;
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
mod update_check;
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
        if let Err(e) = self.inner.set_text(text.to_string()) {
            tracing::warn!("clipboard set_text failed: {e}");
        }
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
    /// 비동기 파일 식별 결과. `IdentifyWorker::spawn` 의 worker thread 가 완료 시 송신.
    /// 콜사이트(Phase C 의 mouse.rs 등) 는 보관한 마지막 `request_id` 와 매칭해
    /// 오래된 결과를 drop 한다.
    IdentifyDone {
        request_id: crate::identify_worker::IdentifyRequestId,
        target: crate::file_format::FileTarget,
        detector: Option<crate::file_format::DetectorId>,
    },
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
    /// 사용자 init.lua 기반 Lua hook 엔진. 부팅 시 1회 생성, `~/.tasty/init.lua` 가
    /// 있으면 로드. observe-only — 호스트 동작에는 영향 없음. 초기화 실패 시 None.
    lua_engine: Option<tasty_lua::LuaEngine>,
    /// 현재 열려 있는 `PresetWindow` 의 winit window id. modeless editor 윈도우는
    /// 엔진 전역 단일 인스턴스 — 같은 명령이 다시 들어오면 새 윈도우를 만들지 않고
    /// 이 id 의 윈도우로 포커스만 이동한다.
    preset_window_id: Option<WindowId>,
}

/// State for the modal window shake animation.
/// preset apply 시 Mutex 안에서 clone 한 preset 데이터. apply 본체는 mutex lock
/// 밖에서 `&mut AppState` 로 호출되므로 보더 케이스를 enum 하나로 묶는다.
enum ClonedPreset {
    Workspace(tasty_presets::WorkspacePreset),
    Tab(tasty_presets::TabPreset),
    Pane(tasty_presets::PanePreset),
}

struct ModalShake {
    start: std::time::Instant,
    /// Original window position before shake began.
    origin: winit::dpi::PhysicalPosition<i32>,
}

use winit::window::WindowId;

/// Phase 6.2c — envelope 의 `session_token` 필드를 보고 caller 를 결정한다.
///
/// - `session_token` 가 None → `CallerContext::Local`
/// - 형식이 잘못된 토큰(64-char hex 아님) → `Err(deny)`
/// - 유효 형식이지만 store 에 없음/만료/revoked → `Err(deny)` (Local fallback 금지)
/// - 유효 → `CallerContext::Agent { ... }`
///
/// memory store 가 초기화되지 않은 경우 (`with_store` 가 `None`): 토큰이 있어도
/// 검증 불가이므로 `Err(deny)` 로 막는다 — 부팅 초기의 가장된 호출을 막는다.
fn resolve_caller_from_envelope(
    request: &ipc::protocol::JsonRpcRequest,
) -> Result<ipc::caller::CallerContext, ipc::protocol::JsonRpcResponse> {
    use std::collections::HashSet;
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};

    let token_str = match request.session_token.as_deref() {
        None => return Ok(ipc::caller::CallerContext::Local),
        Some(s) => s,
    };
    let id = request.id.clone().unwrap_or(serde_json::Value::Null);
    let deny = |msg: &str| {
        ipc::protocol::JsonRpcResponse::error(
            id.clone(),
            -32001,
            &format!("permission_denied: {msg}"),
        )
    };
    let token = match ipc::caller::SessionToken::from_str(token_str) {
        Some(t) => t,
        None => return Err(deny("invalid session_token format (expect 64 hex chars)")),
    };
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let resolved = tasty_memory::with_store(|mem| {
        let mut store = ipc::session::SessionStore::new(mem, tasty_memory::HOST_OWNER);
        store.resolve(&token, now_ms)
    });
    let session = match resolved {
        None => return Err(deny("memory store not initialized")),
        Some(Err(e)) => return Err(deny(&format!("session lookup failed: {e}"))),
        Some(Ok(None)) => return Err(deny("session_token unknown/expired/revoked")),
        Some(Ok(Some(s))) => s,
    };
    let perms: HashSet<plugin::manifest::Permission> = session.permission_set();
    Ok(ipc::caller::CallerContext::Agent {
        agent_id: session.agent_id,
        parent: session.parent,
        permissions: Arc::new(perms),
        token,
    })
}

/// `ShortcutOverride`를 `command.shortcut_changed` payload용 단순 문자열로 변환.
/// `Key` 모드의 다중 키는 `, `로 join, `Inherit`는 `@source`로 표기, 비어 있거나
/// `None` 모드는 `None` 반환. 정확한 상태는 plugin이 IPC로 재조회한다.
fn shortcut_override_display(
    ov: Option<&plugin::registry_state::ShortcutOverride>,
) -> Option<String> {
    use plugin::registry_state::ShortcutOverride;
    match ov? {
        ShortcutOverride::Key { value } if !value.is_empty() => Some(value.join(", ")),
        ShortcutOverride::Key { .. } => None,
        ShortcutOverride::Inherit { source } => Some(format!("@{source}")),
        ShortcutOverride::None => None,
    }
}

/// debug 빌드 한정 — `debug.event_bus.*` IPC 처리. PluginManager의 EventBus를
/// 직접 조회/조작한다. release 빌드에는 컴파일되지 않는다.
#[cfg(debug_assertions)]
fn handle_debug_event_bus(
    mgr: Option<&mut plugin::PluginManager>,
    method: &str,
    params: &serde_json::Value,
    id: serde_json::Value,
) -> ipc::protocol::JsonRpcResponse {
    use ipc::protocol::JsonRpcResponse;
    let mgr = match mgr {
        Some(m) => m,
        None => return JsonRpcResponse::error(id, -32000, "plugin manager not initialized"),
    };
    match method {
        "debug.event_bus.list_subscribers" => {
            let key = match params.get("key").and_then(|v| v.as_str()) {
                Some(k) => k.to_string(),
                None => return JsonRpcResponse::invalid_params(id, "missing 'key'"),
            };
            let subs = mgr.event_bus.debug_list_subscribers(&key);
            let result: Vec<_> = subs
                .into_iter()
                .map(|(plugin_id, sub_id, pattern)| {
                    serde_json::json!({
                        "plugin_id": plugin_id,
                        "sub_id": sub_id,
                        "pattern": pattern,
                    })
                })
                .collect();
            JsonRpcResponse::success(id, serde_json::json!({ "subscribers": result }))
        }
        "debug.event_bus.publish" => {
            let key = match params.get("key").and_then(|v| v.as_str()) {
                Some(k) => k.to_string(),
                None => return JsonRpcResponse::invalid_params(id, "missing 'key'"),
            };
            let payload_str = params
                .get("payload")
                .and_then(|v| v.as_str())
                .unwrap_or("{}");
            let payload: serde_json::Value = match serde_json::from_str(payload_str) {
                Ok(v) => v,
                Err(e) => {
                    return JsonRpcResponse::invalid_params(
                        id,
                        &format!("invalid JSON payload: {e}"),
                    );
                }
            };
            let scope_str = params
                .get("scope")
                .and_then(|v| v.as_str())
                .unwrap_or("system");
            let scope = match scope_str {
                "system" => tasty_plugin_protocol::EventScope::System,
                "surface" => tasty_plugin_protocol::EventScope::Surface,
                other => {
                    return JsonRpcResponse::invalid_params(
                        id,
                        &format!("unknown scope '{other}' (expected 'system' or 'surface')"),
                    );
                }
            };
            let envelope = mgr.build_host_envelope(&key, &payload, scope);
            let trace_id = envelope.meta.trace_id.clone();
            mgr.publish_host_event(envelope);
            JsonRpcResponse::success(
                id,
                serde_json::json!({ "published": true, "trace_id": trace_id }),
            )
        }
        "debug.event_bus.trace" => {
            let trace_id = match params.get("trace_id").and_then(|v| v.as_str()) {
                Some(t) => t,
                None => return JsonRpcResponse::invalid_params(id, "missing 'trace_id'"),
            };
            let envelopes = mgr.event_bus.debug_trace(trace_id);
            let result: Vec<_> = envelopes
                .into_iter()
                .map(|e| serde_json::to_value(&e).unwrap_or(serde_json::Value::Null))
                .collect();
            JsonRpcResponse::success(id, serde_json::json!({ "envelopes": result }))
        }
        _ => JsonRpcResponse::method_not_found(id, method),
    }
}

/// debug 빌드 한정 — `debug.extension.invoke_hook` IPC.
/// extension에 hook을 직접 fire하고 응답을 그대로 caller에 회신한다.
/// 비동기: response_tx로 회신 (main loop의 handle_plugin_response가 처리).
#[cfg(debug_assertions)]
fn handle_debug_extension_invoke_hook(
    mgr: Option<&mut plugin::PluginManager>,
    params: &serde_json::Value,
    id: serde_json::Value,
    response_tx: std::sync::mpsc::SyncSender<ipc::protocol::JsonRpcResponse>,
) {
    use ipc::protocol::JsonRpcResponse;
    use ipc::server::send_response;
    let mgr = match mgr {
        Some(m) => m,
        None => {
            send_response(&response_tx, JsonRpcResponse::error(
                id,
                -32000,
                "plugin manager not initialized",
            ));
            return;
        }
    };
    let extension_id = match params.get("extension_id").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => {
            send_response(&response_tx, JsonRpcResponse::invalid_params(id, "missing 'extension_id'"));
            return;
        }
    };
    let kind = match params.get("kind").and_then(|v| v.as_str()) {
        Some("event") => tasty_plugin_protocol::ExtensionHookKind::Event,
        Some("ipc") => tasty_plugin_protocol::ExtensionHookKind::Ipc,
        _ => {
            send_response(&response_tx, JsonRpcResponse::invalid_params(
                id,
                "missing/invalid 'kind' (expected 'event' or 'ipc')",
            ));
            return;
        }
    };
    let phase = match params.get("phase").and_then(|v| v.as_str()) {
        Some("pre") => tasty_plugin_protocol::ExtensionHookPhase::Pre,
        Some("post") => tasty_plugin_protocol::ExtensionHookPhase::Post,
        _ => {
            send_response(&response_tx, JsonRpcResponse::invalid_params(
                id,
                "missing/invalid 'phase' (expected 'pre' or 'post')",
            ));
            return;
        }
    };
    let mode = match params.get("mode").and_then(|v| v.as_str()) {
        Some("transform") => crate::plugin::manifest::HookMode::Transform,
        Some("filter") => crate::plugin::manifest::HookMode::Filter,
        Some("observe") => crate::plugin::manifest::HookMode::Observe,
        _ => {
            send_response(&response_tx, JsonRpcResponse::invalid_params(
                id,
                "missing/invalid 'mode' (expected 'transform', 'filter', or 'observe')",
            ));
            return;
        }
    };
    let target = match params.get("target").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => {
            send_response(&response_tx, JsonRpcResponse::invalid_params(id, "missing 'target'"));
            return;
        }
    };
    let payload = params
        .get("payload")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    mgr.debug_invoke_extension_hook(
        &extension_id,
        kind,
        phase,
        mode,
        &target,
        payload,
        id,
        response_tx,
    );
}

/// Lua hook 1회 발사 헬퍼. lua 가 None 이거나 직렬화 실패 시 silent no-op.
pub(crate) fn fire_lua<T: serde::Serialize>(
    lua: Option<&tasty_lua::LuaEngine>,
    event: &str,
    payload: &T,
) {
    if let Some(lua) = lua {
        match serde_json::to_value(payload) {
            Ok(v) => lua.fire(event, &v),
            Err(e) => {
                tracing::warn!(target: "tasty_lua", "fire '{event}' serialize failed: {e}")
            }
        }
    }
}

/// Lua hook 엔진 부트스트랩. `~/.tasty/init.lua` 가 있으면 로드.
/// 초기화/로드 실패는 warn 로만 남기고 None 반환 — 호스트 부팅을 막지 않는다.
fn init_lua_engine() -> Option<tasty_lua::LuaEngine> {
    let mut engine = match tasty_lua::LuaEngine::new() {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!("lua engine init failed: {e}");
            return None;
        }
    };
    if let Some(home) = tasty_core::paths::tasty_home() {
        let init_path = home.join("init.lua");
        match engine.load_init(&init_path) {
            Ok(true) => tracing::info!(
                target: "tasty_lua",
                "loaded init.lua from {}",
                init_path.display(),
            ),
            Ok(false) => {}
            Err(e) => tracing::warn!("lua: failed to load init.lua: {e}"),
        }
    }
    Some(engine)
}

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
            lua_engine: init_lua_engine(),
            preset_window_id: None,
        }
    }

    /// PresetWindow 를 연다. 이미 열려 있으면 새 윈도우를 만들지 않고 기존 윈도우에
    /// 포커스만 옮긴다 (엔진 전역 단일 인스턴스).
    fn open_preset_window(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if let Some(id) = self.preset_window_id {
            if let Some(w) = self.windows.get(&id) {
                w.base().winit.focus_window();
                return;
            }
            self.preset_window_id = None;
        }

        use winit::window::WindowAttributes;
        let mut attrs = WindowAttributes::default()
            .with_title(crate::i18n::t("preset.window.title"))
            .with_inner_size(winit::dpi::LogicalSize::new(960, 640))
            .with_min_inner_size(winit::dpi::LogicalSize::new(760, 480))
            .with_visible(false);
        if let Some(icon) = crate::app_icon::winit_window_icon() {
            attrs = attrs.with_window_icon(Some(icon));
        }
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                tracing::warn!("failed to create preset window: {e}");
                return;
            }
        };

        let appearance = self
            .focused_window()
            .map(|w| w.state.engine.settings.appearance.clone())
            .unwrap_or_else(|| crate::settings::Settings::load().appearance);
        let gpu = match pollster::block_on(crate::gpu::GpuState::new(
            window.clone(),
            &appearance,
            self.engine.proxy.clone(),
        )) {
            Ok(g) => g,
            Err(e) => {
                tracing::warn!("failed to init GPU for preset window: {e}");
                return;
            }
        };

        let store = std::sync::Arc::clone(&self.engine.preset_store);
        let window_id = window.id();
        let mut preset = window::PresetWindow::new(gpu, window, store);
        #[cfg(windows)]
        {
            use window::Window as _;
            preset.render();
        }
        #[cfg(not(windows))]
        {
            use window::Window as _;
            preset.mark_dirty();
        }
        self.windows.insert(window_id, Box::new(preset));
        self.preset_window_id = Some(window_id);
        tracing::info!("opened preset window {:?}", window_id);
    }

    /// PresetWindow close 시 정리. store 는 Arc<Mutex<>> 공유라 별도 회수 불필요.
    fn on_preset_window_closed(&mut self, window_id: WindowId) {
        if self.preset_window_id != Some(window_id) {
            return;
        }
        self.preset_window_id = None;
        self.windows.remove(&window_id);
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
        // 비동기 파일 식별 worker. file_format Arc 를 공유하므로 plugin contribute /
        // user reload 변경이 worker 호출에도 그대로 반영된다.
        state.engine.identify_worker = Some(Arc::new(crate::identify_worker::IdentifyWorker::new(
            state.engine.file_format.clone(),
            self.engine.proxy.clone(),
        )));

        // 첫 윈도우 생성 시 plugin manager 한 번만 초기화.
        if self.plugin_manager.is_none() {
            // EngineState 와 같은 file_format / file_handler Arc 를 공유해
            // plugin enable/disable 시 EngineState 가 보유한 registry 가 그대로 갱신되도록 한다.
            let mut mgr = plugin::PluginManager::with_registries(
                factory,
                state.engine.file_format.clone(),
                state.engine.file_handler.clone(),
            );
            mgr.set_surface_registry(state.engine.surface_registry.clone());
            // 기본 제공 플러그인이 설치되지 않았으면 번들에서 복사. 사용자가
            // 명시적으로 제거한 항목 (`removed_builtins`)은 건드리지 않는다.
            plugin::install_builtins_if_needed(&mut mgr);
            mgr.packages = plugin::discover();
            mgr.discover_and_start();
            state.tool_registry.set_plugin_items(mgr.plugin_tool_items());
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
                // AppState::new 시점에는 layout이 아직 복원되지 않아 state.active_workspace=0.
                // restore가 끝난 지금 실제 활성 인덱스로 sync해야 사용자가 보는 화면이
                // 일치한다 (sync 없으면 첫 화면이 비활성 workspace[0]의 deferred
                // placeholder들로 채워진다).
                if let Some(restored_idx) = state.engine.restored_active_workspace.take() {
                    state.switch_workspace(restored_idx);
                }
            }
        }
        #[cfg(debug_assertions)]
        {
            state.engine.input_simulation_enabled = self.input_simulation_enabled;
        }
        // App 의 preset_store Arc 를 EngineState 에 공유. apply popup / 우클릭 저장 등이
        // MainWindow 컨텍스트에서 lock 한 번으로 직접 접근할 수 있게 한다.
        state.engine.preset_store = Some(self.engine.preset_store.clone());
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
        if let Some(mgr) = self.plugin_manager.as_mut() {
            use tasty_plugin_protocol::EventScope;
            use tasty_plugin_protocol::events::payloads::{WindowCreated, WindowModality};
            let payload = WindowCreated {
                window_id: u64::from(window_id),
                kind: "main".to_string(),
                modality: WindowModality::Modeless,
            };
            mgr.emit_host_event("window.created", &payload, EventScope::System);
            fire_lua(self.lua_engine.as_ref(), "window.create.post", &payload);
        }
    }

    /// Initialize the full app state (terminal, IPC server, etc.) after shell is confirmed.
    fn init_app_state(
        &mut self,
        window: Arc<Window>,
        gpu: GpuState,
        settings: crate::settings::Settings,
    ) {
        // state.db / memory.db 초기화는 create_app_state 이전에 반드시 호출.
        // 첫 윈도우는 `create_new_window` 를 거치지 않고 곧장 이 함수로 진입하므로
        // 여기서도 호출이 필요하다. 두 init 모두 OnceLock 기반 idempotent.
        let db_init_error = crate::storage::init().err();

        let memory_config = tasty_memory::MemoryConfig {
            entry_max_bytes: settings.memory.entry_max_mb.saturating_mul(1024 * 1024),
            secret_quota_per_owner_bytes: settings
                .memory
                .secret_quota_mb_per_plugin
                .saturating_mul(1024 * 1024),
            regular_quota_total_bytes: settings
                .memory
                .regular_quota_mb_total
                .saturating_mul(1024 * 1024),
        };
        if let Err(e) = tasty_memory::init_with_config(memory_config) {
            tracing::warn!("memory.db init failed: {e}");
        }

        let mut state = self.create_app_state(&gpu, settings.appearance.sidebar_width);

        // DB 초기화 실패 알림 — create_new_window 와 동일하게 InfoModal 로 안내 후 Exit(1).
        if let Some(err) = db_init_error {
            tracing::error!("state.db init failed: {err}");
            let (key, args) = err.user_message_i18n();
            let body = match args.len() {
                0 => crate::i18n::t(key).to_string(),
                1 => crate::i18n::t_fmt(key, &args[0]),
                _ => crate::i18n::t_fmt2(key, &args[0], &args[1]),
            };
            crate::ui::info_modal::show_info_modal(
                &mut state,
                crate::ui::info_modal::InfoModal {
                    title: crate::i18n::t("db_error.title").to_string(),
                    body,
                    on_close: crate::ui::info_modal::InfoModalAction::Exit(1),
                },
            );
        }

        self.engine.start_ipc();
        self.register_window(gpu, state, window);
        // Event Bus 1.0: `system.startup_complete`는 부팅 완료 직후 1회 발화.
        // init_app_state는 첫 윈도우 등록 시 한 번만 호출되므로 별도 once 가드 불필요.
        if let Some(mgr) = self.plugin_manager.as_mut() {
            use tasty_plugin_protocol::EventScope;
            use tasty_plugin_protocol::events::payloads::SystemStartupComplete;
            mgr.emit_host_event(
                "system.startup_complete",
                &SystemStartupComplete::default(),
                EventScope::System,
            );
        }
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

        // state.db 초기화. 실패하면 InfoModal로 안내 후 종료(Exit 1).
        // create_app_state 이전에 호출해야 plugin/recent_files 등이 정상 동작.
        let db_init_error = crate::storage::init().err();

        let mut settings = crate::settings::Settings::load();

        // memory.db 초기화. state.db와 독립 파일(~/.tasty/memory.db). 현재는
        // 에이전트 memory.* IPC만 의존하므로 실패해도 앱을 종료시키지 않는다 —
        // 핸들러가 호출 시점에 "store not initialized"를 응답한다. 1.5에서
        // surface.meta.* 포워딩이 들어가면 정책 재검토.
        let memory_config = tasty_memory::MemoryConfig {
            entry_max_bytes: settings.memory.entry_max_mb.saturating_mul(1024 * 1024),
            secret_quota_per_owner_bytes: settings
                .memory
                .secret_quota_mb_per_plugin
                .saturating_mul(1024 * 1024),
            regular_quota_total_bytes: settings
                .memory
                .regular_quota_mb_total
                .saturating_mul(1024 * 1024),
        };
        if let Err(e) = tasty_memory::init_with_config(memory_config) {
            tracing::warn!("memory.db init failed: {e}");
        }

        // Apply saved theme preset at startup. theme 이름이 preset에 없으면
        // catppuccin-mocha로 fallback하고 사용자에게 InfoModal로 알린다.
        let presets = crate::theme::presets();
        let invalid_theme_name = if let Some(preset) =
            presets.iter().find(|p| p.id == settings.appearance.theme)
        {
            crate::theme::set_theme(preset.theme);
            None
        } else {
            let invalid = settings.appearance.theme.clone();
            let fallback_id = "catppuccin-mocha";
            settings.appearance.theme = fallback_id.to_string();
            if let Some(default_preset) = presets.iter().find(|p| p.id == fallback_id) {
                crate::theme::set_theme(default_preset.theme);
            }
            Some(invalid)
        };
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

        // DB 초기화 실패 알림. 가장 먼저 푸시해서 큐 head에 둠 → [확인] 시 Exit(1).
        if let Some(err) = db_init_error {
            tracing::error!("state.db init failed: {err}");
            let (key, args) = err.user_message_i18n();
            let body = match args.len() {
                0 => crate::i18n::t(key).to_string(),
                1 => crate::i18n::t_fmt(key, &args[0]),
                _ => crate::i18n::t_fmt2(key, &args[0], &args[1]),
            };
            crate::ui::info_modal::show_info_modal(
                &mut state,
                crate::ui::info_modal::InfoModal {
                    title: crate::i18n::t("db_error.title").to_string(),
                    body,
                    on_close: crate::ui::info_modal::InfoModalAction::Exit(1),
                },
            );
        }

        // Theme fallback 알림 (잘못된 theme 이름이었던 경우).
        if let Some(invalid) = invalid_theme_name {
            crate::ui::info_modal::show_info_modal(
                &mut state,
                crate::ui::info_modal::InfoModal {
                    title: crate::i18n::t("theme_error.title").to_string(),
                    body: crate::i18n::t_fmt("theme_error.body", &invalid),
                    on_close: crate::ui::info_modal::InfoModalAction::Continue,
                },
            );
        }

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
        let (file_format, file_handler) = if let Some(w) = self.focused_window() {
            (
                w.state.engine.file_format.clone(),
                w.state.engine.file_handler.clone(),
            )
        } else {
            // Settings 윈도우가 main 창 없이 열리는 경로는 거의 없지만, fallback 으로 빈 registry 를 만든다.
            // 이 경로에서는 Settings 의 FileHandler 탭이 비어 보이고 저장도 의미가 없다.
            (
                Arc::new(crate::file_format::FileFormatRegistry::new()),
                Arc::new(crate::file_handler::FileHandlerRegistry::new()),
            )
        };
        let user_config_path = tasty_core::paths::tasty_home().map(|d| d.join("file-handlers.toml"));
        let mut modal = window::SettingsWindow::new(
            gpu,
            window,
            settings,
            file_format,
            file_handler,
            user_config_path,
        );
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
        let mut emit_queue: Vec<(String, String, Option<String>, Option<String>)> = Vec::new();
        for ((plugin_id, command_id), value) in draft {
            let prev_display = shortcut_override_display(
                mgr.config.shortcut_override(&plugin_id, &command_id),
            );
            let new_display = match &value {
                Some(ov) => shortcut_override_display(Some(ov)),
                None => None,
            };
            let local_changed = match value {
                Some(ov) => {
                    mgr.config.set_shortcut_override(&plugin_id, &command_id, ov);
                    true
                }
                None => mgr.config.clear_shortcut_override(&plugin_id, &command_id),
            };
            if local_changed {
                changed = true;
                emit_queue.push((plugin_id, command_id, new_display, prev_display));
            }
        }
        if changed {
            if let Err(e) = mgr.config.save() {
                tracing::warn!("plugins.toml save failed after shortcut update: {e}");
            }
            for (plugin_id, command_id, shortcut, prev_shortcut) in emit_queue {
                use tasty_plugin_protocol::EventScope;
                use tasty_plugin_protocol::events::payloads::CommandShortcutChanged;
                let payload = CommandShortcutChanged {
                    plugin_id,
                    command_id,
                    shortcut,
                    prev_shortcut,
                };
                mgr.emit_host_event(
                    "command.shortcut_changed",
                    &payload,
                    EventScope::System,
                );
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
                    install_dir: pkg.dir.to_string_lossy().into_owned(),
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

    /// PluginManager의 현재 `plugin_tool_items()`를 모든 MainWindow의 AppState로
    /// 푸시한다. plugin 라이프사이클 변경 후(install/enable/disable/grant ui.tool_item
    /// /revoke ui.tool_item/uninstall) 호출해야 사이드바 도구 메뉴가 갱신된다.
    fn refresh_tool_registry(&mut self) {
        let items = match self.plugin_manager.as_ref() {
            Some(mgr) => mgr.plugin_tool_items(),
            None => return,
        };
        for main in self.main_windows_iter_mut() {
            main.state.tool_registry.set_plugin_items(items.clone());
        }
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

        let mut pending_toasts: Vec<(String, crate::ui::ToastKind)> = Vec::new();

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
                plugins_ui::PluginsAction::OpenInstallDir { path } => {
                    if !crate::terminal_link::open_uri(&path) {
                        tracing::warn!("plugins modal: open install dir failed: {path}");
                    }
                }
                plugins_ui::PluginsAction::Install { src_path } => {
                    let resp = ipc::handler::plugin::handle_install(
                        Some(mgr),
                        serde_json::json!(0),
                        &serde_json::json!({ "path": src_path }),
                    );
                    pending_toasts.push(match (resp.error, resp.result) {
                        (Some(err), _) => (
                            crate::i18n::t_fmt("plugins.add_install_failed", &err.message),
                            crate::ui::ToastKind::Error,
                        ),
                        (None, Some(result)) => {
                            let installed = result
                                .get("installed")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            (
                                crate::i18n::t_fmt("plugins.add_installed", &installed),
                                crate::ui::ToastKind::Success,
                            )
                        }
                        (None, None) => (
                            crate::i18n::t_fmt("plugins.add_install_failed", "unknown error"),
                            crate::ui::ToastKind::Error,
                        ),
                    });
                }
            }
        }

        // 모든 lifecycle action 이후 도구 메뉴를 갱신. install/enable/disable/grant/
        // revoke/uninstall 어떤 경로든 ui.tool_item 권한 또는 plugin 활성 상태가
        // 바뀌었을 수 있으므로 매번 다시 수집한다 (low-cost).
        self.refresh_tool_registry();

        let snapshot = self.snapshot_plugins();
        if let Some(modal) = self.windows.get_mut(&modal_id) {
            if let Some(plugins_window) =
                modal.as_any_mut().downcast_mut::<window::PluginsWindow>()
            {
                plugins_window.refresh_snapshot(snapshot);
                for (msg, kind) in pending_toasts {
                    plugins_window.push_toast(msg, kind);
                }
            }
        }
    }

    /// 도구 메뉴 클릭 등 MainWindow 가 enqueue 한 "PresetWindow 그냥 열기" 요청 drain.
    fn process_pending_open_preset_window(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
    ) {
        let mut requested = false;
        for w in self.main_windows_iter_mut() {
            if w.state.dialogs.pending_open_preset_window {
                w.state.dialogs.pending_open_preset_window = false;
                requested = true;
            }
        }
        if requested {
            self.open_preset_window(event_loop);
        }
    }

    /// 단축키/picker 가 enqueue 한 preset 적용 요청을 처리한다.
    /// 1프레임에 최대 1개 (picker 가 동시에 여러 개 enqueue 할 수 없음).
    fn process_pending_preset_apply(&mut self) {
        use crate::state::{PendingPresetApply, preset_apply::ApplyOptions};
        use tasty_presets::PresetKind;

        let mut request: Option<(WindowId, PendingPresetApply)> = None;
        for (id, w) in self.windows.iter_mut() {
            if let Some(main) = w.as_main_mut() {
                if let Some(req) = main.state.dialogs.pending_preset_apply.take() {
                    request = Some((*id, req));
                    break;
                }
            }
        }
        let Some((source_id, req)) = request else {
            return;
        };

        // preset 데이터를 lock 안에서 clone 해 두고, apply 는 lock 밖에서 수행.
        let (kind, cloned): (PresetKind, Option<ClonedPreset>) = {
            let store = match self.engine.preset_store.lock() {
                Ok(g) => g,
                Err(poisoned) => {
                    tracing::warn!("preset_store mutex poisoned; recovering");
                    poisoned.into_inner()
                }
            };
            match &req {
                PendingPresetApply::Workspace(name) => (
                    PresetKind::Workspace,
                    store.get_workspace(name).cloned().map(ClonedPreset::Workspace),
                ),
                PendingPresetApply::Tab(name) => (
                    PresetKind::Tab,
                    store.get_tab(name).cloned().map(ClonedPreset::Tab),
                ),
                PendingPresetApply::Pane(name) => (
                    PresetKind::Pane,
                    store.get_pane(name).cloned().map(ClonedPreset::Pane),
                ),
            }
        };

        let Some(main) = self
            .windows
            .get_mut(&source_id)
            .and_then(|w| w.as_main_mut())
        else {
            return;
        };

        let result: Result<(), crate::state::preset_apply::ApplyError> = match cloned {
            Some(ClonedPreset::Workspace(p)) => main
                .state
                .apply_workspace_preset(&p, ApplyOptions { focus: true })
                .map(|_| ()),
            Some(ClonedPreset::Tab(p)) => main
                .state
                .apply_tab_preset(&p, None, ApplyOptions { focus: true })
                .map(|_| ()),
            Some(ClonedPreset::Pane(p)) => main
                .state
                .apply_pane_preset(&p, None, ApplyOptions { focus: true })
                .map(|_| ()),
            None => Err(crate::state::preset_apply::ApplyError::Empty),
        };

        if let Err(e) = &result {
            tracing::warn!("preset apply failed: {e}");
            main.state.toasts.push(
                crate::i18n::t("preset.toast.apply_failed"),
                crate::ui::ToastKind::Error,
                crate::ui::ToastScope::Window,
            );
        }
        let _ = kind;
        main.mark_dirty();
    }

    /// MainWindow 가 우클릭으로 enqueue 한 preset 저장 요청을 처리한다.
    /// store 의 unique_name 으로 충돌 회피 → save_* → 토스트 → PresetWindow 오픈 + select.
    /// 한 번에 1개 요청만 처리 (컨텍스트 메뉴는 modal 이라 동시 클릭 불가).
    fn process_pending_preset_save(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        use tasty_presets::PresetKind;

        let mut request: Option<(WindowId, crate::state::PendingPresetSave)> = None;
        for (id, w) in self.windows.iter_mut() {
            if let Some(main) = w.as_main_mut() {
                if let Some(req) = main.state.dialogs.pending_preset_save.take() {
                    request = Some((*id, req));
                    break;
                }
            }
        }
        let Some((source_id, req)) = request else {
            return;
        };

        let (kind, save_result, name) = {
            let mut store = match self.engine.preset_store.lock() {
                Ok(g) => g,
                Err(poisoned) => {
                    tracing::warn!("preset_store mutex poisoned; recovering");
                    poisoned.into_inner()
                }
            };
            match req {
                crate::state::PendingPresetSave::Workspace { base_name, mut preset } => {
                    let name = store.unique_name(PresetKind::Workspace, &base_name);
                    preset.name = name.clone();
                    let res = store.save_workspace(preset);
                    (PresetKind::Workspace, res.map(|_| ()), name)
                }
                crate::state::PendingPresetSave::Tab { base_name, mut preset } => {
                    let name = store.unique_name(PresetKind::Tab, &base_name);
                    preset.name = name.clone();
                    let res = store.save_tab(preset);
                    (PresetKind::Tab, res.map(|_| ()), name)
                }
                crate::state::PendingPresetSave::Pane { base_name, mut preset } => {
                    let name = store.unique_name(PresetKind::Pane, &base_name);
                    preset.name = name.clone();
                    let res = store.save_pane(preset);
                    (PresetKind::Pane, res.map(|_| ()), name)
                }
            }
        };

        // 결과 토스트는 요청을 보낸 MainWindow 에 푸시.
        let toast_key = match (&save_result, kind) {
            (Ok(_), PresetKind::Workspace) => "preset.toast.saved_workspace",
            (Ok(_), PresetKind::Tab) => "preset.toast.saved_tab",
            (Ok(_), PresetKind::Pane) => "preset.toast.saved_pane",
            (Err(_), _) => "preset.toast.save_failed",
        };
        let toast_kind = if save_result.is_ok() {
            crate::ui::ToastKind::Info
        } else {
            crate::ui::ToastKind::Error
        };
        if let Some(main) = self
            .windows
            .get_mut(&source_id)
            .and_then(|w| w.as_main_mut())
        {
            main.state.toasts.push(
                crate::i18n::t(toast_key),
                toast_kind,
                crate::ui::ToastScope::Window,
            );
            main.mark_dirty();
        }

        match save_result {
            Ok(_) => {
                self.open_preset_window(event_loop);
                if let Some(pwid) = self.preset_window_id {
                    if let Some(pw) = self
                        .windows
                        .get_mut(&pwid)
                        .and_then(|w| w.as_any_mut().downcast_mut::<window::PresetWindow>())
                    {
                        pw.select(kind, name);
                    }
                }
            }
            Err(e) => {
                tracing::warn!("preset save failed: {e}");
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
            // theme/language 변경 감지용 prev 값 — 첫 main window의 현재 설정 기준.
            // SettingsWindow는 단일 SoT라 prev/new는 글로벌 비교로 충분.
            let prev_theme = self
                .main_windows_iter_mut()
                .next()
                .map(|w| w.state.engine.settings.appearance.theme.clone());
            let prev_language = self
                .main_windows_iter_mut()
                .next()
                .map(|w| w.state.engine.settings.general.language.clone());
            for main in self.main_windows_iter_mut() {
                main.state.engine.settings = new_settings.clone();
                main.state.settings_open = false;
                main.mark_dirty();
            }
            if let Err(e) = new_settings.save() {
                tracing::warn!("failed to save settings: {e}");
            }
            self.apply_plugin_shortcut_draft(plugin_draft);
            // Event Bus 1.0: theme/language 변경 발화.
            if let Some(mgr) = self.plugin_manager.as_mut() {
                use tasty_plugin_protocol::EventScope;
                use tasty_plugin_protocol::events::payloads::{LanguageChanged, ThemeChanged};
                if prev_theme.as_deref() != Some(new_settings.appearance.theme.as_str()) {
                    mgr.emit_host_event(
                        "theme.changed",
                        &ThemeChanged {
                            theme_id: new_settings.appearance.theme.clone(),
                        },
                        EventScope::System,
                    );
                }
                if prev_language.as_deref() != Some(new_settings.general.language.as_str()) {
                    mgr.emit_host_event(
                        "language.changed",
                        &LanguageChanged {
                            language_code: new_settings.general.language.clone(),
                        },
                        EventScope::System,
                    );
                }
            }
        } else if modal.as_any().is::<window::PluginsWindow>() {
            for main in self.main_windows_iter_mut() {
                main.state.plugins_open = false;
                main.mark_dirty();
            }
        }
    }

    /// Process pending IPC commands. Returns true if any commands were processed.
    fn process_ipc(&mut self) -> bool {
        use crate::ipc::server::send_response;
        let ipc = match &self.engine.ipc_server {
            Some(ipc) => ipc,
            None => return false,
        };

        let mut processed = false;
        let mut tool_registry_dirty = false;
        while let Ok(cmd) = ipc.try_recv() {
            // Phase 6.2c — envelope 의 session_token 을 검증해 caller 결정.
            // 토큰이 없으면 Local. 있는데 invalid/expired/revoked 면 permission_denied
            // 로 즉시 거부 (Local 로 fallback 하지 않는다 — 위조 방어).
            // 검증을 통과한 caller 가 부적격 메서드(local_only 등)를 호출하면
            // ensure_allowed 가 한 단계 위에서 차단한다.
            let caller = match resolve_caller_from_envelope(&cmd.request) {
                Ok(c) => c,
                Err(resp) => {
                    send_response(&cmd.response_tx, resp);
                    processed = true;
                    continue;
                }
            };
            // Agent caller 는 모든 app-level/local-only 분기를 호출하면 안 된다.
            // method_meta 기반으로 한 번에 차단하고, 통과한 경우에만 분기를 본다.
            // Local caller 는 이전과 동일하게 통과.
            if !matches!(caller, ipc::caller::CallerContext::Local) {
                if let Err(e) = caller.ensure_allowed(&cmd.request.method) {
                    tracing::warn!("ipc agent caller denied: {e}");
                    let rpc_id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
                    // Phase 6.5a audit: app-level dispatcher 의 deny 도 기록.
                    if let Some(st) = self
                        .windows
                        .values()
                        .find_map(|w| w.as_main().map(|m| &m.state))
                    {
                        let ws = st
                            .engine
                            .workspaces
                            .get(st.active_workspace)
                            .map(|w| w.id);
                        let seq = st.engine.telemetry_seq.next();
                        ipc::audit::record(
                            &caller,
                            &cmd.request.method,
                            ipc::audit::AuditDecision::Deny,
                            Some(&format!("{e}")),
                            ws,
                            seq,
                        );
                    }
                    // Phase 6.4a — Agent caller 의 MissingPermission 은 elevation
                    // 발행. NotPluginCallable/UnknownMethod 는 elevation 으로
                    // 회복되지 않으므로 단순 deny.
                    let mut data = serde_json::json!(null);
                    if let (
                        ipc::caller::CallerError::MissingPermission { permission, .. },
                        ipc::caller::CallerContext::Agent { agent_id, .. },
                    ) = (&e, &caller)
                    {
                        let agent_id = agent_id.clone();
                        let perm_token = permission.as_token();
                        let method = cmd.request.method.clone();
                        let main_state = self
                            .windows
                            .values_mut()
                            .find_map(|w| w.as_main_mut().map(|m| &mut m.state));
                        if let Some(st) = main_state {
                            if let Some(rec) = ipc::handler::approval::publish_capability_elevation(
                                st,
                                &agent_id,
                                &method,
                                &perm_token,
                                None,
                            ) {
                                data = serde_json::json!({
                                    "kind": "capability_elevation",
                                    "approval_id": rec.request.id,
                                    "permission": perm_token,
                                    "method": method,
                                });
                            }
                        }
                    }
                    let mut response = ipc::protocol::JsonRpcResponse::error(
                        rpc_id,
                        -32001,
                        &format!("permission_denied: {e}"),
                    );
                    if !data.is_null()
                        && let Some(err) = response.error.as_mut()
                    {
                        err.data = Some(data);
                    }
                    send_response(&cmd.response_tx, response);
                    processed = true;
                    continue;
                }
            }
            // App-level IPC methods (don't need focused window)
            #[cfg(debug_assertions)]
            if cmd.request.method == "system.shutdown" {
                let response = ipc::protocol::JsonRpcResponse::success(
                    cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
                    serde_json::json!({"shutdown": true}),
                );
                send_response(&cmd.response_tx, response);
                crate::shortcuts::send_app_event(&self.engine.proxy, AppEvent::Shutdown);
                return true;
            }
            if cmd.request.method == "script.reload" {
                let resp_id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
                let response = match self.lua_engine.as_mut() {
                    None => ipc::protocol::JsonRpcResponse::error(
                        resp_id,
                        -32603,
                        "lua engine not initialized",
                    ),
                    Some(engine) => match engine.reload() {
                        Ok(loaded) => ipc::protocol::JsonRpcResponse::success(
                            resp_id,
                            serde_json::json!({ "loaded": loaded }),
                        ),
                        Err(e) => ipc::protocol::JsonRpcResponse::error(
                            resp_id,
                            -32603,
                            &format!("lua reload failed: {e}"),
                        ),
                    },
                };
                send_response(&cmd.response_tx, response);
                processed = true;
                continue;
            }
            if cmd.request.method == "window.create" {
                crate::shortcuts::send_app_event(&self.engine.proxy, AppEvent::CreateWindow);
                let response = ipc::protocol::JsonRpcResponse::success(
                    cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
                    serde_json::json!({"scheduled": true}),
                );
                send_response(&cmd.response_tx, response);
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
                send_response(&cmd.response_tx, response);
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
                send_response(&cmd.response_tx, response);
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
                    "plugin.show" => ipc::handler::plugin::handle_show(
                        self.plugin_manager.as_ref(),
                        id,
                        &cmd.request.params,
                    ),
                    "plugin.extension.list" => {
                        ipc::handler::plugin::handle_extension_list(
                            self.plugin_manager.as_ref(),
                            id,
                        )
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
                    "plugin.grant_agent_permission" => {
                        ipc::handler::session::handle_grant_agent_permission(
                            id,
                            &cmd.request.params,
                        )
                    }
                    "plugin.revoke_agent_permission" => {
                        ipc::handler::session::handle_revoke_agent_permission(
                            id,
                            &cmd.request.params,
                        )
                    }
                    "plugin.list_agent_permissions" => {
                        ipc::handler::session::handle_list_agent_permissions(
                            id,
                            &cmd.request.params,
                        )
                    }
                    "plugin.audit_query" => {
                        ipc::handler::audit::handle_query(id, &cmd.request.params)
                    }
                    "plugin.audit_summary" => {
                        ipc::handler::audit::handle_summary(id, &cmd.request.params)
                    }
                    "plugin.audit_follow" => {
                        ipc::handler::audit::handle_follow(id, &cmd.request.params)
                    }
                    "plugin.audit_clear" => {
                        ipc::handler::audit::handle_clear(id, &cmd.request.params)
                    }
                    "plugin.request_permission" => {
                        // 첫 main window 의 state 를 빌려 사용 (모든 window 가
                        // 같은 approval_store Arc 공유). main 이 하나도 없으면
                        // elevation popup 표시 자체가 의미 없으므로 internal_error.
                        let main_state = self
                            .windows
                            .values_mut()
                            .find_map(|w| w.as_main_mut().map(|m| &mut m.state));
                        match main_state {
                            Some(st) => {
                                ipc::handler::session::handle_request_permission(
                                    st,
                                    &caller,
                                    id,
                                    &cmd.request.params,
                                )
                            }
                            None => ipc::protocol::JsonRpcResponse::error(
                                id,
                                -32603,
                                "no main window available for elevation popup",
                            ),
                        }
                    }
                    other => ipc::protocol::JsonRpcResponse::method_not_found(id, other),
                };
                // plugin 라이프사이클이 바뀌었을 수 있는 메서드만 도구 메뉴 재집계
                // 표시. list/show/permissions/extension.list는 read-only이므로 skip.
                // (실제 refresh는 IPC drain 루프 종료 후 — 루프 안에서는 ipc borrow가 살아있음)
                if matches!(
                    cmd.request.method.as_str(),
                    "plugin.install"
                        | "plugin.remove"
                        | "plugin.enable"
                        | "plugin.disable"
                        | "plugin.grant"
                        | "plugin.revoke"
                ) {
                    tool_registry_dirty = true;
                }
                send_response(&cmd.response_tx, response);
                processed = true;
                continue;
            }
            // ── Debug Event Bus (App-level — needs PluginManager) ──
            #[cfg(debug_assertions)]
            if cmd.request.method.starts_with("debug.event_bus.") {
                let id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
                let response = handle_debug_event_bus(
                    self.plugin_manager.as_mut(),
                    &cmd.request.method,
                    &cmd.request.params,
                    id,
                );
                send_response(&cmd.response_tx, response);
                processed = true;
                continue;
            }
            #[cfg(debug_assertions)]
            if cmd.request.method == "debug.extension.invoke_hook" {
                let id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
                handle_debug_extension_invoke_hook(
                    self.plugin_manager.as_mut(),
                    &cmd.request.params,
                    id,
                    cmd.response_tx.clone(),
                );
                processed = true;
                continue;
            }
            #[cfg(debug_assertions)]
            if cmd.request.method.starts_with("debug.popup.") {
                let id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
                let response = match cmd.request.method.as_str() {
                    "debug.popup.list" => ipc::handler::popup::handle_list(
                        self.plugin_manager.as_ref(),
                        id,
                    ),
                    "debug.popup.open" => ipc::handler::popup::handle_open(
                        self.plugin_manager.as_mut(),
                        id,
                        &cmd.request.params,
                    ),
                    "debug.popup.close" => ipc::handler::popup::handle_close(
                        self.plugin_manager.as_mut(),
                        id,
                        &cmd.request.params,
                    ),
                    other => ipc::protocol::JsonRpcResponse::method_not_found(id, other),
                };
                send_response(&cmd.response_tx, response);
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
                send_response(&cmd.response_tx, response);
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
                        send_response(&cmd.response_tx, response);
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
                    send_response(&cmd.response_tx, response);
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
                    send_response(&cmd.response_tx, response);
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
                    send_response(&cmd.response_tx, response);
                    w.base.dirty = true;
                }
                processed = true;
                continue;
            }

            // approval.await: blocking + timeout. 메인 스레드가 막히지 않게 워커
            // 스레드에 위임. Arc<ApprovalStore> 만 클론하면 도메인 단독으로 동작한다.
            if cmd.request.method == "approval.await" {
                let store_opt = self
                    .windows
                    .values()
                    .find_map(|w| w.as_main().map(|w| w.state.engine.approval_store.clone()))
                    .or_else(|| {
                        self.parked_states
                            .first()
                            .map(|s| s.engine.approval_store.clone())
                    });
                let rpc_id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
                match store_opt {
                    Some(store) => {
                        let params = cmd.request.params.clone();
                        let response_tx = cmd.response_tx.clone();
                        std::thread::spawn(move || {
                            let resp =
                                ipc::handler::approval::await_blocking(&store, rpc_id, &params);
                            send_response(&response_tx, resp);
                        });
                    }
                    None => {
                        send_response(
                            &cmd.response_tx,
                            ipc::protocol::JsonRpcResponse::error(
                                rpc_id,
                                -32000,
                                "no application state available",
                            ),
                        );
                    }
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
                    let response =
                        ipc::handler::handle_with_caller(&mut w.state, &cmd.request, &caller);
                    send_response(&cmd.response_tx, response);
                    w.base.dirty = true;
                    processed = true;
                    continue;
                }
            }
            if let Some(state) = self.parked_states.first_mut() {
                let response = ipc::handler::handle_with_caller(state, &cmd.request, &caller);
                send_response(&cmd.response_tx, response);
                processed = true;
            }
        }
        if tool_registry_dirty {
            self.refresh_tool_registry();
        }
        processed
    }

    /// plugin process가 보낸 IPC 호출들을 라우터로 디스패치하고 결과를 plugin에 회신.
    /// CallerContext::Plugin으로 들어가므로 권한 게이트가 적용된다. 호출 메서드가
    /// 다른 plugin이 점유한 namespace prefix와 매칭되면 `forward_namespace_call_from_plugin`
    /// 경로로 우회 (응답은 target plugin이 줄 때까지 보류되며 main loop 다음 tick에서
    /// caller plugin에 `ipc.result`로 회신).
    fn process_plugin_ipc_calls(&mut self) {
        let calls = match self.plugin_manager.as_mut() {
            Some(mgr) => mgr.take_pending_plugin_calls(),
            None => return,
        };
        for call in calls {
            // shared buffer 생성은 main 채널 + 보조 채널을 동시에 다뤄야 해서
            // dispatcher에 노출하지 않고 매니저가 직접 처리한다. params에서 size를
            // 꺼내 manager에 위임 → 매니저가 fd/HANDLE 송신 + RPC 응답을 모두 처리.
            if call.method == tasty_plugin_protocol::METHOD_HOST_SHARED_BUFFER_CREATE {
                let size = call
                    .params
                    .get("size")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                if let Some(mgr) = self.plugin_manager.as_mut() {
                    let (result, error) = match mgr.create_shared_buffer_for(
                        &call.plugin_id,
                        call.call_id,
                        size,
                    ) {
                        Ok(r) => (
                            serde_json::to_value(&r).ok(),
                            None,
                        ),
                        Err(e) => (None, Some(e)),
                    };
                    mgr.send_ipc_result(
                        &call.plugin_id,
                        call.call_id,
                        result,
                        error,
                    );
                }
                continue;
            }
            // popup.close 인터셉트 — PluginManager가 App에 있어 일반 라우터로 도달 불가.
            // ensure_allowed로 method_meta 권한 게이트(ui.popup)를 통과한 뒤,
            // instance_id가 호출자 plugin 소유인지 확인하고 PluginRequest 사유로 close.
            if call.method == "popup.close" {
                let caller = ipc::caller::CallerContext::Plugin {
                    plugin_id: call.plugin_id.clone(),
                    permissions: call.permissions.clone(),
                };
                let (result, error) = match caller.ensure_allowed(&call.method) {
                    Err(e) => (None, Some(e.to_string())),
                    Ok(()) => {
                        let instance_id = call.params.get("instance_id").and_then(|v| v.as_u64());
                        match instance_id {
                            None => (
                                None,
                                Some("popup.close: missing 'instance_id'".to_string()),
                            ),
                            Some(id) => {
                                let mgr = self.plugin_manager.as_mut();
                                let owns = mgr
                                    .as_ref()
                                    .and_then(|m| {
                                        m.popup_instances()
                                            .find(|(iid, _)| *iid == id)
                                            .map(|(_, inst)| inst.plugin_id == call.plugin_id)
                                    })
                                    .unwrap_or(false);
                                if !owns {
                                    (
                                        None,
                                        Some(format!(
                                            "popup.close: instance {id} not owned by plugin '{}'",
                                            call.plugin_id
                                        )),
                                    )
                                } else if let Some(m) = mgr {
                                    m.close_popup_instance(
                                        id,
                                        tasty_plugin_protocol::PopupCloseReason::PluginRequest,
                                    );
                                    (Some(serde_json::Value::Object(Default::default())), None)
                                } else {
                                    (None, Some("popup.close: plugin manager unavailable".into()))
                                }
                            }
                        }
                    }
                };
                if let Some(mgr) = self.plugin_manager.as_mut() {
                    mgr.send_ipc_result(&call.plugin_id, call.call_id, result, error);
                }
                continue;
            }
            // namespace forward 경로: 메서드가 다른 plugin의 prefix에 매칭되면
            // 검증/forward를 plugin_manager에 위임한다. 응답은 비동기.
            //
            // self-call(caller가 prefix owner와 동일)인 경우는 forward하지 않고
            // 호스트 dispatcher로 통과시킨다. plugin이 자기 namespace 메서드의
            // 구현을 호스트 본문에 위임하는 trampoline 패턴(예: com.tasty.image)을
            // 지원하기 위함. 호스트에 동명 메서드가 없으면 일반 -32601이 떨어진다.
            if let Some(mgr) = self.plugin_manager.as_mut() {
                if let Some(owner) = mgr.ipc_namespaces.resolve(&call.method) {
                    if owner != call.plugin_id {
                        mgr.forward_namespace_call_from_plugin(
                            &call.method,
                            call.params.clone(),
                            &call.plugin_id,
                            call.call_id,
                        );
                        continue;
                    }
                }
            }
            let caller = ipc::caller::CallerContext::Plugin {
                plugin_id: call.plugin_id.clone(),
                permissions: call.permissions.clone(),
            };
            let request = ipc::protocol::JsonRpcRequest {
                jsonrpc: "2.0".to_string(),
                id: Some(serde_json::Value::from(call.call_id)),
                method: call.method.clone(),
                params: call.params.clone(),
                session_token: None,
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

    /// 모든 윈도우/parked state의 surface close lifecycle 큐를 비우고 구독 plugin에
    /// broadcast한다. `is_user_close` bool → `SurfaceCloseReason` enum 매핑은
    /// 여기서 수행 (state/ 레이어가 plugin/ 의존을 갖지 않게).
    ///
    /// Event Bus 1.0 `surface.closed`로 broadcast. (PR 4에서 옛 `surface.lifecycle`
    /// IPC 폐기. plugin은 `event_subscribe = ["surface.closed"]`로 구독한다.)
    pub(crate) fn dispatch_pending_surface_lifecycle(&mut self) {
        use tasty_plugin_protocol::EventScope;
        use tasty_plugin_protocol::events::LifecycleReason;
        use tasty_plugin_protocol::events::payloads::SurfaceClosed;
        let mut drained: Vec<crate::state::PendingSurfaceClosed> = Vec::new();
        for w in self.windows.values_mut() {
            if let Some(main) = w.as_main_mut() {
                drained.extend(main.state.take_pending_lifecycle_events());
            }
        }
        for s in &mut self.parked_states {
            drained.extend(s.take_pending_lifecycle_events());
        }
        if drained.is_empty() {
            return;
        }
        let lua = self.lua_engine.as_ref();
        let Some(mgr) = self.plugin_manager.as_mut() else {
            return;
        };
        for ev in drained {
            let bus_reason = if ev.is_user_close {
                LifecycleReason::User
            } else {
                LifecycleReason::Ipc
            };
            let payload = SurfaceClosed {
                surface_id: ev.surface_id,
                kind: ev.kind.to_string(),
                reason: bus_reason,
            };
            mgr.emit_host_event("surface.closed", &payload, EventScope::Surface);
            fire_lua(lua, "surface.delete.post", &payload);
        }
    }

    /// `tasty-memory` regular 영역의 누적 변경을 drain 해 `memory.changed` host
    /// event 로 broadcast. secret 영역 변경은 store 가 발화 큐에 넣지 않으므로
    /// 자동으로 누락된다 (다른 plugin 누설 방지).
    pub(crate) fn dispatch_pending_memory_changes(&mut self) {
        use tasty_plugin_protocol::EventScope;
        use tasty_plugin_protocol::events::payloads::{
            MemoryChangeKind as ProtoKind, MemoryChanged,
        };
        let Some(changes) =
            tasty_memory::with_store(|s| s.take_pending_changes())
        else {
            return;
        };
        if changes.is_empty() {
            return;
        }
        let Some(mgr) = self.plugin_manager.as_mut() else {
            return;
        };
        for ch in changes {
            let kind = match ch.kind {
                tasty_memory::MemoryChangeKind::Created => ProtoKind::Created,
                tasty_memory::MemoryChangeKind::Updated => ProtoKind::Updated,
                tasty_memory::MemoryChangeKind::Deleted => ProtoKind::Deleted,
                tasty_memory::MemoryChangeKind::Expired => ProtoKind::Expired,
            };
            let payload = MemoryChanged {
                scope: ch.scope,
                key: ch.key,
                kind,
                version: ch.version,
            };
            mgr.emit_host_event("memory.changed", &payload, EventScope::System);
        }
    }

    /// 도구 메뉴 클릭으로 enqueue된 이벤트 큐(`pending_tool_events`)를 모든 AppState
    /// 에서 drain해 PluginManager로 publish한다. payload는 plugin 작성자가 정의한 임의
    /// JSON value를 그대로 전달 (현재 `{ "tool_id": "<plugin_id>/<tool_id>" }`).
    pub(crate) fn dispatch_pending_tool_events(&mut self) {
        let mut drained: Vec<(String, serde_json::Value)> = Vec::new();
        for w in self.windows.values_mut() {
            if let Some(main) = w.as_main_mut() {
                drained.append(&mut main.state.pending_tool_events);
            }
        }
        for s in &mut self.parked_states {
            drained.append(&mut s.pending_tool_events);
        }
        if drained.is_empty() {
            return;
        }
        let Some(mgr) = self.plugin_manager.as_mut() else {
            return;
        };
        for (key, payload) in drained {
            // tool 트리거 이벤트는 system scope. 매니페스트 events_emitted에 등록되지 않은
            // 임의 키도 호스트 발화는 허용 (publish 권한 검사는 plugin 발화 경로에만 적용).
            mgr.emit_host_event(&key, &payload, tasty_plugin_protocol::EventScope::System);
        }
    }

    /// file handler IPC action 큐 drain. user TOML 등에서 `type="ipc"` 인 핸들러가
    /// 매칭되면 `(method, target)` 이 enqueue 되어 여기서 plugin namespace 메서드로
    /// forward 된다. 응답은 무시 (fire-and-forget) — 핸들러 실행 결과는 plugin 자체
    /// 로그/이벤트로 관찰.
    pub(crate) fn dispatch_pending_handler_ipc(&mut self) {
        let mut drained: Vec<(String, crate::file_format::FileTarget)> = Vec::new();
        for w in self.windows.values_mut() {
            if let Some(main) = w.as_main_mut() {
                drained.append(&mut main.state.pending_handler_ipc);
            }
        }
        for s in &mut self.parked_states {
            drained.append(&mut s.pending_handler_ipc);
        }
        if drained.is_empty() {
            return;
        }
        let Some(mgr) = self.plugin_manager.as_mut() else {
            for (method, target) in drained {
                tracing::warn!(
                    method = %method,
                    target = %target.display(),
                    "file handler IPC action dropped: plugin manager not running",
                );
            }
            return;
        };
        for (method, target) in drained {
            let params = serde_json::json!({
                "path": target.as_path().to_string_lossy(),
            });
            let (tx, _rx) = std::sync::mpsc::sync_channel(1);
            mgr.forward_namespace_call(&method, params, None, serde_json::Value::Null, tx);
        }
    }

    /// 호스트 내부 Intent 큐를 모든 AppState 에서 drain 해 도메인별 핸들러로 분기한다.
    /// 설계: `docs/design/action-dispatch.md`. 처리 순서 = 발화 순서.
    /// drain 중 새로 발화된 Intent 는 다음 프레임에 처리 (재진입 방지).
    pub(crate) fn dispatch_pending_intents(&mut self) {
        // 모든 windows + parked_states 에서 드레인한 뒤 일괄 처리.
        // 각 state 마다 독립적으로 처리해야 — popup mutation 은 그 state.popups 대상이므로.
        let mut per_state_batches: Vec<(WindowId, Vec<crate::intent::DispatchedIntent>)> =
            Vec::new();
        let mut parked_batches: Vec<(usize, Vec<crate::intent::DispatchedIntent>)> = Vec::new();

        for (id, w) in self.windows.iter_mut() {
            if let Some(main) = w.as_main_mut() {
                let batch = main.state.take_pending_intents();
                if !batch.is_empty() {
                    per_state_batches.push((*id, batch));
                }
            }
        }
        for (idx, s) in self.parked_states.iter_mut().enumerate() {
            let batch = s.take_pending_intents();
            if !batch.is_empty() {
                parked_batches.push((idx, batch));
            }
        }

        for (window_id, batch) in per_state_batches {
            let Some(main) = self
                .windows
                .get_mut(&window_id)
                .and_then(|w| w.as_main_mut())
            else {
                continue;
            };
            for intent in batch {
                #[cfg(debug_assertions)]
                crate::intent::watch::observe(&intent);
                Self::dispatch_one_intent(&mut main.state, &intent);
            }
            main.mark_dirty();
        }
        for (idx, batch) in parked_batches {
            let Some(state) = self.parked_states.get_mut(idx) else {
                continue;
            };
            for intent in batch {
                #[cfg(debug_assertions)]
                crate::intent::watch::observe(&intent);
                Self::dispatch_one_intent(state, &intent);
            }
        }
    }

    /// 단일 Intent 를 도메인 핸들러로 분기한다.
    fn dispatch_one_intent(
        state: &mut crate::state::AppState,
        intent: &crate::intent::DispatchedIntent,
    ) {
        use crate::intent::Intent;
        match &intent.body {
            Intent::OpenPopup { .. }
            | Intent::ClosePopup { .. }
            | Intent::TogglePopup { .. } => {
                crate::intent::popup::handle(state, intent);
            }
            Intent::Noop => {}
        }
    }

    /// `ToolAction::OpenPopup` 클릭으로 enqueue된 popup 큐를 모든 AppState에서 drain해
    /// `PluginManager::open_popup_instance`로 dispatch한다. plugin이 실행 중이 아니면
    /// `open_popup_instance`가 자체적으로 warn 후 무시.
    pub(crate) fn dispatch_pending_popup_opens(&mut self) {
        let mut drained: Vec<(String, String, serde_json::Value)> = Vec::new();
        for w in self.windows.values_mut() {
            if let Some(main) = w.as_main_mut() {
                drained.append(&mut main.state.pending_popup_opens);
            }
        }
        for s in &mut self.parked_states {
            drained.append(&mut s.pending_popup_opens);
        }
        if drained.is_empty() {
            return;
        }
        let Some(mgr) = self.plugin_manager.as_mut() else {
            return;
        };
        for (plugin_id, popup_id, context) in drained {
            mgr.open_popup_instance(&plugin_id, &popup_id, context);
        }
    }

    /// plugin popup 렌더 중 수집된 사용자 입력 / close 사유를 모든 AppState에서 drain해
    /// `PluginManager`로 forward한다. (`send_popup_event` / `close_popup_instance`)
    pub(crate) fn dispatch_plugin_popup_events(&mut self) {
        let mut drained_events: Vec<(u64, tasty_plugin_protocol::ui_tree::UiEvent)> = Vec::new();
        let mut drained_closes: Vec<(u64, tasty_plugin_protocol::PopupCloseReason)> = Vec::new();
        for w in self.windows.values_mut() {
            if let Some(main) = w.as_main_mut() {
                drained_events.append(&mut main.state.plugin_popup_events);
                drained_closes.append(&mut main.state.plugin_popup_closes);
            }
        }
        for s in &mut self.parked_states {
            drained_events.append(&mut s.plugin_popup_events);
            drained_closes.append(&mut s.plugin_popup_closes);
        }
        if drained_events.is_empty() && drained_closes.is_empty() {
            return;
        }
        let Some(mgr) = self.plugin_manager.as_mut() else {
            return;
        };
        for (instance_id, event) in drained_events {
            mgr.send_popup_event(instance_id, &event);
        }
        // 같은 인스턴스에 대해 close 사유가 여러 번 쌓일 수 있다 (Escape 매 프레임 등).
        // 첫 사유로 close하고 나머지는 무시 — close_popup_instance가 알아서 멱등 처리.
        let mut seen = std::collections::HashSet::new();
        for (instance_id, reason) in drained_closes {
            if seen.insert(instance_id) {
                mgr.close_popup_instance(instance_id, reason);
            }
        }
    }

    /// 호스트 자동 발화 큐(`PendingHostEvent`)를 모든 AppState에서 drain해 Event Bus
    /// 1.0 wire payload로 변환·발화한다. focus처럼 발화 시점을 일일이 hook하기 번거로운
    /// 이벤트는 먼저 `detect_focus_change()`로 변화 검사 후 queue에 push된다.
    pub(crate) fn dispatch_pending_host_events(&mut self) {
        use tasty_plugin_protocol::EventScope;
        use tasty_plugin_protocol::LifecycleReason;
        use tasty_plugin_protocol::events::payloads::{
            HookFired, NotificationCreated, PaneClosed, PaneCreated, PaneSplit, ProcessExited,
            SplitDirection, SurfaceCreated, SurfaceCreatedBy, SurfaceFocused, SurfaceResized,
            SurfaceTitleChanged, TabClosed, TabCreated, TabFocused, TabMoved, TabRenamed,
            WorkspaceActivated, WorkspaceClosed, WorkspaceCreated, WorkspaceRenamed,
        };

        let mut drained: Vec<crate::state::PendingHostEvent> = Vec::new();
        for (win_id, w) in self.windows.iter_mut() {
            if let Some(main) = w.as_main_mut() {
                main.state.detect_focus_change();
                main.state.detect_workspace_activation();
                main.state.detect_tab_focus_change();
                main.state.detect_tab_lifecycle();
                main.state.detect_pane_lifecycle();
                main.state.detect_workspace_lifecycle(u64::from(*win_id));
                main.state.detect_surface_lifecycle();
                drained.extend(main.state.take_pending_host_events());
            }
        }
        for s in &mut self.parked_states {
            s.detect_focus_change();
            s.detect_workspace_activation();
            s.detect_tab_focus_change();
            s.detect_tab_lifecycle();
            s.detect_pane_lifecycle();
            // parked AppState은 더 이상 window에 붙어있지 않으므로 workspace.created
            // 발화에 의미 있는 window_id를 채울 수 없다. workspace.closed만 의도하는
            // 경우라도 polling은 새 workspace를 detect할 수 없게 베이스라인부터
            // 비교가 필요하다. window 분리 직전의 detect에서 이미 베이스라인이
            // 형성됐다고 가정하고 동일 호출 — window_id는 0 (sentinel).
            s.detect_workspace_lifecycle(0);
            s.detect_surface_lifecycle();
            drained.extend(s.take_pending_host_events());
        }
        if drained.is_empty() {
            return;
        }
        let lua = self.lua_engine.as_ref();
        let Some(mgr) = self.plugin_manager.as_mut() else {
            return;
        };
        for ev in drained {
            match ev {
                crate::state::PendingHostEvent::SurfaceFocused {
                    surface_id,
                    prev_surface_id,
                } => {
                    let payload = SurfaceFocused {
                        surface_id,
                        prev_surface_id,
                    };
                    mgr.emit_host_event("surface.focused", &payload, EventScope::Surface);
                }
                crate::state::PendingHostEvent::SurfaceResized {
                    surface_id,
                    width_px,
                    height_px,
                } => {
                    let payload = SurfaceResized {
                        surface_id,
                        width_px,
                        height_px,
                    };
                    mgr.emit_host_event_throttled(
                        "surface.resized",
                        surface_id as u64,
                        &payload,
                        EventScope::Surface,
                    );
                }
                crate::state::PendingHostEvent::SurfaceTitleChanged { surface_id, title } => {
                    let payload = SurfaceTitleChanged { surface_id, title };
                    mgr.emit_host_event("surface.title_changed", &payload, EventScope::Surface);
                }
                crate::state::PendingHostEvent::SurfaceCreated {
                    surface_id,
                    kind,
                    tab_id,
                    pane_id,
                    workspace_id,
                    created_by_plugin,
                } => {
                    let created_by = match created_by_plugin {
                        Some(pid) => SurfaceCreatedBy::Agent { source_plugin: pid },
                        None => SurfaceCreatedBy::User,
                    };
                    let payload = SurfaceCreated {
                        surface_id,
                        kind: kind.to_string(),
                        tab_id,
                        pane_id,
                        workspace_id,
                        created_by,
                    };
                    mgr.emit_host_event("surface.created", &payload, EventScope::Surface);
                    fire_lua(lua, "surface.create.post", &payload);
                }
                crate::state::PendingHostEvent::WorkspaceActivated {
                    workspace_id,
                    prev_workspace_id,
                } => {
                    let payload = WorkspaceActivated {
                        workspace_id,
                        prev_workspace_id,
                    };
                    mgr.emit_host_event("workspace.activated", &payload, EventScope::System);
                }
                crate::state::PendingHostEvent::WorkspaceRenamed {
                    workspace_id,
                    name,
                    subtitle,
                    description,
                    user_direct,
                } => {
                    let payload = WorkspaceRenamed {
                        workspace_id,
                        name,
                        subtitle,
                        description,
                    };
                    mgr.emit_host_event("workspace.renamed", &payload, EventScope::System);
                    // 사용자 직접 변경(GUI rename dialog)만 Lua hook 발화 — IPC 경유는 제외.
                    if user_direct {
                        fire_lua(lua, "workspace.change.post", &payload);
                    }
                }
                crate::state::PendingHostEvent::TabFocused {
                    tab_id,
                    pane_id,
                    prev_tab_id,
                } => {
                    let payload = TabFocused {
                        tab_id,
                        pane_id,
                        prev_tab_id,
                    };
                    mgr.emit_host_event("tab.focused", &payload, EventScope::System);
                }
                crate::state::PendingHostEvent::TabRenamed {
                    tab_id,
                    title,
                    user_direct,
                } => {
                    let payload = TabRenamed { tab_id, title };
                    mgr.emit_host_event("tab.renamed", &payload, EventScope::System);
                    if user_direct {
                        fire_lua(lua, "tab.change.post", &payload);
                    }
                }
                crate::state::PendingHostEvent::ProcessExited { surface_id } => {
                    let payload = ProcessExited {
                        surface_id,
                        exit_code: None,
                    };
                    mgr.emit_host_event("process.exited", &payload, EventScope::Surface);
                }
                crate::state::PendingHostEvent::NotificationCreated {
                    id,
                    title,
                    body,
                    source,
                } => {
                    let payload = NotificationCreated {
                        id: id.to_string(),
                        title,
                        body,
                        source,
                    };
                    mgr.emit_host_event("notification.created", &payload, EventScope::System);
                }
                crate::state::PendingHostEvent::TabCreated {
                    tab_id,
                    pane_id,
                    workspace_id,
                    kind,
                } => {
                    let payload = TabCreated {
                        tab_id,
                        pane_id,
                        workspace_id,
                        kind,
                    };
                    mgr.emit_host_event("tab.created", &payload, EventScope::System);
                    fire_lua(lua, "tab.create.post", &payload);
                }
                crate::state::PendingHostEvent::TabClosed { tab_id, pane_id } => {
                    let payload = TabClosed {
                        tab_id,
                        pane_id,
                        reason: LifecycleReason::User,
                    };
                    mgr.emit_host_event("tab.closed", &payload, EventScope::System);
                    fire_lua(lua, "tab.delete.post", &payload);
                }
                crate::state::PendingHostEvent::TabMoved {
                    tab_id,
                    from_pane,
                    to_pane,
                } => {
                    let payload = TabMoved {
                        tab_id,
                        from_pane,
                        to_pane,
                    };
                    mgr.emit_host_event("tab.moved", &payload, EventScope::System);
                }
                crate::state::PendingHostEvent::PaneCreated {
                    pane_id,
                    workspace_id,
                } => {
                    let payload = PaneCreated {
                        pane_id,
                        parent_pane_group: None,
                        workspace_id,
                    };
                    mgr.emit_host_event("pane.created", &payload, EventScope::System);
                    fire_lua(lua, "pane.create.post", &payload);
                }
                crate::state::PendingHostEvent::PaneClosed { pane_id } => {
                    let payload = PaneClosed {
                        pane_id,
                        reason: LifecycleReason::User,
                    };
                    mgr.emit_host_event("pane.closed", &payload, EventScope::System);
                    fire_lua(lua, "pane.delete.post", &payload);
                }
                crate::state::PendingHostEvent::WorkspaceCreated {
                    workspace_id,
                    window_id,
                    name,
                } => {
                    let payload = WorkspaceCreated {
                        workspace_id,
                        window_id,
                        name,
                    };
                    mgr.emit_host_event("workspace.created", &payload, EventScope::System);
                    fire_lua(lua, "workspace.create.post", &payload);
                }
                crate::state::PendingHostEvent::WorkspaceClosed { workspace_id } => {
                    let payload = WorkspaceClosed {
                        workspace_id,
                        reason: LifecycleReason::User,
                    };
                    mgr.emit_host_event("workspace.closed", &payload, EventScope::System);
                    fire_lua(lua, "workspace.delete.post", &payload);
                }
                crate::state::PendingHostEvent::HookFired {
                    hook_id,
                    event_kind,
                    surface_id,
                } => {
                    let scope = if surface_id != 0 {
                        EventScope::Surface
                    } else {
                        EventScope::System
                    };
                    let payload = HookFired {
                        hook_id: hook_id.to_string(),
                        event_kind,
                        surface_id: if surface_id != 0 { Some(surface_id) } else { None },
                        payload: serde_json::Value::Null,
                    };
                    mgr.emit_host_event("hook.fired", &payload, scope);
                }
                crate::state::PendingHostEvent::PaneSplit {
                    original_pane,
                    new_pane,
                    direction,
                } => {
                    let direction = match direction {
                        crate::model::SplitDirection::Horizontal => SplitDirection::Horizontal,
                        crate::model::SplitDirection::Vertical => SplitDirection::Vertical,
                    };
                    let payload = PaneSplit {
                        original_pane,
                        new_pane,
                        direction,
                    };
                    mgr.emit_host_event("pane.split", &payload, EventScope::System);
                }
                crate::state::PendingHostEvent::Raw { key, payload } => {
                    mgr.emit_host_event(&key, &payload, EventScope::System);
                }
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
        // SAFETY: AttachConsole은 thread-safe Win32 호출. main 진입 첫 단계로,
        // 다른 thread가 아직 spawn되지 않은 시점. 결과 무시는 의도적 (부모가 GUI 셸이면 실패).
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

    // state.db 초기화는 GUI 부팅 시점(create_new_window)으로 이동됨.
    // 실패 시 InfoModal로 사용자에게 안내 후 Exit(1).

    // If a subcommand was provided, run in CLI client mode
    if let Some(command) = cli.command {
        return cli::run_client(command);
    }

    // Inside a tasty terminal without subcommand: show help instead of launching GUI
    if !cli.launch && std::env::var("TASTY_SURFACE_ID").is_ok() {
        cli::print_augmented_help()?;
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
    fire_lua(app.lua_engine.as_ref(), "tasty.startup.post", &serde_json::Value::Null);
    event_loop.run_app(&mut app)?;

    Ok(())
}
