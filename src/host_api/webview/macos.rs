//! macOS WKWebView wrapper.
//! Reference: wry/src/wkwebview/mod.rs (MIT license, Tauri)

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use block2::{DynBlock, RcBlock};
use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSAppearance, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSEvent, NSResponder, NSView,
};
use objc2_foundation::{NSError, NSPoint, NSRect, NSSize, NSString, NSURL};
use objc2_web_kit::{
    WKContentRuleList, WKContentRuleListStore, WKNavigation, WKNavigationAction,
    WKNavigationActionPolicy, WKNavigationDelegate, WKUserContentController, WKWebView,
    WKWebViewConfiguration,
};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

use super::keys::WebViewKeySink;
use super::{NavState, PendingNavigation, WebViewBounds};

struct NavDelegateIvars {
    surface_id: u32,
    nav_state: Rc<Cell<NavState>>,
    /// 차단 여부와 별도로 기록한 탐색 시도 큐.
    pending_navigations: Rc<RefCell<Vec<PendingNavigation>>>,
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

    unsafe impl WKNavigationDelegate for NavDelegate {
        #[unsafe(method(webView:didStartProvisionalNavigation:))]
        fn did_start_provisional(&self, _web_view: &WKWebView, _navigation: Option<&WKNavigation>) {
            let sid = self.ivars().surface_id;
            tracing::debug!("WebView surface {sid}: load started");
            self.ivars().nav_state.set(NavState::Loading);
        }

        #[unsafe(method(webView:didFinishNavigation:))]
        fn did_finish(&self, _web_view: &WKWebView, _navigation: Option<&WKNavigation>) {
            let sid = self.ivars().surface_id;
            tracing::debug!("WebView surface {sid}: load finished");
            self.ivars().nav_state.set(NavState::Done);
        }

        #[unsafe(method(webView:didFailNavigation:withError:))]
        fn did_fail(
            &self,
            _web_view: &WKWebView,
            _navigation: Option<&WKNavigation>,
            error: &NSError,
        ) {
            tracing::warn!(
                "WebView surface {}: WKWebView navigation failed: {}",
                self.ivars().surface_id,
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
                "WebView surface {}: WKWebView provisional navigation failed: {}",
                self.ivars().surface_id,
                error.localizedDescription()
            );
            self.ivars().nav_state.set(NavState::Failed);
        }

        /// 탐색 시도를 기록하고 Allow로 응답한다. 원격 차단은 별도 content rule에 맡긴다.
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
                    .push(PendingNavigation {
                        url: url.to_string(),
                        user_gesture: false,
                    });
            }
            decision_handler.call((WKNavigationActionPolicy::Allow,));
        }
    }
);

impl NavDelegate {
    fn new(
        mtm: MainThreadMarker,
        surface_id: u32,
        nav_state: Rc<Cell<NavState>>,
        pending_navigations: Rc<RefCell<Vec<PendingNavigation>>>,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(NavDelegateIvars {
            surface_id,
            nav_state,
            pending_navigations,
        });
        // SAFETY: NSObject 의 지정 초기화자 init 을 super 로 호출.
        unsafe { msg_send![super(this), init] }
    }
}

struct KeyWebViewIvars {
    surface_id: u32,
    key_bridge: Rc<dyn WebViewKeySink>,
}

