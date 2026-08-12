//! Linux WebKitGTK wrapper (X11 only).
//! Reference: wry/src/webkitgtk/mod.rs (MIT license, Tauri)
//!
//! Creates an X11 child window inside the parent, then hosts a GTK window
//! with a WebKitGTK WebView inside it.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::glib::Cast;
use gtk::prelude::*;
use webkit2gtk::{
    LoadEvent, NavigationPolicyDecision, NavigationPolicyDecisionExt, PolicyDecisionExt,
    PolicyDecisionType, ResponsePolicyDecision, ResponsePolicyDecisionExt, SettingsExt,
    URIRequestExt, WebView, WebViewExt,
};
use winit::raw_window_handle::{
    HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle,
};

use super::{NavState, WebViewBounds};

pub struct PlatformWebView {
    webview: WebView,
    gtk_window: gtk::Window,
    x11_window: std::os::raw::c_ulong,
    xlib: x11_dl::xlib::Xlib,
    x11_display: *mut std::os::raw::c_void,
    /// `new()`가 이 값을 만든 스레드의 `ThreadId`를 캡처해둔다. raw Xlib 핸들
    /// (x11_display/x11_window)에 실제로 접근하는 모든 메서드는 진입부에서
    /// `assert_origin_thread`로 이 값과 현재 스레드를 비교한다. 자연 `!Send`
    /// (아래 Drop 주석 참조)만으로는 "호출측이 실제로 옮기지 않았다"는 주장을
    /// 타입 시스템이 강제하지 못하므로, 이 필드가 그 불변식의 런타임 강제 지점이다.
    origin_thread: std::thread::ThreadId,
    /// 원격(http/https) 차단 여부(기본 true=차단). decide-policy 핸들러가 read.
    block_remote: Rc<Cell<bool>>,
    /// navigation 생명주기 상태(기본 Idle). load-changed/load-failed 시그널이 갱신,
    /// host sync_webviews 가 `nav_state()` 로 read. GTK 시그널은 GTK main loop
    /// (=winit main thread) 발화라 `Rc<Cell>` 로 충분(block_remote 동일).
    nav_state: Rc<Cell<NavState>>,
    /// decide-policy 가 캡처한, 아직 host 에 통지되지 않은 navigation 시도 URL 큐
    /// (도착 순서 보존). host `sync_webviews` 가 매 프레임 `take_pending_navigations`
    /// 로 비우고 plugin 에 `webview.navigation_attempt` 로 forward — "원격 http(s)
    /// 차단" 판정과 독립적으로 차단 여부와 무관하게 쌓인다.
    pending_navigations: Rc<RefCell<Vec<String>>>,
}

