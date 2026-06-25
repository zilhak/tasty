//! macOS WKWebView wrapper.
//! Reference: wry/src/wkwebview/mod.rs (MIT license, Tauri)

use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSAppearance, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSView};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString, NSURL};
use objc2_web_kit::{WKWebView, WKWebViewConfiguration};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

use super::WebViewBounds;

pub struct PlatformWebView {
    webview: Retained<WKWebView>,
}

impl PlatformWebView {
    /// Create a WKWebView as a child of the given window, positioned at `bounds`.
    pub fn new(
        window: &impl HasWindowHandle,
        bounds: WebViewBounds,
        scale_factor: f64,
    ) -> Result<Self, String> {
        let mtm =
            MainThreadMarker::new().ok_or_else(|| "Must be called from main thread".to_string())?;

        let ns_view_ptr = match window.window_handle().map_err(|e| e.to_string())?.as_raw() {
            RawWindowHandle::AppKit(w) => w.ns_view.as_ptr(),
            _ => return Err("Not an AppKit window".to_string()),
        };
        // SAFETY: ns_view_ptr는 winit이 만든 활성 NSView로, 본 함수 호출 동안 살아있다
        // (winit이 윈도우를 drop하지 않는 한). mtm 검증 통과로 main thread 확정.
        let ns_view: &NSView = unsafe { &*(ns_view_ptr as *const NSView) };

        // SAFETY: mtm으로 main thread 확정. WKWebView/WKPreferences API는 main thread only.
        // msg_send![setValue:forKey:]는 NSString 두 객체에 대한 KVC — 같은 thread, 같은 호출 흐름.
        // WKWebView init + addSubview 시퀀스는 한 setup 단위라 분할 시 가독성 저하.
        #[allow(clippy::multiple_unsafe_ops_per_block)]
        unsafe {
            let config = WKWebViewConfiguration::new(mtm);

            // Set default text encoding to UTF-8 (matches browser behavior for charset-less HTML)
            let prefs = config.preferences();
            let key = NSString::from_str("defaultTextEncodingName");
            let value = NSString::from_str("UTF-8");
            let _: () = objc2::msg_send![&prefs, setValue: &*value, forKey: &*key];

            let frame = logical_to_nsrect(ns_view, bounds, scale_factor);

            let webview =
                WKWebView::initWithFrame_configuration(WKWebView::alloc(mtm), frame, &config);

            ns_view.addSubview(&webview);

            Ok(Self { webview })
        }
    }

    /// Update the webview position and size.
    pub fn set_bounds(&self, bounds: WebViewBounds, scale_factor: f64) {
        // SAFETY: main thread에서만 호출 — PlatformWebView는 main thread 객체
        // (Retained<WKWebView>이므로 !Send/!Sync 기본). logical_to_nsrect도 main thread.
        // superview/setFrame 호출이 한 묶음이라 분할 불필요.
        #[allow(clippy::multiple_unsafe_ops_per_block)]
        unsafe {
            if let Some(parent) = self.webview.superview() {
                let frame = logical_to_nsrect(&parent, bounds, scale_factor);
                self.webview.setFrame(frame);
            }
        }
    }

    /// Show or hide the webview.
    pub fn set_visible(&self, visible: bool) {
        self.webview.setHidden(!visible);
    }

