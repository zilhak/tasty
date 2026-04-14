//! Cross-platform WebView wrapper.
//!
//! Provides a minimal native webview that can be embedded as a child view
//! inside a winit/wgpu window. Only 6 operations are needed:
//! create, set_bounds, set_visible, load_url, load_html, drop.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::PlatformWebView;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use self::windows::PlatformWebView;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::PlatformWebView;

/// Logical bounds for a webview (in logical pixels, origin at top-left).
#[derive(Debug, Clone, Copy)]
pub struct WebViewBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