impl PlatformWebView {
    pub fn new(
        window: &(impl HasWindowHandle + HasDisplayHandle),
        bounds: WebViewBounds,
        scale_factor: f64,
    ) -> Result<Self, String> {
        let parent_xid = match window.window_handle().map_err(|e| e.to_string())?.as_raw() {
            RawWindowHandle::Xlib(w) => w.window,
            _ => return Err("Not an X11 window (Wayland is not supported)".to_string()),
        };

        let x11_display_ptr = match window.display_handle().map_err(|e| e.to_string())?.as_raw() {
            RawDisplayHandle::Xlib(d) => d
                .display
                .map(|p| p.as_ptr())
                .unwrap_or(std::ptr::null_mut()),
            _ => std::ptr::null_mut(),
        };

        // Initialize GTK if not already initialized
        if !gtk::is_initialized() {
            gtk::init().map_err(|e| format!("GTK init failed: {e}"))?;
        }

        let xlib = x11_dl::xlib::Xlib::open().map_err(|e| format!("Failed to open Xlib: {e}"))?;

        let x = (bounds.x * scale_factor) as i32;
        let y = (bounds.y * scale_factor) as i32;
        let w = (bounds.width * scale_factor) as u32;
        let h = (bounds.height * scale_factor) as u32;

        // Get X11 display
        let display = if x11_display_ptr.is_null() {
            // SAFETY: XOpenDisplay(null)는 DISPLAY env에서 기본 디스플레이를 연다.
            // 호출 실패 시 null 반환 — 아래 is_null 체크로 처리.
            // PlatformWebView::new는 winit event loop (main thread)에서만 호출되므로
            // Xlib 단일 thread 가정 충족.
            unsafe { (xlib.XOpenDisplay)(std::ptr::null()) }
        } else {
            x11_display_ptr as _
        };

        if display.is_null() {
            return Err("Failed to get X11 display".to_string());
        }

        // Create X11 child window
        // SAFETY: display는 위에서 null 체크 통과한 유효한 X11 Display*.
        // parent_xid는 winit이 만든 활성 X11 윈도우. 호출은 main thread.
        let x11_window = unsafe {
            (xlib.XCreateSimpleWindow)(display, parent_xid as _, x, y, w.max(1), h.max(1), 0, 0, 0)
        };

        if x11_window == 0 {
            return Err("XCreateSimpleWindow failed".to_string());
        }

        // SAFETY: 방금 만든 x11_window를 같은 display에 map → flush. 단일 thread, 같은 호출.
        unsafe {
            (xlib.XMapWindow)(display, x11_window);
            (xlib.XFlush)(display);
        }

        // Create GDK window from X11 window
        let gdk_display = gtk::gdk::Display::default().ok_or("No GDK display")?;

        let x11_gdk_display: gdkx11::X11Display = gdk_display
            .downcast()
            .map_err(|_| "GDK display is not X11")?;

        let gdk_window: gtk::gdk::Window =
            gdkx11::X11Window::foreign_new_for_display(&x11_gdk_display, x11_window).upcast();

        // Create GTK window and bind to the GDK window
        let gtk_window = gtk::Window::new(gtk::WindowType::Toplevel);
        let gdk_win_clone = gdk_window.clone();
        gtk_window.connect_realize(move |w| {
            w.window().map(|_| {
                w.set_window(gdk_win_clone.clone());
            });
        });

        let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
        gtk_window.add(&vbox);

        // Create WebView
        let webview = WebView::new();
        vbox.pack_start(&webview, true, true, 0);

        // 원격 콘텐츠 차단(기본 ON). webkit2gtk 2.0.2 바인딩은 UserContentFilterStore/
        // UserContentFilter 를 노출하지 않으므로(content-blocker 불가) decide-policy 로
        // navigation/response URI 를 검사해 원격 http/https 면 무시한다.
        // **한계**: decide-policy 는 최상위/프레임 네비게이션과 정책 협의 대상 응답에만
        // 발화하고, 페이지 내 서브리소스(img/css/js)는 잡지 못할 수 있다 — 완전한
        // 서브리소스 차단은 UserContentFilter 바인딩(상위 webkit2gtk)이 필요(후속).
        let block_remote = Rc::new(Cell::new(true));
        let pending_navigations = Rc::new(RefCell::new(Vec::new()));
        {
            let block = block_remote.clone();
            let pending_nav = pending_navigations.clone();
            webview.connect_decide_policy(move |_wv, decision, decision_type| {
                let uri = match decision_type {
                    PolicyDecisionType::Response => decision
                        .downcast_ref::<ResponsePolicyDecision>()
                        .and_then(|d| d.request())
                        .and_then(|r| r.uri()),
                    PolicyDecisionType::NavigationAction | PolicyDecisionType::NewWindowAction => {
                        decision
                            .downcast_ref::<NavigationPolicyDecision>()
                            .and_then(|d| d.request())
                            .and_then(|r| r.uri())
                    }
                    _ => None,
                };
                // navigation 시도(사용자 클릭·페이지 이동) 캡처 — Response(서브리소스 정책)
                // 는 제외, NavigationAction/NewWindowAction 만. 아래 차단 판정과 무관하게
                // 항상 기록한다("원격 http(s) 차단"과 통지는 독립).
                if matches!(
                    decision_type,
                    PolicyDecisionType::NavigationAction | PolicyDecisionType::NewWindowAction
                ) && let Some(uri) = &uri
                {
                    pending_nav.borrow_mut().push(uri.as_str().to_string());
                }
                if !block.get() {
                    return false;
                }
                if let Some(uri) = &uri {
                    let s = uri.as_str();
                    if s.starts_with("http://") || s.starts_with("https://") {
                        decision.ignore();
                        return true;
                    }
                }
                false
            });
        }

        // navigation 생명주기 시그널. load-changed(Started→Loading / Finished→Done) +
        // load-failed(→Failed). webkit2gtk 는 실패 시 load-failed 다음 load-changed(Finished)
        // 를 쏠 수 있어, Finished 에서 `!= Failed` 가드로 Failed 를 Done 으로 되돌리지 않는다.
        let nav_state = Rc::new(Cell::new(NavState::Idle));
        {
            let nav = nav_state.clone();
            webview.connect_load_changed(move |_wv, event| match event {
                LoadEvent::Started => nav.set(NavState::Loading),
                LoadEvent::Finished => {
                    if nav.get() != NavState::Failed {
                        nav.set(NavState::Done);
                    }
                }
                _ => {} // Redirected / Committed 은 무시
            });
        }
        {
            let nav = nav_state.clone();
            webview.connect_load_failed(move |_wv, _event, failing_uri, error| {
                // 사유는 로그 전용 — 화면 error chrome 은 URL 만 보여준다.
                tracing::warn!("WebKitGTK load-failed uri={failing_uri} err={error}");
                nav.set(NavState::Failed);
                true // 기본 에러 페이지 억제(host error chrome 사용)
            });
        }

        gtk_window.show_all();

        Ok(Self {
            webview,
            gtk_window,
            x11_window,
            xlib,
            x11_display: display as _,
            origin_thread: std::thread::current().id(),
            block_remote,
            nav_state,
            pending_navigations,
        })
    }