    /// Navigate to a URL (supports file:// for local files).
    /// For file:// URLs, uses `loadFileURL:allowingReadAccessToURL:` with the
    /// parent directory as the access scope, so relative resources (CSS, JS,
    /// images, iframes) in the same directory tree are accessible.
    pub fn load_url(&self, url: &str) {
        // SAFETY: main thread WKWebView API. NSString/NSURL은 호출 동안 살아있는 local Retained.
        // URL loading 시퀀스는 한 단위라 분할 시 가독성 저하.
        #[allow(clippy::multiple_unsafe_ops_per_block)]
        unsafe {
            if let Some(path) = url.strip_prefix("file://") {
                // Use fileURLWithPath for proper percent-encoding of CJK paths
                let file_url = NSURL::fileURLWithPath(&NSString::from_str(path));
                // Allow read access to parent directory for relative resources
                let dir_path = std::path::Path::new(path)
                    .parent()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| "/".to_string());
                let dir_url =
                    NSURL::fileURLWithPath_isDirectory(&NSString::from_str(&dir_path), true);
                self.webview
                    .loadFileURL_allowingReadAccessToURL(&file_url, &dir_url);
            } else {
                let ns_url = NSURL::URLWithString(&NSString::from_str(url));
                if let Some(ns_url) = ns_url {
                    let request = objc2_foundation::NSURLRequest::requestWithURL(&ns_url);
                    self.webview.loadRequest(&request);
                }
            }
        }
    }

    /// Content zoom (1.0 = 100%). WKWebView `pageZoom` 은 텍스트+이미지 전체를 배율 적용한다.
    pub fn set_zoom(&self, factor: f64) {
        // SAFETY: main thread WKWebView property. self 는 main thread 객체(Retained<WKWebView>).
        unsafe {
            let _: () = objc2::msg_send![&self.webview, setPageZoom: factor];
        }
    }

    /// JavaScript 실행 허용 여부. WKPreferences `javaScriptEnabled` — 다음 네비게이션부터 적용.
    /// host 는 "Sandbox scripts" on(기본) → `enabled=false`(스크립트 격리), off → `true` 로 건다.
    pub fn set_javascript_enabled(&self, enabled: bool) {
        // SAFETY: main thread. configuration().preferences() 는 main thread KVC 대상.
        #[allow(clippy::multiple_unsafe_ops_per_block)]
        unsafe {
            let config = self.webview.configuration();
            let prefs = config.preferences();
            let _: () = objc2::msg_send![&prefs, setJavaScriptEnabled: enabled];
        }
    }

    /// `prefers-color-scheme` 강제. WKWebView 는 NSView 라 `setAppearance:`
    /// (NSAppearanceCustomization) 로 effective appearance 를 고정할 수 있고, 웹 콘텐츠의
    /// `prefers-color-scheme` 미디어쿼리가 이를 따른다. Follow=상속(nil), Light=Aqua,
    /// Dark=DarkAqua. 적용은 즉시(렌더 갱신).
    pub fn set_color_scheme(&self, scheme: super::ColorScheme) {
        // SAFETY: main thread AppKit. self 는 main thread 객체(Retained<WKWebView>),
        // WKWebView : NSView 가 setAppearance: 에 응답한다. appearanceNamed: 와
        // NSAppearanceName* 정적은 main thread AppKit API.
        #[allow(clippy::multiple_unsafe_ops_per_block)]
        unsafe {
            let appearance: Option<Retained<NSAppearance>> = match scheme {
                super::ColorScheme::Follow => None,
                super::ColorScheme::Light => NSAppearance::appearanceNamed(NSAppearanceNameAqua),
                super::ColorScheme::Dark => NSAppearance::appearanceNamed(NSAppearanceNameDarkAqua),
            };
            let _: () = objc2::msg_send![&self.webview, setAppearance: appearance.as_deref()];
        }
    }

    /// 원격(http/https) 콘텐츠 허용 여부. WKWebView 에는 동기 toggle 이 없다 — 차단하려면
    /// `WKContentRuleList`(`compileContentRuleList...` 는 **completion-handler block 기반
    /// async** 컴파일 후 `userContentController` 에 add/remove) 또는 `WKNavigationDelegate`
    /// 객체(별도 objc2 class 정의)가 필요하다. 둘 다 block/delegate 수명 관리가 얽혀
    /// half-baked 위험이 커 현재 no-op 으로 둔다(후속 backend 작업 필요).
    pub fn set_remote_content_allowed(&self, allowed: bool) {
        tracing::debug!(
            "set_remote_content_allowed({allowed}) — macOS WKWebView no-op \
             (WKContentRuleList async 컴파일 또는 WKNavigationDelegate 객체 필요)"
        );
    }

    /// Load HTML string directly.
    pub fn load_html(&self, html: &str) {
        // SAFETY: main thread WKWebView API. NSString/NSURL은 호출 동안 살아있는 local Retained.
        unsafe {
            let ns_html = NSString::from_str(html);
            let base_url = NSURL::URLWithString(&NSString::from_str("about:blank"));
            self.webview
                .loadHTMLString_baseURL(&ns_html, base_url.as_deref());
        }
    }
}

impl Drop for PlatformWebView {
    fn drop(&mut self) {
        self.webview.removeFromSuperview();
    }
}

/// Convert logical bounds (top-left origin) to NSRect,
/// handling macOS coordinate system (bottom-left origin for non-flipped views).
unsafe fn logical_to_nsrect(parent: &NSView, bounds: WebViewBounds, _scale_factor: f64) -> NSRect {
    let is_flipped = parent.isFlipped();
    let parent_h = parent.frame().size.height;

    let origin_y = if is_flipped {
        bounds.y
    } else {
        parent_h - bounds.y - bounds.height
    };

    NSRect {
        origin: NSPoint::new(bounds.x, origin_y),
        size: NSSize::new(bounds.width, bounds.height),
    }
}
