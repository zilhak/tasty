#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod adapters;
mod app;
mod boot;
mod clipboard;
mod core;
mod db;
mod file;
mod gfx;
mod host_api;
mod hub;
mod i18n;
mod intent;
mod model;
mod platform;
mod ports;
mod scheduler;
mod state;
mod store;
mod view;
mod waker;

/// `Surface::as_any` / `as_any_mut` 구현을 한 줄로 채우는 매크로.
///
/// ```ignore
/// impl Surface for MyPanel {
///     crate::impl_surface_any!();
///     // ... 다른 메서드들 ...
/// }
/// ```
#[macro_export]
macro_rules! impl_surface_any {
    () => {
        fn as_any(&self) -> &dyn ::std::any::Any {
            self
        }
        fn as_any_mut(&mut self) -> &mut dyn ::std::any::Any {
            self
        }
    };
}

pub mod engine;

use anyhow::Result;

pub use tasty_font as font;
pub use tasty_settings as settings;
use tasty_terminal as terminal;
pub use tasty_themes as theme;
pub use tasty_utils::path as paths;

pub(crate) use adapters::cli;
pub(crate) use adapters::ipc;
pub(crate) use adapters::plugin;
pub(crate) use adapters::ui;
pub(crate) use adapters::ui::input::click_cursor;
pub(crate) use adapters::ui::input::double_tap;
pub(crate) use adapters::ui::input::shortcuts;
pub(crate) use adapters::ui::preset as preset_ui;
pub(crate) use adapters::ui::surface::diff as diff_ui;
pub(crate) use adapters::ui::surface::empty as empty_ui;
pub(crate) use adapters::ui::surface::image as image_ui;
pub(crate) use adapters::ui::surface::markdown as markdown_ui;
pub(crate) use adapters::ui::terminal_link;
pub(crate) use adapters::ui::theme_bridge;
pub(crate) use adapters::ui::window;
pub(crate) use adapters::ui::window::plugins::ui as plugins_ui;
pub(crate) use adapters::ui::window::settings::ui as settings_ui;
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
pub(crate) use host_api::hooks;
pub(crate) use host_api::hooks::global as global_hooks;
pub(crate) use host_api::webview;
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

fn main() -> Result<()> {
    boot::run()
}
