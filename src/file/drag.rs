//! Cross-platform file drag-and-drop to external applications.
//!
//! Initiates OS-native drag sessions so files selected in the Explorer
//! can be dropped onto Finder, Windows Explorer, Nautilus, etc.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
pub use linux::start_file_drag;
#[cfg(target_os = "macos")]
pub use macos::start_file_drag;
#[cfg(windows)]
pub use windows::start_file_drag;

/// Outcome of a drag operation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DragResult {
    /// The drop was accepted by the target application.
    /// 현재 macOS 구현만 구성한다 — Windows/Linux 스텁은 Cancelled 고정이라
    /// 해당 타깃 빌드에선 dead_code 로 보인다 (크로스 플랫폼 API 표면 유지).
    #[allow(dead_code)]
    Accepted,
    /// The drag was cancelled (user released outside a valid target).
    Cancelled,
}
