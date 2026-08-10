//! macOS WKWebView wrapper.
//! Reference: wry/src/wkwebview/mod.rs (MIT license, Tauri)

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use block2::{DynBlock, RcBlock};
use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSAppearance, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSView};
use objc2_foundation::{NSError, NSPoint, NSRect, NSSize, NSString, NSURL};
use objc2_web_kit::{
    WKContentRuleList, WKContentRuleListStore, WKNavigation, WKNavigationAction,
    WKNavigationActionPolicy, WKNavigationDelegate, WKUserContentController, WKWebView,
    WKWebViewConfiguration,
};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

use super::{NavState, WebViewBounds};

/// `NavDelegate` 의 ivar — host 와 공유하는 navigation 상태 셀.
struct NavDelegateIvars {
    nav_state: Rc<Cell<NavState>>,
    /// decidePolicyForNavigationAction 이 캡처한, 아직 host 에 통지되지 않은 navigation
    /// 시도 URL 큐(도착 순서 보존). host `sync_webviews` 가 매 프레임
    /// `take_pending_navigations` 로 비우고 plugin 에 forward — "원격 http(s) 차단"
    /// (WKContentRuleList, 이 delegate 와 무관하게 독립 동작)과는 별개로 차단 여부와
    /// 무관하게 모든 navigation 시도마다 쌓인다.
    pending_navigations: Rc<RefCell<Vec<String>>>,
}

define_class!(
    // SAFETY:
    // - 상위 클래스 NSObject 는 서브클래싱 제약이 없다.
    // - `MainThreadOnly` 가 맞다: WKWebView/WKNavigationDelegate 는 main thread 전용이며
    //   이 delegate 는 main thread 에서만 생성·호출된다(WebKit 이 콜백을 main thread 에서 발화).
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "TastyNavDelegate"]
    #[ivars = NavDelegateIvars]
    struct NavDelegate;

    unsafe impl NSObjectProtocol for NavDelegate {}

    // WKNavigationDelegate: navigation 생명주기 콜백. start/finish/fail* 만 구현(나머지 optional).
    unsafe impl WKNavigationDelegate for NavDelegate {
        #[unsafe(method(webView:didStartProvisionalNavigation:))]
        fn did_start_provisional(&self, _web_view: &WKWebView, _navigation: Option<&WKNavigation>) {
            self.ivars().nav_state.set(NavState::Loading);
        }

        #[unsafe(method(webView:didFinishNavigation:))]
        fn did_finish(&self, _web_view: &WKWebView, _navigation: Option<&WKNavigation>) {
            self.ivars().nav_state.set(NavState::Done);
        }

        #[unsafe(method(webView:didFailNavigation:withError:))]
        fn did_fail(
            &self,
            _web_view: &WKWebView,
            _navigation: Option<&WKNavigation>,
            error: &NSError,
        ) {
            // 사유는 로그 전용 — 화면 error chrome 은 URL 만 보여준다.
            tracing::warn!(
                "WKWebView navigation failed: {}",
                error.localizedDescription()
            );
            self.ivars().nav_state.set(NavState::Failed);
        }

        #[unsafe(method(webView:didFailProvisionalNavigation:withError:))]
        fn did_fail_provisional(
            &self,
            _web_view: &WKWebView,
            _navigation: Option<&WKNavigation>,
            error: &NSError,
        ) {
            tracing::warn!(
                "WKWebView provisional navigation failed: {}",
                error.localizedDescription()
            );
            self.ivars().nav_state.set(NavState::Failed);
        }

        /// navigation 시도(사용자 클릭·페이지 이동) URL 캡처용. 차단 여부 판정에는 관여하지
        /// 않는다 — 원격(http/https) 차단은 `WKContentRuleList`(이 메서드와 완전히 독립,
        /// `apply_block_state` 참조)가 서브리소스 레벨에서 이미 처리하므로, 여기서는 항상
        /// `.Allow` 를 돌려준다.
        #[unsafe(method(webView:decidePolicyForNavigationAction:decisionHandler:))]
        fn decide_policy(
            &self,
            _web_view: &WKWebView,
            navigation_action: &WKNavigationAction,
            decision_handler: &DynBlock<dyn Fn(WKNavigationActionPolicy)>,
        ) {
            // SAFETY: main thread WebKit delegate 호출(WKNavigationDelegate 는
            // MainThreadOnly). request()/URL()/absoluteString() 은 이 호출 동안 살아있는
            // Retained 값을 반환하는 main thread AppKit/Foundation API.
            let url = unsafe {
                navigation_action
                    .request()
                    .URL()
                    .and_then(|u| u.absoluteString())
            };
            if let Some(url) = url {
                self.ivars()
                    .pending_navigations
                    .borrow_mut()
                    .push(url.to_string());
            }
            decision_handler.call((WKNavigationActionPolicy::Allow,));
        }
    }
);

