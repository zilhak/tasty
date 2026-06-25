//! macOS WKWebView wrapper.
//! Reference: wry/src/wkwebview/mod.rs (MIT license, Tauri)

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSAppearance, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSView};
use objc2_foundation::{NSError, NSPoint, NSRect, NSSize, NSString, NSURL};
use objc2_web_kit::{
    WKContentRuleList, WKContentRuleListStore, WKUserContentController, WKWebView,
    WKWebViewConfiguration,
};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

use super::WebViewBounds;

/// 원격(http/https) 서브리소스 전체를 차단하는 WKContentRuleList JSON. 로컬 file:// 는 통과.
const REMOTE_BLOCK_RULE_JSON: &str =
    r#"[{"trigger":{"url-filter":"^https?://"},"action":{"type":"block"}}]"#;

pub struct PlatformWebView {
    webview: Retained<WKWebView>,
    /// 비동기 컴파일된 원격-차단 룰 캐시(완료 전 None). handler 와 공유.
    content_rule_list: Rc<RefCell<Option<Retained<WKContentRuleList>>>>,
    /// 현재 원하는 차단 상태(true=원격 차단, allow_remote=false 대응. 기본 true).
    block_remote: Rc<Cell<bool>>,
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

            // 원격-차단 룰을 비동기 컴파일. 기본 차단(block_remote=true) — completion handler 가
            // 컴파일 완료 시 캐시에 저장하고 현재 상태를 적용한다.
            let content_rule_list: Rc<RefCell<Option<Retained<WKContentRuleList>>>> =
                Rc::new(RefCell::new(None));
            let block_remote = Rc::new(Cell::new(true));
            if let Some(store) = WKContentRuleListStore::defaultStore(mtm) {
                let webview_cb = webview.clone();
                let rule_cb = content_rule_list.clone();
                let block_cb = block_remote.clone();
                // completion handler: main thread 에서 컴파일 완료 시 호출(WebKit 보장).
                let handler =
                    RcBlock::new(move |list: *mut WKContentRuleList, err: *mut NSError| {
                        // SAFETY(외부 unsafe 블록 상속): WebKit 이 main thread 에서 컴파일 완료를
                        // 호출하며 list/err 는 이 시점 valid. list non-null 이면 +0 참조라 보관 위해
                        // retain 한다.
                        if let Some(retained) = Retained::retain(list) {
                            tracing::debug!("WKContentRuleList 컴파일 성공 — 원격 차단 룰 설치");
                            *rule_cb.borrow_mut() = Some(retained);
                            apply_block_state(
                                &webview_cb,
                                block_cb.get(),
                                rule_cb.borrow().as_deref(),
                            );
                        } else if let Some(err) = err.as_ref() {
                            tracing::warn!(
                                "WKContentRuleList compile 실패: {}",
                                err.localizedDescription()
                            );
                        }
                    });
                let id = NSString::from_str("tasty-block-remote");
                let json = NSString::from_str(REMOTE_BLOCK_RULE_JSON);
                store.compileContentRuleListForIdentifier_encodedContentRuleList_completionHandler(
                    Some(&id),
                    Some(&json),
                    Some(&handler),
                );
            } else {
                tracing::warn!("WKContentRuleListStore::defaultStore 없음 — 원격 차단 비활성");
            }

            Ok(Self {
                webview,
                content_rule_list,
                block_remote,
            })
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

    /// 원격(http/https) 콘텐츠 허용 여부. `false`(기본)면 `^https?://` 서브리소스를
    /// `WKContentRuleList` 로 차단, `true`면 차단 해제. 룰이 아직 비동기 컴파일 중이면
    /// (`content_rule_list` None) 상태만 기록하고 컴파일 완료 handler 가 적용한다.
    pub fn set_remote_content_allowed(&self, allowed: bool) {
        self.block_remote.set(!allowed);
        apply_block_state(
            &self.webview,
            self.block_remote.get(),
            self.content_rule_list.borrow().as_deref(),
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

/// 차단 상태를 webview 의 `userContentController` 에 idempotent 하게 반영한다.
/// 항상 기존 룰을 모두 제거한 뒤, 차단이면 룰을 다시 추가(중복 add 방지). 룰이 아직
/// 컴파일되지 않았으면(None) 추가는 생략(완료 handler 가 재적용).
fn apply_block_state(
    webview: &WKWebView,
    block_remote: bool,
    rule_list: Option<&WKContentRuleList>,
) {
    // SAFETY: main thread WKWebView API — configuration()/userContentController() 및
    // add/removeAllContentRuleLists 는 main thread only. 호출 경로(new 의 main-thread
    // completion handler / set_remote_content_allowed)가 main thread 를 보장한다.
    #[allow(clippy::multiple_unsafe_ops_per_block)]
    unsafe {
        let ucc: Retained<WKUserContentController> =
            webview.configuration().userContentController();
        ucc.removeAllContentRuleLists();
        if block_remote && let Some(list) = rule_list {
            ucc.addContentRuleList(list);
        }
    }
    tracing::debug!(
        block_remote,
        rule_compiled = rule_list.is_some(),
        "webview 원격 차단 상태 적용 (removeAll + 차단 시 add)"
    );
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