define_class!(
    // SAFETY:
    // - WKWebView 는 서브클래싱을 허용한다(wry 등 임베딩 구현의 표준 방식). 지정
    //   초기화자 `initWithFrame:configuration:` 를 아래 `KeyWebView::new` 가 super 로
    //   호출해 올바르게 초기화한다.
    // - `MainThreadOnly` 가 맞다: WKWebView/NSResponder 는 main thread 전용이고 아래
    //   오버라이드는 전부 AppKit 이 main thread 에서 발화한다.
    // - `Drop` 을 구현하지 않는다.
    #[unsafe(super(WKWebView))]
    #[thread_kind = MainThreadOnly]
    #[name = "TastyKeyWebView"]
    #[ivars = KeyWebViewIvars]
    struct KeyWebView;

    unsafe impl NSObjectProtocol for KeyWebView {}

    impl KeyWebView {
        /// key equivalent는 뷰 전체에 전달될 수 있어 이 WebView가 포커스를 가진 경우만 처리한다.
        /// define_class가 Bool 반환 shim을 만들므로 return 대신 마지막 bool 표현식으로 값을 돌려준다.
        #[unsafe(method(performKeyEquivalent:))]
        fn perform_key_equivalent(&self, event: &NSEvent) -> bool {
            if view_holds_first_responder(self) && self.forward_key_to_host(event) {
                true
            } else {
                // SAFETY: main thread AppKit responder chain. super 구현에 위임.
                unsafe { msg_send![super(self), performKeyEquivalent: event] }
            }
        }

        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            if self.forward_key_to_host(event) {
                return;
            }
            // SAFETY: main thread AppKit responder chain. super 구현에 위임.
            unsafe { msg_send![super(self), keyDown: event] }
        }

        /// native 클릭을 host에 알리고 원래 이벤트 처리는 super에 맡긴다.
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            let ivars = self.ivars();
            ivars.key_bridge.note_focus(ivars.surface_id);
            // SAFETY: main thread AppKit. super 구현에 위임.
            unsafe { msg_send![super(self), mouseDown: event] }
        }

        /// 내부 content view가 first responder가 되면 mouseDown이 여기 오지 않을 수 있어 포커스 경로도 알린다.
        #[unsafe(method(becomeFirstResponder))]
        fn become_first_responder(&self) -> bool {
            let ivars = self.ivars();
            ivars.key_bridge.note_focus(ivars.surface_id);
            // SAFETY: main thread AppKit. super 구현에 위임.
            unsafe { msg_send![super(self), becomeFirstResponder] }
        }
    }
);

impl KeyWebView {
    fn new(
        mtm: MainThreadMarker,
        frame: NSRect,
        config: &WKWebViewConfiguration,
        surface_id: u32,
        key_bridge: Rc<dyn WebViewKeySink>,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(KeyWebViewIvars {
            surface_id,
            key_bridge,
        });
        // SAFETY: WKWebView 의 지정 초기화자를 super 로 호출.
        unsafe { msg_send![super(this), initWithFrame: frame, configuration: config] }
    }

    fn forward_key_to_host(&self, event: &NSEvent) -> bool {
        if event.isARepeat() {
            return false;
        }
        let mods = nsevent_mods_to_winit(event.modifierFlags());
        let chars = event.charactersIgnoringModifiers();
        let Some(key) = chars.and_then(|c| ns_chars_to_winit_key(&c.to_string())) else {
            return false;
        };
        let physical = macos_keycode_to_physical(event.keyCode());
        let ivars = self.ivars();
        ivars
            .key_bridge
            .capture_key(ivars.surface_id, key, physical, mods)
    }
}

/// macOS의 레이아웃 독립 keyCode를 winit 물리 키로 변환한다.
fn macos_keycode_to_physical(key_code: u16) -> winit::keyboard::PhysicalKey {
    use winit::platform::scancode::PhysicalKeyExtScancode;
    winit::keyboard::PhysicalKey::from_scancode(key_code as u32)
}

/// 자기 뷰 또는 자손이 first responder인지 확인한다. 키 처리와 포커스 반환에 같은 조건을 쓴다.
fn view_holds_first_responder(view: &NSView) -> bool {
    let Some(window) = view.window() else {
        return false;
    };
    let Some(responder) = window.firstResponder() else {
        return false;
    };
    responder
        .downcast_ref::<NSView>()
        .is_some_and(|v| v.isDescendantOf(view))
}

/// http(s) content-blocker 규칙. 컴파일·설치 전 요청은 이 규칙으로 막지 못한다.
const REMOTE_BLOCK_RULE_JSON: &str =
    r#"[{"trigger":{"url-filter":"^https?://"},"action":{"type":"block"}}]"#;

pub struct PlatformWebView {
    webview: Retained<KeyWebView>,
    content_rule_list: Rc<RefCell<Option<Retained<WKContentRuleList>>>>,
    block_remote: Rc<Cell<bool>>,
    nav_state: Rc<Cell<NavState>>,
    pending_navigations: Rc<RefCell<Vec<PendingNavigation>>>,
    /// WKWebView의 delegate 참조가 weak이므로 여기서 수명을 유지한다.
    _nav_delegate: Retained<NavDelegate>,
}

/// 이 backend의 생성 실패는 Permanent로 분류해 같은 생성 요청을 재시도하지 않는다.
fn perm(msg: impl std::fmt::Display) -> super::WebViewCreateError {
    super::WebViewCreateError::Permanent(msg.to_string())
}

