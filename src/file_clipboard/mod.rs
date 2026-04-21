//! Cross-platform file clipboard operations.
//!
//! Provides copy/cut/paste of files via OS-native clipboard formats,
//! enabling interop with Finder, Nautilus, Windows Explorer.

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "macos")]
pub use macos::{get_file_clipboard, set_file_clipboard};

#[cfg(windows)]
mod windows;

#[cfg(windows)]
pub use windows::{get_file_clipboard, set_file_clipboard};

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
pub use linux::{get_file_clipboard, set_file_clipboard};

/// Whether files were copied or cut.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FileClipboardOp {
    Copy,
    Cut,
}
