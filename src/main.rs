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
pub(crate) use adapters::ui::surface::image as image_ui;
#[cfg(feature = "gui")]
pub(crate) use adapters::ui::surface::markdown as markdown_ui;
#[cfg(feature = "gui")]
pub(crate) use adapters::ui::terminal_link;
#[cfg(feature = "gui")]
pub(crate) use app::App;
pub(crate) use app::event::AppEvent;
#[cfg(feature = "gui")]
pub(crate) use boot::waker as waker_factory_winit;
#[cfg(feature = "gui")]
pub(crate) use clipboard::{ClipboardContext, ClipboardData};
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
#[cfg(all(any(windows, target_os = "macos", target_os = "linux"), feature = "gui"))]
pub(crate) use platform::system_tray;
pub(crate) use state::search as search_state;
#[cfg(feature = "gui")]
pub(crate) use state::selection;
pub(crate) use store::clipboard_history;
pub(crate) use store::notification;
pub(crate) use store::recent_files;
pub(crate) use store::scrollback as scrollback_store;
#[cfg(feature = "gui")]
pub(crate) use view as window;
#[cfg(feature = "gui")]
pub(crate) use view::plugins::ui as plugins_ui;
#[cfg(feature = "gui")]
pub(crate) use view::settings::ui as settings_ui;

fn main() -> Result<()> {
    boot::run()
}
