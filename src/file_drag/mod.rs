//! Cross-platform file drag-and-drop to external applications.
//!
//! Initiates OS-native drag sessions so files selected in the Explorer
//! can be dropped onto Finder, Windows Explorer, Nautilus, etc.

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "macos")]
pub use macos::start_file_drag;

#[cfg(windows)]
mod windows;

#[cfg(windows)]
pub use windows::start_file_drag;

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
pub use linux::start_file_drag;

/// Outcome of a drag operation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DragResult {
    /// The drop was accepted by the target application.
    Accepted,
    /// The drag was cancelled (user released outside a valid target).
    Cancelled,
}
