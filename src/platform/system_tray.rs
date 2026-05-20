//! Windows system tray integration.
//!
//! Creates a tray icon with menu items: Show Window, New Window, Quit.
//! Only compiled on Windows.

#![cfg(windows)]

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

    let tray_icon = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("Tasty Terminal")
        .with_icon(icon)
        .build()?;

    tracing::info!("System tray icon created");
    Ok((tray_icon, ids))
}

/// Poll for tray menu events. Returns the menu item ID if an event was received.
pub fn poll_menu_event() -> Option<String> {
    MenuEvent::receiver().try_recv().ok().map(|e| e.id.0)
}
