#![allow(private_interfaces)]
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod boot;
mod clipboard;
mod command_index;
mod command_palette;
mod db;
mod file;
mod gfx;
mod git_viewer;
mod host_api;
mod input;
mod intent;
mod layout_persistence;
mod native_menu;
mod platform;
mod state;
mod store;
mod surface_registry;
mod ui;
mod update_check;

pub mod engine;
pub mod window;

use anyhow::Result;

pub use tasty_core::{i18n, model, paths, theme};
pub use tasty_font as font;
pub use tasty_settings as settings;
use tasty_terminal as terminal;

pub(crate) use app::App;
pub(crate) use boot::waker as waker_factory_winit;
pub(crate) use clipboard::{ClipboardContext, ClipboardData};
pub(crate) use engine::output_observer;
pub(crate) use engine::state as engine_state;
pub(crate) use file::dispatch as file_dispatch;
pub(crate) use file::handler_recent as file_handler_recent;
pub(crate) use file::handlers_save as file_handlers_save;
pub(crate) use file::identify_worker;
pub(crate) use gfx::gpu;
pub(crate) use gfx::renderer;
pub(crate) use host_api::cli;
pub(crate) use host_api::hooks;
pub(crate) use host_api::hooks::global as global_hooks;
pub(crate) use host_api::ipc;
pub(crate) use host_api::plugin;
pub(crate) use host_api::webview;
pub(crate) use input::click_cursor;
pub(crate) use input::double_tap;
pub(crate) use input::shortcuts;
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
pub(crate) use ui::surface::image as image_ui;
pub(crate) use ui::surface::markdown as markdown_ui;
pub(crate) use ui::terminal_link;
pub(crate) use ui::theme_bridge;
pub(crate) use window::plugins::ui as plugins_ui;
pub(crate) use window::settings::ui as settings_ui;

use model::DividerInfo;

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

fn main() -> Result<()> {
    boot::run()
}