    /// raw Xlib 핸들(x11_display/x11_window)에 접근하는 모든 메서드가 진입부에서
    /// 호출한다. `new()`가 캡처한 생성 스레드와 다르면 즉시 panic한다 —
    /// `XInitThreads` 없이 다른 스레드에서 이 핸들을 건드리면 UB이므로, debug에서만
    /// 잡으면 release 에서 조용히 UB가 난다. 따라서 `debug_assert!`가 아니라
    /// `assert!`로 release 빌드에서도 유지한다.
    fn assert_origin_thread(&self) {
        assert_eq!(
            std::thread::current().id(),
            self.origin_thread,
            "PlatformWebView(X11) accessed from a thread different from its creation \
             thread; raw Xlib Display*/Window handles require single-thread confinement \
             (no XInitThreads)"
        );
    }

    pub fn set_bounds(&self, bounds: WebViewBounds, scale_factor: f64) {
        self.assert_origin_thread();
        let x = (bounds.x * scale_factor) as i32;
        let y = (bounds.y * scale_factor) as i32;
        let w = (bounds.width * scale_factor) as i32;
        let h = (bounds.height * scale_factor) as i32;

        // SAFETY: self가 살아있으면 x11_display/x11_window 모두 valid (Drop이 정리).
        // 호출은 main thread (winit event loop) 흐름에서만 일어남 — 위
        // assert_origin_thread 가 이를 런타임으로 강제한다.
        unsafe {
            (self.xlib.XMoveResizeWindow)(
                self.x11_display as _,
                self.x11_window,
                x,
                y,
                w.max(1) as u32,
                h.max(1) as u32,
            );
            (self.xlib.XFlush)(self.x11_display as _);
        }

        self.gtk_window.resize(w.max(1), h.max(1));
    }

    pub fn set_visible(&self, visible: bool) {
        self.assert_origin_thread();
        if visible {
            // SAFETY: self valid; main thread(위 assert_origin_thread 로 런타임 강제).
            unsafe {
                (self.xlib.XMapWindow)(self.x11_display as _, self.x11_window);
                (self.xlib.XFlush)(self.x11_display as _);
            }
            self.gtk_window.show_all();
        } else {
            // SAFETY: self valid; main thread(위 assert_origin_thread 로 런타임 강제).
            unsafe {
                (self.xlib.XUnmapWindow)(self.x11_display as _, self.x11_window);
                (self.xlib.XFlush)(self.x11_display as _);
            }
            self.gtk_window.hide();
        }
    }

