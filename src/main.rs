#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod adapters;
mod app;
mod boot;
mod clipboard;
mod core;
mod db;
mod file;
#[cfg(feature = "gui")]
mod gfx;
mod hook_handler;
mod host_api;
mod hub;
mod i18n;
mod intent;
mod model;
mod platform;
mod plugin_bridge;
mod ports;
mod scheduler;
mod state;
mod store;
#[cfg(feature = "gui")]
mod view;
mod waker;
mod webhook;

pub mod engine;

use anyhow::Result;

pub use tasty_font as font;
pub use tasty_settings as settings;
#[cfg(feature = "gui")]
use tasty_terminal as terminal;
pub use tasty_themes as theme;
pub use tasty_utils::path as paths;

pub(crate) use adapters::cli;
pub(crate) use adapters::ipc;
pub(crate) use adapters::plugin;
#[cfg(feature = "gui")]
pub(crate) use adapters::ui::input::click_cursor;
#[cfg(feature = "gui")]
pub(crate) use adapters::ui::input::double_tap;
#[cfg(feature = "gui")]
pub(crate) use adapters::ui::input::shortcuts;
#[cfg(feature = "gui")]
pub(crate) use adapters::ui::preset as preset_ui;
#[cfg(feature = "gui")]
pub(crate) use adapters::ui::surface::empty as empty_ui;
#[cfg(feature = "gui")]
pub(crate) use adapters::ui::surface::explorer as explorer_ui;
#[cfg(feature = "gui")]
pub(crate) use adapters::ui::surface::webview_chrome as webview_chrome_ui;
#[cfg(feature = "gui")]
pub(crate) use adapters::ui::terminal_link;
#[cfg(feature = "gui")]
pub(crate) use app::App;
pub(crate) use app::event::AppEvent;
#[cfg(feature = "gui")]
pub(crate) use boot::waker as waker_factory_winit;
#[cfg(feature = "gui")]
pub(crate) use clipboard::ClipboardContext;
pub(crate) use engine::output_observer;
pub(crate) use engine::surface_registry::meta as surface_meta;
#[cfg(feature = "gui")]
pub(crate) use file::dispatch as file_dispatch;
#[cfg(feature = "gui")]
pub(crate) use file::identify_worker;
#[cfg(feature = "gui")]
pub(crate) use gfx::gpu;
#[cfg(feature = "gui")]
pub(crate) use gfx::renderer;
pub(crate) use host_api::hooks;
pub(crate) use host_api::hooks::global as global_hooks;
#[cfg(feature = "gui")]
pub(crate) use host_api::webview;
#[cfg(feature = "gui")]
pub(crate) use platform::app_icon;
pub(crate) use platform::crash_report;
#[cfg(all(debug_assertions, feature = "gui"))]
pub(crate) use platform::debug_info;
#[cfg(all(windows, feature = "gui"))]
pub(crate) use platform::jump_list;
#[cfg(all(target_os = "macos", feature = "gui"))]
pub(crate) use platform::macos_delegate;
#[cfg(all(
    any(windows, target_os = "macos", target_os = "linux"),
    feature = "gui"
))]
pub(crate) use platform::system_tray;
pub(crate) use state::search as search_state;
#[cfg(feature = "gui")]
pub(crate) use state::selection;
pub(crate) use store::notification;
pub(crate) use store::recent_files;
pub(crate) use store::scrollback as scrollback_store;
#[cfg(feature = "gui")]
pub(crate) use view as window;
#[cfg(feature = "gui")]
pub(crate) use view::plugins::ui as plugins_ui;
#[cfg(feature = "gui")]
pub(crate) use view::settings::ui as settings_ui;

/// dhat heap 프로파일링 (opt-in): `cargo build --features dhat-heap` 빌드에서
/// 전 할당을 계측해 종료 시 `dhat-heap.json` 을 남긴다. UMDH 없는 환경에서의
/// 크로스플랫폼 heap attribution 용 — docs/dev-guide/memory-leak-soak.md 참조.
/// release 배포 경로와 무관 (기본 feature 아님).
#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

fn main() -> Result<()> {
    #[cfg(feature = "dhat-heap")]
    let _profiler = dhat::Profiler::new_heap();
    boot::run()
}
