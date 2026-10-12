//! 플랫폼별 native WebView와 공통 키 전달 계약.
//! 백엔드는 같은 이름의 메서드를 제공하고 cfg로 선택한다. 차이는 docs/design/systems/webview.md를 따른다.

pub mod keys;
mod load_generation;
pub mod script_gate;

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

/// 생성 실패를 재시도 정책에 맞게 분류한다. native 창 생성·삭제가 다음 시도를 유발할 수 있어 상한이 필요하다.
#[derive(Debug, Clone)]
pub enum WebViewCreateError {
    /// 다음 시도를 허용하는 분류. 성공 가능성을 보장하지 않는다.
    // 이유: macOS 생성 경로는 이 분류를 사용하지 않는다.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    Transient(String),
    /// 현재 생성 요청에서 재시도하지 않을 실패로 분류한 값.
    Permanent(String),
}

impl std::fmt::Display for WebViewCreateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transient(m) => write!(f, "{m} (일시)"),
            Self::Permanent(m) => write!(f, "{m} (영구)"),
        }
    }
}

impl WebViewCreateError {
    pub fn is_permanent(&self) -> bool {
        matches!(self, Self::Permanent(_))
    }
}

pub use crate::model::NavState;

pub use keys::{HostShortcutPolicy, ShortcutSources, WebViewKeyBridge, WebViewKeyEvent};

/// backend가 기록한 탐색 시도. user_gesture는 Linux·Windows 엔진의 보고값이며 macOS는 false다.
/// 제스처 안에서 페이지 스크립트가 바꾼 목적지도 포함될 수 있어 목적지에 대한 별도 사용자 동의는 아니다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingNavigation {
    pub url: String,
    pub user_gesture: bool,
}

/// 사용자가 이 surface에서 직접 시작하는 탐색. 호스트 로드와 달리 사용자가 보고 있는 문서로
/// 처리한다(`HtmlScriptState::on_user_load_requested`). 에이전트 경로는 이 값을 만들지 않는다.
// 이유: release에서 이 값을 만드는 주소 표시줄 UI가 아직 없다. debug IPC가 같은 경로를 재현한다.
#[cfg_attr(not(debug_assertions), allow(dead_code))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebViewUserNav {
    Back,
    Forward,
    Reload,
    /// 해석을 마친 주소(`file://`·`http(s)://`)를 연다. 상대 경로 해석과 파일 존재 확인은 호출자가 한다.
    Open(String),
}

/// 에이전트가 IPC로 요청한 히스토리 이동. 호스트 출처 로드로 처리한다(`HtmlScriptState::on_host_load_requested`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebViewHistoryStep {
    Back,
    Forward,
    Reload,
}

impl WebViewHistoryStep {
    /// 이 이동이 로드를 일으키는지. 히스토리 처음의 뒤로, 끝의 앞으로, 문서 없는 새로고침은 아니다.
    /// 사용자 탐색(`WebViewUserNav`)의 같은 조작도 이 기준을 쓴다.
    pub fn loads(self, info: &crate::model::WebViewNavInfo) -> bool {
        match self {
            Self::Back => info.can_go_back,
            Self::Forward => info.can_go_forward,
            Self::Reload => info.url.is_some(),
        }
    }
}

/// 논리 좌표의 사각형. 물리 좌표 변환은 to_physical/from_physical로 모은다.
/// 플랫폼 호출 전 정수 절단의 경계가 바뀌지 않도록 f64를 유지한다.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WebViewBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// 물리 픽셀 사각형. 플랫폼별 정수 변환은 호출자가 수행한다.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PhysicalWebViewBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl WebViewBounds {
    pub fn from_physical(physical: PhysicalWebViewBounds, scale_factor: f64) -> Self {
        Self {
            x: physical.x / scale_factor,
            y: physical.y / scale_factor,
            width: physical.width / scale_factor,
            height: physical.height / scale_factor,
        }
    }

    /// 논리 좌표를 물리 픽셀로 바꾼다. Cocoa는 point를 사용하므로 이 변환을 쓰지 않는다.
    // 이유: macOS에도 왕복 검사를 남기기 위해 함수를 유지한다.
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
mod navigable_url_tests {
    use super::is_navigable_url;

    #[test]
    fn file_and_http_schemes_open_as_addresses() {
        for url in [
            "file:///tmp/a.html",
            "http://localhost:8080",
            "https://tasty.dev",
        ] {
            assert!(is_navigable_url(url), "{url}");
        }
    }

    #[test]
    fn anything_else_loads_as_raw_html() {
        for url in [
            "<!doctype html><html></html>",
            "about:blank",
            "data:text/html,x",
            "",
        ] {
            assert!(!is_navigable_url(url), "{url}");
        }
    }
}

#[cfg(test)]
mod bounds_tests {
    use super::{PhysicalWebViewBounds, WebViewBounds};

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

/// 페이지 색상 모드 요청. 실제 적용 지원은 backend마다 다르다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorScheme {
    Follow,
    Light,
    Dark,
}

/// plugin 설정에서 해석한 페이지 설정. sandbox_scripts는 JS 비활성화 요청이며 별도 격리 엔진이 아니다.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HtmlWebViewSettings {
    pub zoom_percent: f64,
    pub javascript_enabled: bool,
    pub allow_remote_content: bool,
    pub color_scheme: ColorScheme,
}

impl Default for HtmlWebViewSettings {
    fn default() -> Self {
        Self {
            zoom_percent: 100.0,
            javascript_enabled: false,
            allow_remote_content: false,
            color_scheme: ColorScheme::Follow,
        }
    }
}

/// `webview.set_url`의 url을 주소로 열지(true) raw HTML로 실을지(false) 정한다.
/// 로드 분기와 host chrome의 URL 줄 표시가 같은 판정을 쓰도록 한 곳에 둔다.
pub fn is_navigable_url(url: &str) -> bool {
    url.starts_with("file://") || url.starts_with("http://") || url.starts_with("https://")
}

/// 같은 plugin 설정을 읽고 수정하도록 surface 종류를 설정 소유 plugin에 연결한다.
pub fn webview_settings_plugin_id(kind: &str) -> Option<&'static str> {
    match kind {
        "html" => Some("com.tasty.html"),
        "markdown" => Some("com.tasty.markdown"),
        _ => None,
    }
}

impl HtmlWebViewSettings {
    /// backend에 설정을 요청한다. 비동기 필터 준비·플랫폼의 미지원 항목 때문에 즉시 적용을 보장하지 않는다.
    pub fn apply(&self, wv: &PlatformWebView) {
        wv.set_zoom(self.zoom_percent / 100.0);
        wv.set_javascript_enabled(self.javascript_enabled);
        wv.set_color_scheme(self.color_scheme);
        wv.set_remote_content_allowed(self.allow_remote_content);
    }
}
