#![allow(private_interfaces)]
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod boot;
mod cli;
mod clipboard;
mod command_index;
mod command_palette;
mod db;
mod file;

mod git_viewer;
mod gpu;
mod hooks;
mod input;
mod intent;
mod ipc;
mod layout_persistence;
mod native_menu;
mod platform;
mod plugin;
mod plugins_ui;
mod renderer;
mod settings_ui;
mod shortcuts;
mod state;
mod store;
mod surface_registry;
mod ui;
mod update_check;
mod webview;

pub mod engine;
pub mod window;

use anyhow::Result;

pub use tasty_core::{i18n, model, paths, theme};
pub use tasty_font as font;
pub use tasty_settings as settings;
use tasty_terminal as terminal;

pub(crate) use app::App;
pub(crate) use boot::waker as waker_factory_winit;
pub(crate) use engine::output_observer;
pub(crate) use engine::state as engine_state;
pub(crate) use file::dispatch as file_dispatch;
pub(crate) use file::handler_recent as file_handler_recent;
pub(crate) use file::handlers_save as file_handlers_save;
pub(crate) use file::identify_worker;
pub(crate) use hooks::global as global_hooks;
pub(crate) use input::click_cursor;
pub(crate) use input::double_tap;
pub(crate) use platform::app_icon;
pub(crate) use platform::crash_report;
#[cfg(debug_assertions)]
pub(crate) use platform::debug_info;
#[cfg(windows)]
pub(crate) use platform::jump_list;
#[cfg(target_os = "macos")]
pub(crate) use platform::macos_delegate;
#[cfg(windows)]
pub(crate) use platform::system_tray;
pub(crate) use state::search as search_state;
pub(crate) use state::selection;
pub(crate) use store::clipboard_history;
pub(crate) use store::notification;
pub(crate) use store::recent_files;
pub(crate) use store::scrollback as scrollback_store;
pub(crate) use surface_registry::meta as surface_meta;
pub(crate) use ui::preset as preset_ui;
pub(crate) use ui::surface::diff as diff_ui;
pub(crate) use ui::surface::empty as empty_ui;
pub(crate) use ui::surface::html as html_ui;
pub(crate) use ui::surface::image as image_ui;
pub(crate) use ui::surface::markdown as markdown_ui;
pub(crate) use ui::terminal_link;
pub(crate) use ui::theme_bridge;

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
        if let Err(e) = self.inner.set_text(text.to_string()) {
            tracing::warn!("clipboard set_text failed: {e}");
        }
    }
}

/// Clipboard data detected by the background polling thread.
pub(crate) enum ClipboardData {
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
pub(crate) enum AppEvent {
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
        target: crate::file::format::FileTarget,
        detector: Option<crate::file::format::DetectorId>,
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

/// Phase 6.2c — envelope 의 `session_token` 필드를 보고 caller 를 결정한다.
///
/// - `session_token` 가 None → `CallerContext::Local`
/// - 형식이 잘못된 토큰(64-char hex 아님) → `Err(deny)`
/// - 유효 형식이지만 store 에 없음/만료/revoked → `Err(deny)` (Local fallback 금지)
/// - 유효 → `CallerContext::Agent { ... }`
///
/// memory store 가 초기화되지 않은 경우 (`with_store` 가 `None`): 토큰이 있어도
/// 검증 불가이므로 `Err(deny)` 로 막는다 — 부팅 초기의 가장된 호출을 막는다.
pub(crate) fn resolve_caller_from_envelope(
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
        permissions: Arc::new(perms),
    })
}

/// `ShortcutOverride`를 `command.shortcut_changed` payload용 단순 문자열로 변환.
/// `Key` 모드의 다중 키는 `, `로 join, `Inherit`는 `@source`로 표기, 비어 있거나
/// `None` 모드는 `None` 반환. 정확한 상태는 plugin이 IPC로 재조회한다.
pub(crate) fn shortcut_override_display(
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
pub(crate) fn handle_debug_event_bus(
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
pub(crate) fn handle_debug_extension_invoke_hook(
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
            send_response(
                &response_tx,
                JsonRpcResponse::error(id, -32000, "plugin manager not initialized"),
            );
            return;
        }
    };
    let extension_id = match params.get("extension_id").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => {
            send_response(
                &response_tx,
                JsonRpcResponse::invalid_params(id, "missing 'extension_id'"),
            );
            return;
        }
    };
    let kind = match params.get("kind").and_then(|v| v.as_str()) {
        Some("event") => tasty_plugin_protocol::ExtensionHookKind::Event,
        Some("ipc") => tasty_plugin_protocol::ExtensionHookKind::Ipc,
        _ => {
            send_response(
                &response_tx,
                JsonRpcResponse::invalid_params(
                    id,
                    "missing/invalid 'kind' (expected 'event' or 'ipc')",
                ),
            );
            return;
        }
    };
    let phase = match params.get("phase").and_then(|v| v.as_str()) {
        Some("pre") => tasty_plugin_protocol::ExtensionHookPhase::Pre,
        Some("post") => tasty_plugin_protocol::ExtensionHookPhase::Post,
        _ => {
            send_response(
                &response_tx,
                JsonRpcResponse::invalid_params(
                    id,
                    "missing/invalid 'phase' (expected 'pre' or 'post')",
                ),
            );
            return;
        }
    };
    let mode = match params.get("mode").and_then(|v| v.as_str()) {
        Some("transform") => crate::plugin::manifest::HookMode::Transform,
        Some("filter") => crate::plugin::manifest::HookMode::Filter,
        Some("observe") => crate::plugin::manifest::HookMode::Observe,
        _ => {
            send_response(
                &response_tx,
                JsonRpcResponse::invalid_params(
                    id,
                    "missing/invalid 'mode' (expected 'transform', 'filter', or 'observe')",
                ),
            );
            return;
        }
    };
    let target = match params.get("target").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => {
            send_response(
                &response_tx,
                JsonRpcResponse::invalid_params(id, "missing 'target'"),
            );
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

fn main() -> Result<()> {
    boot::run()
}