    /// 현재 navigation 생명주기 상태(load-changed/load-failed 시그널이 갱신).
    pub fn nav_state(&self) -> NavState {
        self.nav_state.get()
    }

    /// decide-policy 가 캡처한 navigation 시도 URL 을 도착 순서대로 비워서 반환한다.
    /// host `sync_webviews` 가 매 프레임 호출해 plugin 에 forward.
    pub fn take_pending_navigations(&self) -> Vec<String> {
        std::mem::take(&mut *self.pending_navigations.borrow_mut())
    }

    pub fn load_url(&self, url: &str) {
        // 콜백이 늦게 와도 즉시 spinner 가 뜨도록 Loading 선반영.
        self.nav_state.set(NavState::Loading);
        self.webview.load_uri(url);
    }

    pub fn load_html(&self, html: &str) {
        self.nav_state.set(NavState::Loading);
        self.webview.load_html(html, None);
    }

    /// Content zoom (1.0 = 100%). WebKitGTK `WebView::zoom_level`.
    pub fn set_zoom(&self, factor: f64) {
        self.webview.set_zoom_level(factor);
    }

    /// JavaScript 실행 허용 여부. WebKitGTK `WebKitSettings::enable_javascript` — 다음
    /// 네비게이션부터 적용. host 는 "Sandbox scripts" on(기본) → `enabled=false`.
    pub fn set_javascript_enabled(&self, enabled: bool) {
        if let Some(settings) = WebViewExt::settings(&self.webview) {
            settings.set_enable_javascript(enabled);
        }
    }

    /// `prefers-color-scheme` 강제. WebKitGTK 는 깔끔한 단일 toggle 이 없어 현재 no-op —
    /// 후속. `scheme` 만 로깅.
    pub fn set_color_scheme(&self, scheme: super::ColorScheme) {
        tracing::debug!("set_color_scheme({scheme:?}) — Linux WebKitGTK no-op (후속)");
    }

    /// 원격(http/https) 콘텐츠 허용 여부. `new()` 의 decide-policy 핸들러가 이 플래그를
    /// read 해 `false`면 원격 URI navigation/response 를 무시한다(서브리소스 한계는 new 주석
    /// 참조). 여기서는 플래그만 갱신.
    pub fn set_remote_content_allowed(&self, allowed: bool) {
        self.block_remote.set(!allowed);
        tracing::debug!("Linux WebKitGTK set_remote_content_allowed({allowed})");
    }
}

impl Drop for PlatformWebView {
    fn drop(&mut self) {
        self.assert_origin_thread();
        // SAFETY: Drop은 self가 마지막으로 살아있는 시점. webview.destroy()와
        // XDestroyWindow는 같은 display 인스턴스에서 한 번씩 호출. 호출은
        // PlatformWebView가 생성된 main thread에서만 일어난다.
        //
        // 이 불변식은 두 겹으로 강제된다: (1) 본 타입은 x11_display(raw pointer)와
        // Rc<Cell<_>> 필드로 인해 auto-trait 상 자연 `!Send`이며(macOS/Windows
        // 백엔드와 동일 패턴) 의도적으로 Send를 부여하지 않아 안전한 Rust 코드로는
        // 애초에 다른 스레드로 옮길 수 없다. (2) 그럼에도 unsafe 코드나 향후 회귀로
        // 이 불변식이 깨질 경우를 대비해, 위 `assert_origin_thread` 가 생성 스레드와
        // 현재 스레드를 런타임으로 비교해 release 빌드에서도 즉시 panic 시킨다
        // (X11 핸들 오용은 UB라 debug에서만 잡으면 release 에서 조용히 UB가 난다).
        unsafe {
            self.webview.destroy();
            (self.xlib.XDestroyWindow)(self.x11_display as _, self.x11_window);
        }
        self.gtk_window.close();
    }
}
