//! Cross-platform native context menu.
//!
//! Uses OS-native menus (NSMenu on macOS, Win32 TrackPopupMenu on Windows,
//! GTK Menu on Linux) so they render above native child views (WebView).

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "macos")]
pub use macos::show_context_menu;

#[cfg(windows)]
mod windows;

#[cfg(windows)]
pub use windows::show_context_menu;

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
pub use linux::show_context_menu;

/// A single item in a native context menu.
pub struct MenuItem {
    /// Unique identifier returned when this item is selected.
    pub id: u32,
    /// Display label.
    pub label: String,
    /// Whether this item is enabled (grayed out if false).
    pub enabled: bool,
}

impl MenuItem {
    pub fn new(id: u32, label: impl Into<String>) -> Self {
        Self { id, label: label.into(), enabled: true }
    }

    #[allow(dead_code)]
    pub fn disabled(id: u32, label: impl Into<String>) -> Self {
        Self { id, label: label.into(), enabled: false }
    }

    #[allow(dead_code)]
    pub fn separator() -> Self {
        Self { id: 0, label: String::new(), enabled: false }
    }

    pub fn is_separator(&self) -> bool {
        self.label.is_empty()
    }
}