impl PlatformWebView {
    pub fn new(
        window: &impl HasWindowHandle,
        bounds: WebViewBounds,
        scale_factor: f64,
        surface_id: u32,
        key_bridge: Rc<dyn WebViewKeySink>,
    ) -> Result<Self, super::WebViewCreateError> {
        let mtm = MainThreadMarker::new().ok_or_else(|| perm("Must be called from main thread"))?;

        let ns_view_ptr = match window.window_handle().map_err(perm)?.as_raw() {
            RawWindowHandle::AppKit(w) => w.ns_view.as_ptr(),
            _ => return Err(perm("Not an AppKit window")),
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

            let prefs = config.preferences();
            let key = NSString::from_str("defaultTextEncodingName");
            let value = NSString::from_str("UTF-8");
            let _: () = objc2::msg_send![&prefs, setValue: &*value, forKey: &*key];

            let frame = logical_to_nsrect(ns_view, bounds, scale_factor);

            let webview = KeyWebView::new(mtm, frame, &config, surface_id, key_bridge);

            ns_view.addSubview(&webview);

            // WebKit의 delegate는 weak 참조이므로 struct에도 보관한다.
            let nav_state = Rc::new(Cell::new(NavState::Idle));
            let pending_navigations: Rc<RefCell<Vec<PendingNavigation>>> =
                Rc::new(RefCell::new(Vec::new()));
            let nav_delegate = NavDelegate::new(
                mtm,
                surface_id,
                nav_state.clone(),
                pending_navigations.clone(),
            );
            let nav_proto = ProtocolObject::from_ref(&*nav_delegate);
            webview.setNavigationDelegate(Some(nav_proto));

            // 컴파일 완료 전에는 차단 규칙이 없다. 콜백이 현재 설정에 맞춰 설치한다.
            let content_rule_list: Rc<RefCell<Option<Retained<WKContentRuleList>>>> =
                Rc::new(RefCell::new(None));
            let block_remote = Rc::new(Cell::new(true));
            if let Some(store) = WKContentRuleListStore::defaultStore(mtm) {
                let webview_cb = webview.clone();
                let rule_cb = content_rule_list.clone();
                let block_cb = block_remote.clone();
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

    /// 이 WebView나 자손이 first responder일 때만 부모 contentView로 돌린다.
    /// nil로 바꾸면 winit 뷰가 응답자에서 빠질 수 있어 contentView를 명시한다.
    pub fn release_keyboard_focus(&self) {
        let Some(window) = self.webview.window() else {
            return;
        };
        if !view_holds_first_responder(&self.webview) {
            return;
        }
        let Some(content) = window.contentView() else {
            return;
        };
        let content_responder: &NSResponder = &content;
        if !window.makeFirstResponder(Some(content_responder)) {
            tracing::warn!("webview: winit content view 로 first responder 복구 실패");
        }
    }

    pub fn set_visible(&self, visible: bool) {
        self.webview.setHidden(!visible);
    }

    /// delegate 콜백이 기록한 navigation 상태.
    pub fn nav_state(&self) -> NavState {
        self.nav_state.get()
    }

    pub fn take_pending_navigations(&self) -> Vec<PendingNavigation> {
        std::mem::take(&mut *self.pending_navigations.borrow_mut())
    }

    pub fn load_url(&self, url: &str) {
        self.nav_state.set(NavState::Loading);
        // SAFETY: main thread WKWebView API. NSString/NSURL은 호출 동안 살아있는 local Retained.
        // URL loading 시퀀스는 한 단위라 분할 시 가독성 저하.
        #[allow(clippy::multiple_unsafe_ops_per_block)]
        unsafe {
            if let Some(path) = url.strip_prefix("file://") {
                // URL 문자열이 아닌 파일 경로 API로 비ASCII 경로를 전달한다.
                let file_url = NSURL::fileURLWithPath(&NSString::from_str(path));
                // 상대 리소스가 같은 디렉터리 아래를 읽을 수 있도록 부모 경로를 허용한다.
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

    pub fn set_zoom(&self, factor: f64) {
        // SAFETY: main thread WKWebView property. self 는 main thread 객체(Retained<WKWebView>).
        unsafe {
            let _: () = objc2::msg_send![&self.webview, setPageZoom: factor];
        }
    }

    pub fn set_javascript_enabled(&self, enabled: bool) {
        // SAFETY: main thread. configuration().preferences() 는 main thread KVC 대상.
        #[allow(clippy::multiple_unsafe_ops_per_block)]
        unsafe {
            let config = self.webview.configuration();
            let prefs = config.preferences();
            let _: () = objc2::msg_send![&prefs, setJavaScriptEnabled: enabled];
        }
    }

    /// Follow는 상속, Light/Dark는 AppKit appearance를 지정한다.
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

    /// 비동기 규칙이 준비되면 http(s) 차단 여부를 반영한다. 미준비·컴파일 실패 동안의 차단을 보장하지 않는다.
    pub fn set_remote_content_allowed(&self, allowed: bool) {
        self.block_remote.set(!allowed);
        apply_block_state(
            &self.webview,
            self.block_remote.get(),
            self.content_rule_list.borrow().as_deref(),
        );
    }

    pub fn load_html(&self, html: &str) {
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

/// 기존 content rule을 모두 지우고 차단이 켜졌으며 규칙이 준비된 경우 다시 넣는다.
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

/// Command는 SUPER, Option은 ALT로 전달한다. 바인딩 토큰의 해석은 공통 matcher가 맡는다.
fn nsevent_mods_to_winit(
    flags: objc2_app_kit::NSEventModifierFlags,
) -> winit::keyboard::ModifiersState {
    use objc2_app_kit::NSEventModifierFlags;
    use winit::keyboard::ModifiersState;
    let mut mods = ModifiersState::empty();
    mods.set(
        ModifiersState::CONTROL,
        flags.contains(NSEventModifierFlags::Control),
    );
    mods.set(
        ModifiersState::SHIFT,
        flags.contains(NSEventModifierFlags::Shift),
    );
    mods.set(
        ModifiersState::ALT,
        flags.contains(NSEventModifierFlags::Option),
    );
    mods.set(
        ModifiersState::SUPER,
        flags.contains(NSEventModifierFlags::Command),
    );
    mods
}

/// AppKit의 private-use named key와 문자를 winit 키로 바꾼다. 빈 문자열은 None이다.
fn ns_chars_to_winit_key(chars: &str) -> Option<winit::keyboard::Key> {
    use winit::keyboard::{Key, NamedKey};
    let c = chars.chars().next()?;
    let named = match c as u32 {
        0xF700 => NamedKey::ArrowUp,
        0xF701 => NamedKey::ArrowDown,
        0xF702 => NamedKey::ArrowLeft,
        0xF703 => NamedKey::ArrowRight,
        0xF704 => NamedKey::F1,
        0xF705 => NamedKey::F2,
        0xF706 => NamedKey::F3,
        0xF707 => NamedKey::F4,
        0xF708 => NamedKey::F5,
        0xF709 => NamedKey::F6,
        0xF70A => NamedKey::F7,
        0xF70B => NamedKey::F8,
        0xF70C => NamedKey::F9,
        0xF70D => NamedKey::F10,
        0xF70E => NamedKey::F11,
        0xF70F => NamedKey::F12,
        0xF727 => NamedKey::Insert,
        0xF728 => NamedKey::Delete,
        0xF729 => NamedKey::Home,
        0xF72B => NamedKey::End,
        0xF72C => NamedKey::PageUp,
        0xF72D => NamedKey::PageDown,
        0x0D | 0x03 => NamedKey::Enter,
        0x09 => NamedKey::Tab,
        0x7F | 0x08 => NamedKey::Backspace,
        0x1B => NamedKey::Escape,
        0x20 => NamedKey::Space,
        _ => {
            if c.is_control() {
                return None;
            }
            return Some(Key::Character(c.to_string().into()));
        }
    };
    Some(Key::Named(named))
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::keyboard::{Key, NamedKey};

    #[test]
    fn ns_chars_map_to_character_and_named() {
        assert_eq!(ns_chars_to_winit_key("d"), Some(Key::Character("d".into())));
        assert_eq!(ns_chars_to_winit_key("="), Some(Key::Character("=".into())));
        assert_eq!(
            ns_chars_to_winit_key("\u{F708}"),
            Some(Key::Named(NamedKey::F5))
        );
        assert_eq!(
            ns_chars_to_winit_key("\u{1B}"),
            Some(Key::Named(NamedKey::Escape))
        );
        assert_eq!(ns_chars_to_winit_key(""), None);
    }
}
