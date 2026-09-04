//! Cross-platform WebView wrapper.
//!
//! Provides a minimal native webview that can be embedded as a child view
//! inside a winit/wgpu window. Lifecycle/geometry surface is 6 operations:
//! create, set_bounds, set_visible, load_url, load_html, drop.
//!
//! 거기에 **키보드 계약**이 하나 더 붙는다([`keys`]). native webview 는 winit 창과
//! 별개의 OS 자식 창/뷰라 자기가 키보드 포커스를 잡으면 host 단축키가 통째로
//! 도달하지 못한다 — 세 백엔드는 자기 native 키 이벤트를 [`WebViewKeyEvent`] 로
//! 정규화해 [`WebViewKeyBridge`] 에 올리고, 우선순위 판정은 그 한 곳에서만 한다.
//! 배경·대안은 `docs/adr/0102-webview-key-forwarding.md`.

pub mod keys;

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

pub use keys::{HostShortcutPolicy, WebViewKeyBridge, WebViewKeyEvent};

/// Logical bounds for a webview (in logical pixels, origin at top-left).
///
/// 이 타입과 [`PhysicalWebViewBounds`] 는 한 쌍이다 — 좌표계가 필드 주석이 아니라
/// **타입 이름**에 남고, 둘 사이는 [`WebViewBounds::to_physical`] /
/// [`WebViewBounds::from_physical`] 로만 오간다. 생산자(레이아웃)와 소비자(플랫폼 창
/// API)는 서로 다른 파일에 있고 그 둘의 나눗셈·곱셈이 정확히 상쇄해야 창이 제자리에
/// 온다 — 각자 `/ scale_factor` · `* scale_factor` 를 손으로 적으면 한쪽만 고쳤을 때
/// 조용히 어긋나므로, 변환은 이 두 함수 밖에 두지 않는다.
///
/// `f32`([`tasty_type_geometry::length::LogicalPx`])가 아니라 `f64` 인 이유는 두
/// 가지다. ① 플랫폼 창 API(GTK/Win32/Cocoa)와 winit `scale_factor` 가 `f64` 다.
/// ② 소비자가 `as i32` 로 **절단**하므로, 정밀도를 바꾸면 정수 경계에 걸친 값의
/// 절단 방향이 뒤집혀 창이 1px 움직일 수 있다.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WebViewBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// 플랫폼 창 API 에 그대로 넘길 **물리(device) 픽셀** 사각형 — [`WebViewBounds`] 의 짝.
///
/// 최종 정수 캐스팅(`as i32` / `as u32`)은 플랫폼 API 가 요구하는 형이 달라 호출부에
/// 남긴다. 이 타입이 보장하는 것은 "여기 담긴 값은 이미 물리 픽셀" 이라는 사실이다.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PhysicalWebViewBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl WebViewBounds {
    /// 물리 사각형을 논리 좌표로 내린다. 산술은 종전 호출부가 하던 것과 같다.
    pub fn from_physical(physical: PhysicalWebViewBounds, scale_factor: f64) -> Self {
        Self {
            x: physical.x / scale_factor,
            y: physical.y / scale_factor,
            width: physical.width / scale_factor,
            height: physical.height / scale_factor,
        }
    }

    /// 논리 좌표를 플랫폼 창 API 용 물리 사각형으로 올린다.
    ///
    /// macOS 는 Cocoa 가 논리 좌표(point)를 그대로 받으므로 이 변환을 쓰지 않는다 —
    /// 물리로 올리는 쪽은 X11(GTK)·Win32 다.
    ///
    /// 그래서 macOS 빌드에서는 호출부가 없다. `-D dead-code` 아래서 그것이 컴파일
    /// 에러가 되므로 그 플랫폼에서만 면제한다 — 지우거나 `#[cfg]` 로 빼지 않는 이유는
    /// 아래 왕복 테스트가 세 OS 모두에서 이 함수를 `from_physical` 의 역으로 고정하기
    /// 때문이다. macOS 에서만 그 고정이 사라지면 한쪽만 바뀌는 것을 못 잡는다.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    pub fn to_physical(self, scale_factor: f64) -> PhysicalWebViewBounds {
        PhysicalWebViewBounds {
            x: self.x * scale_factor,
            y: self.y * scale_factor,
            width: self.width * scale_factor,
            height: self.height * scale_factor,
        }
    }
}

#[cfg(test)]
mod bounds_tests {
    use super::{PhysicalWebViewBounds, WebViewBounds};

    /// 생산자(`from_physical`)와 소비자(`to_physical`)가 서로를 상쇄하는지 고정한다.
    /// 둘은 서로 다른 파일에서 불리므로, 한쪽만 바뀌면 웹뷰가 조용히 어긋난다.
    #[test]
    fn the_physical_round_trip_returns_the_original_rect() {
        let physical = PhysicalWebViewBounds {
            x: 1920.0,
            y: 48.0,
            width: 1280.0,
            height: 720.0,
        };
        for sf in [1.0_f64, 1.25, 1.5, 1.75, 2.0, 2.4] {
            let logical = WebViewBounds::from_physical(physical, sf);
            let back = logical.to_physical(sf);
            for (got, want) in [
                (back.x, physical.x),
                (back.y, physical.y),
                (back.width, physical.width),
                (back.height, physical.height),
            ] {
                assert!(
                    (got - want).abs() < 1e-9,
                    "sf={sf}: {got} != {want} — 왕복이 상쇄되지 않으면 창이 어긋난다"
                );
            }
        }
    }

    /// `from_physical` 이 실제로 논리 좌표(=물리 ÷ scale_factor)를 만든다.
    #[test]
    fn from_physical_divides_by_the_scale_factor() {
        let logical = WebViewBounds::from_physical(
            PhysicalWebViewBounds {
                x: 200.0,
                y: 100.0,
                width: 800.0,
                height: 600.0,
            },
            2.0,
        );
        assert_eq!(
            logical,
            WebViewBounds {
                x: 100.0,
                y: 50.0,
                width: 400.0,
                height: 300.0,
            }
        );
    }
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

/// webview surface kind 문자열 → 그 kind 가 소비하는 generic `plugin_settings` 슬롯의
/// plugin_id. 현재는 html(`com.tasty.html`) 만 generic 설정을 갖는다(다른 kind 는 `None` —
/// 호출자는 default 로 안전 fallback). read 경로(`resolve_webview_settings`)와 write 경로
/// (zoom 단축키 재배선, `adapters/ui/input/shortcuts/zoom.rs`)가 이 매핑을 공유해 두
/// 슬롯이 어긋나지 않게 한다.
pub fn webview_settings_plugin_id(kind: &str) -> Option<&'static str> {
    match kind {
        "html" => Some("com.tasty.html"),
        "markdown" => Some("com.tasty.markdown"),
        _ => None,
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