impl NavDelegate {
    /// main thread 에서 ivar(nav_state 공유 셀 + pending_navigations 큐)를 담아 delegate
    /// 인스턴스를 만든다.
    fn new(
        mtm: MainThreadMarker,
        nav_state: Rc<Cell<NavState>>,
        pending_navigations: Rc<RefCell<Vec<String>>>,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(NavDelegateIvars {
            nav_state,
            pending_navigations,
        });
        // SAFETY: NSObject 의 지정 초기화자 init 을 super 로 호출.
        unsafe { msg_send![super(this), init] }
    }
}

/// 원격(http/https) 서브리소스 전체를 차단하는 WKContentRuleList JSON. 로컬 file:// 는 통과.
const REMOTE_BLOCK_RULE_JSON: &str =
    r#"[{"trigger":{"url-filter":"^https?://"},"action":{"type":"block"}}]"#;

pub struct PlatformWebView {
    webview: Retained<WKWebView>,
    /// 비동기 컴파일된 원격-차단 룰 캐시(완료 전 None). handler 와 공유.
    content_rule_list: Rc<RefCell<Option<Retained<WKContentRuleList>>>>,
    /// 현재 원하는 차단 상태(true=원격 차단, allow_remote=false 대응. 기본 true).
    block_remote: Rc<Cell<bool>>,
    /// navigation 생명주기 상태(기본 Idle). NavDelegate 콜백이 갱신, host sync_webviews 가
    /// `nav_state()` 로 read. 콜백이 전부 main thread 발화라 `Rc<Cell>` 로 충분(block_remote 동일).
    nav_state: Rc<Cell<NavState>>,
    /// decidePolicyForNavigationAction 이 캡처한 navigation 시도 URL 큐. NavDelegate 와
    /// 공유(Rc) — `take_pending_navigations` 로 host 가 매 프레임 비운다.
    pending_navigations: Rc<RefCell<Vec<String>>>,
    /// WKWebView 는 navigationDelegate 를 weak 참조하므로 delegate 를 여기 보관해 생명주기 유지.
    _nav_delegate: Retained<NavDelegate>,
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

            // navigation 생명주기 delegate. start→Loading / finish→Done / fail*→Failed.
            // WKWebView 가 weak 참조하므로 Retained 를 struct 필드(_nav_delegate)로 보관.
            let nav_state = Rc::new(Cell::new(NavState::Idle));
            let pending_navigations: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
            let nav_delegate =
                NavDelegate::new(mtm, nav_state.clone(), pending_navigations.clone());
            let nav_proto = ProtocolObject::from_ref(&*nav_delegate);
            webview.setNavigationDelegate(Some(nav_proto));

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
                nav_state,
                pending_navigations,
                _nav_delegate: nav_delegate,
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
    /// 현재 navigation 생명주기 상태(NavDelegate 콜백이 갱신).
    pub fn nav_state(&self) -> NavState {
        self.nav_state.get()
    }

    /// decidePolicyForNavigationAction 이 캡처한 navigation 시도 URL 을 도착 순서대로
    /// 비워서 반환한다. host `sync_webviews` 가 매 프레임 호출해 plugin 에 forward.
    pub fn take_pending_navigations(&self) -> Vec<String> {
        std::mem::take(&mut *self.pending_navigations.borrow_mut())
    }

    pub fn load_url(&self, url: &str) {
        // 콜백이 늦게 와도 즉시 spinner 가 뜨도록 Loading 선반영.
        self.nav_state.set(NavState::Loading);
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
        // Loading 선반영(load_url 과 동일 — 콜백 지연 대비).
        self.nav_state.set(NavState::Loading);
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
///
/// # Safety
///
/// 호출자는 메인 스레드에서 호출해야 한다 — `parent.frame()`/
/// `parent.isFlipped()`는 AppKit 뷰 상태 조회이며 AppKit은 메인 스레드
/// 전용이다(objc2의 `frame()`/`isFlipped()` 자체는 안전한 `fn`으로
/// 노출되어 있어 이 계약은 objc2가 아니라 AppKit의 스레딩 모델에서 온다).
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
