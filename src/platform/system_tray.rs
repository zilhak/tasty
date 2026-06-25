//! Cross-platform system tray / status item integration (best-effort).
//!
//! Creates a tray icon with menu items: Show Window, New Window, Quit.
//! Backed by the `tray-icon` crate:
//! - Windows: notification area (`Shell_NotifyIcon`)
//! - macOS: menu bar status item (`NSStatusItem`)
//! - Linux: StatusNotifierItem / AppIndicator (via GTK)
//!
//! Per ADR-0001 this is best-effort: when the platform has no usable tray
//! (minimal WM, missing AppIndicator host, etc.), [`create_tray_icon`] returns
//! `None` and the caller falls back to taskbar/dock minimize. Creation never
//! aborts the app.
//!
//! Threading constraints (from the `tray-icon` crate docs):
//! - macOS: the tray must be created on the main thread while the event loop is
//!   already running (earliest at `StartCause::Init`). Our creation site runs in
//!   the winit main-thread window setup, after the loop is up.
//! - Linux: a GTK event loop must be running on the creating thread, and the
//!   tray must be created on that thread. tasty pumps GTK iterations from the
//!   main loop (see the caller's polling site) rather than running a dedicated
//!   GTK main loop.

#![cfg(all(
    any(windows, target_os = "macos", target_os = "linux"),
    feature = "gui"
))]

use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{TrayIcon, TrayIconBuilder};

/// Menu item IDs for tray context menu.
pub struct TrayMenuIds {
    pub show_window: String,
    pub new_window: String,
    pub quit: String,
}

/// Creates the system tray icon with menu. Returns the TrayIcon (must be kept alive)
/// and the menu item IDs for event handling.
///
/// Returns `None` (graceful degradation) when the platform tray is unavailable —
/// the caller falls back to taskbar/dock minimize.
pub fn create_tray_icon() -> Option<(TrayIcon, TrayMenuIds)> {
    match create_tray_icon_inner() {
        Ok(result) => Some(result),
        Err(e) => {
            tracing::warn!("Failed to create tray icon: {e}");
            None
        }
    }
}

fn create_tray_icon_inner() -> Result<(TrayIcon, TrayMenuIds), Box<dyn std::error::Error>> {
    // Linux: `tray-icon` (AppIndicator) assumes GTK is already initialized on the
    // calling thread and does not init it itself. Initialize lazily; if GTK is
    // unavailable (no display / no GTK), bail so the caller degrades gracefully.
    #[cfg(target_os = "linux")]
    if !gtk::is_initialized() {
        gtk::init().map_err(|e| format!("GTK init failed: {e}"))?;
    }

    let icon =
        crate::app_icon::tray_icon().ok_or("failed to decode tray icon from embedded PNG")?;

    // Build context menu
    let show_item = MenuItem::new("Show Window", true, None);
    let new_window_item = MenuItem::new("New Window", true, None);
    let quit_item = MenuItem::new("Quit", true, None);

    let ids = TrayMenuIds {
        show_window: show_item.id().0.clone(),
        new_window: new_window_item.id().0.clone(),
        quit: quit_item.id().0.clone(),
    };

    let menu = Menu::new();
    if let Err(e) = menu.append(&show_item) {
        tracing::warn!("Failed to append Show Window menu item: {e}");
    }
    if let Err(e) = menu.append(&new_window_item) {
        tracing::warn!("Failed to append New Window menu item: {e}");
    }
    if let Err(e) = menu.append(&quit_item) {
        tracing::warn!("Failed to append Quit menu item: {e}");
    }

    // `with_icon_as_template(true)` is a no-op off macOS; on macOS it lets the
    // menu bar tint the icon for light/dark mode instead of showing raw colors.
    let tray_icon = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("Tasty Terminal")
        .with_icon(icon)
        .with_icon_as_template(true)
        .build()?;

    tracing::info!("System tray icon created");
    Ok((tray_icon, ids))
}

/// Poll for tray menu events. Returns the menu item ID if an event was received.
pub fn poll_menu_event() -> Option<String> {
    MenuEvent::receiver().try_recv().ok().map(|e| e.id.0)
}

/// Pump pending GTK events so the tray (StatusNotifierItem) can dispatch menu
/// clicks. No-op off Linux. Must be called from the GTK-owning thread (the
/// winit main thread) on each event-loop tick while a tray exists.
///
/// `tray-icon` on Linux requires a running GTK event loop; tasty does not run a
/// dedicated GTK main loop, so we drive non-blocking iterations from the winit
/// loop. `main_iteration_do(false)` returns immediately when there is nothing
/// to process, so this does not block the render loop.
#[cfg(target_os = "linux")]
pub fn pump_gtk_events() {
    if gtk::is_initialized() {
        while gtk::events_pending() {
            gtk::main_iteration_do(false);
        }
    }
}
