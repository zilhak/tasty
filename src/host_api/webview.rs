//! Cross-platform WebView wrapper.
//!
//! Provides a minimal native webview that can be embedded as a child view
//! inside a winit/wgpu window. Only 6 operations are needed:
//! create, set_bounds, set_visible, load_url, load_html, drop.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
pub use linux::PlatformWebView;
#[cfg(target_os = "macos")]
pub use macos::PlatformWebView;

#[cfg(windows)]
pub use self::windows::PlatformWebView;

/// gui 코드 편의용 재노출. 정의처는 비-gui 모듈 `plugin_bridge::remote_surface`
/// (webview 모듈이 `#[cfg(feature = "gui")]` 게이트라 비-gui 의 RemoteSurface 가
/// 참조할 수 있도록 그곳에 둔다). backend 들은 `super::NavState`, host gui 코드는
/// `crate::webview::NavState` 로 참조.
pub use crate::plugin_bridge::remote_surface::NavState;

/// Logical bounds for a webview (in logical pixels, origin at top-left).
#[derive(Debug, Clone, Copy)]
pub struct WebViewBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// `prefers-color-scheme` override applied to an HTML webview (host-driven from
/// the `com.tasty.html` plugin's `color_scheme` setting).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorScheme {
    /// Follow the system / app appearance (no override).
    Follow,
    Light,
    Dark,
}

/// Host 가 webview 에 적용하는 해석된 HTML viewer 설정. `com.tasty.html` 의
/// `plugin_settings` 슬롯(또는 부재 시 manifest default)에서 도출한다.
///
/// `javascript_enabled` = `!sandbox_scripts` — "Sandbox scripts" 토글이 on(기본)이면
/// 스크립트를 보수적으로 격리(JS off)하고, off 면 실행을 허용한다.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HtmlWebViewSettings {
    pub zoom_percent: f64,
    pub javascript_enabled: bool,
    pub allow_remote_content: bool,
    pub color_scheme: ColorScheme,
}

impl Default for HtmlWebViewSettings {
    fn default() -> Self {
        // manifest default: zoom 100 / sandbox true(→JS off) / remote false / scheme follow.
        Self {
            zoom_percent: 100.0,
            javascript_enabled: false,
            allow_remote_content: false,
            color_scheme: ColorScheme::Follow,
        }
    }
}

impl HtmlWebViewSettings {
    /// 4개 backend 제어 메서드에 적용한다. zoom·JS 는 3 OS 실효. remote 는 3 OS 모두
    /// 실효(macOS=WKContentRuleList / Windows=WebResourceRequested / Linux=decide-policy,
    /// 단 Win/Linux 는 이 세션 미검증·Linux 는 서브리소스 한계). color_scheme 은 macOS 만 실효.
    pub fn apply(&self, wv: &PlatformWebView) {
        wv.set_zoom(self.zoom_percent / 100.0);
        wv.set_javascript_enabled(self.javascript_enabled);
        wv.set_color_scheme(self.color_scheme);
        wv.set_remote_content_allowed(self.allow_remote_content);
    }
}
