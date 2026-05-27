#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod boot;
mod clipboard;
mod db;
mod file;
mod gfx;
mod host_api;
mod input;
mod intent;
mod platform;
mod state;
mod store;
mod ui;

pub mod engine;
pub mod window;

use anyhow::Result;

pub use tasty_core::{i18n, model, paths};
pub use tasty_font as font;
pub use tasty_settings as settings;
use tasty_terminal as terminal;
pub use tasty_themes as theme;

pub(crate) use app::App;
pub(crate) use app::event::AppEvent;
pub(crate) use boot::waker as waker_factory_winit;
pub(crate) use clipboard::{ClipboardContext, ClipboardData};
pub(crate) use engine::output_observer;
pub(crate) use engine::state as engine_state;
pub(crate) use engine::surface_registry::meta as surface_meta;
pub(crate) use file::dispatch as file_dispatch;
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
pub(crate) use ui::preset as preset_ui;
pub(crate) use ui::surface::diff as diff_ui;
pub(crate) use ui::surface::empty as empty_ui;
pub(crate) use ui::surface::image as image_ui;
pub(crate) use ui::surface::markdown as markdown_ui;
pub(crate) use ui::terminal_link;
pub(crate) use ui::theme_bridge;
pub(crate) use window::plugins::ui as plugins_ui;
pub(crate) use window::settings::ui as settings_ui;

fn main() -> Result<()> {
    boot::run()
}
