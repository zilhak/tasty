//! Linux WebKitGTK wrapper (X11 only).
//! Reference: wry/src/webkitgtk/mod.rs (MIT license, Tauri)

// 이유: native 포인터·핸들 호출을 묶는 FFI 모듈이다. 파일 단위 면제는 새 코드에도 적용되므로 안전 래퍼로 옮기면 범위를 줄여야 한다.
#![allow(clippy::multiple_unsafe_ops_per_block)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::glib::Cast;
use gtk::prelude::*;
use webkit2gtk::{
    LoadEvent, NavigationPolicyDecision, NavigationPolicyDecisionExt, PolicyDecisionExt,
    PolicyDecisionType, ResponsePolicyDecision, ResponsePolicyDecisionExt, SettingsExt,
    URIRequestExt, UserContentManager, UserContentManagerExt, WebView, WebViewExt,
};
use winit::raw_window_handle::{
    HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle,
};

use super::keys::WebViewKeySink;
use super::script_gate::ScriptGate;
use super::{NavState, PendingNavigation, WebViewBounds};

/// input shape 를 정한 GTK 크기와 구멍(이 창 기준 물리 px `[x, y, 너비, 높이]`).
type InputShape = ((i32, i32), Vec<[i32; 4]>);

pub struct PlatformWebView {
    webview: WebView,
    gtk_window: gtk::Window,
    x11_window: std::os::raw::c_ulong,
    xlib: x11_dl::xlib::Xlib,
    x11_display: *mut std::os::raw::c_void,
    /// Xlib 핸들은 생성 스레드에서만 사용한다. 메서드 진입에서 이 ID를 확인한다.
    origin_thread: std::thread::ThreadId,
    block_remote: Rc<Cell<bool>>,
    nav_state: Rc<Cell<NavState>>,
    pending_navigations: Rc<RefCell<Vec<PendingNavigation>>>,
    /// html surface의 문서 단위 스크립트 허용. 없으면 JS는 설정값을 그대로 따른다.
    script_gate: Rc<RefCell<Option<ScriptGate>>>,
    parent_x11_window: std::os::raw::c_ulong,
    /// X11 foreign 창의 GDK 참조. Drop 중 GDK 연결의 오류 트랩에도 사용한다.
    gdk_window: gtk::gdk::Window,
    /// 없으면 content filter를 붙이지 못한다.
    ucm: Option<UserContentManager>,
    /// 비동기 컴파일 완료 전에는 필터가 없다.
    content_filter: Rc<RefCell<Option<ContentFilter>>>,
    /// 마지막 `set_bounds` 의 GTK 크기(GDK 논리 px). input shape 의 전체 사각형이다.
    gtk_size: Cell<(i32, i32)>,
    /// 마지막으로 적용한 input shape(GTK 크기와 구멍). 같으면 X 요청을 다시 보내지 않는다.
    applied_input_shape: RefCell<Option<InputShape>>,
}

/// GObject가 아닌 ref-counted filter의 소유권. 안전 바인딩이 없어 Drop에서 직접 unref한다.
struct ContentFilter(*mut webkit2gtk::ffi::WebKitUserContentFilter);

impl Drop for ContentFilter {
    fn drop(&mut self) {
        // SAFETY: 이 포인터는 `webkit_user_content_filter_store_save_finish` 가 준
        // full ref 이고 이 타입만이 소유한다. Drop 은 한 번만 돈다.
        unsafe { webkit2gtk::ffi::webkit_user_content_filter_unref(self.0) };
    }
}

fn set_js(wv: &WebView, enabled: bool) {
    if let Some(settings) = WebViewExt::settings(wv) {
        settings.set_enable_javascript(enabled);
    }
}

/// http(s) 리소스를 막는 WebKit content-blocker 규칙.
const REMOTE_BLOCK_RULES: &str =
    r#"[{"trigger":{"url-filter":"^https?://"},"action":{"type":"block"}}]"#;

const REMOTE_BLOCK_FILTER_ID: &str = "tasty-block-remote";

/// 비동기 콜백까지 Box로 보관하고 콜백이 소유권을 회수한다.
struct FilterSaveState {
    store: *mut webkit2gtk::ffi::WebKitUserContentFilterStore,
    ucm: UserContentManager,
    block_remote: Rc<Cell<bool>>,
    slot: Rc<RefCell<Option<ContentFilter>>>,
}

fn content_filter_store_dir() -> Option<String> {
    let Some(dir) = tasty_utils::path::tasty_home().map(|h| h.join("webkit-content-filters"))
    else {
        tracing::warn!("tasty 홈을 못 찾음 — 원격 서브리소스 차단이 안 걸린다");
        return None;
    };
    if let Err(e) = std::fs::create_dir_all(&dir) {
        tracing::warn!(
            ?dir,
            "content filter 저장소 디렉토리 생성 실패: {e} — 차단이 안 걸린다"
        );
        return None;
    }
    Some(dir.to_string_lossy().into_owned())
}

/// 디스크 필터 저장소에 비동기 컴파일한다. 완료 전이나 실패 시 서브리소스 차단은 적용되지 않는다.
fn compile_remote_block_filter(
    ucm: Option<UserContentManager>,
    block_remote: Rc<Cell<bool>>,
    slot: Rc<RefCell<Option<ContentFilter>>>,
) {
    use gtk::glib::translate::ToGlibPtr;

    let Some(ucm) = ucm else {
        tracing::warn!("WebKitGTK UserContentManager 없음 — 원격 서브리소스 차단이 안 걸린다");
        return;
    };
    let Some(dir_str) = content_filter_store_dir() else {
        return;
    };

    // SAFETY: 아래 세 포인터는 모두 이 블록이 소유한 값에서 나온다 —
    // `dir_str`/`REMOTE_BLOCK_FILTER_ID`/`rules` 의 stash 는 호출이 끝날 때까지 살아
    // 있고(변수로 묶어 둔다), 비동기 호출이 계속 필요로 하는 것(store)은 상태
    // 상자에 넣어 콜백이 되찾는다. 콜백은 GTK main loop 에서 돈다(= 이 스레드).
    unsafe {
        let store = webkit2gtk::ffi::webkit_user_content_filter_store_new(dir_str.to_glib_none().0);
        let state = Box::new(FilterSaveState {
            store,
            ucm,
            block_remote,
            slot,
        });
        let rules = gtk::glib::Bytes::from_static(REMOTE_BLOCK_RULES.as_bytes());
        let id = REMOTE_BLOCK_FILTER_ID.to_glib_none();
        let rules_ptr: *mut gtk::glib::ffi::GBytes = rules.to_glib_none().0;
        webkit2gtk::ffi::webkit_user_content_filter_store_save(
            store,
            id.0,
            rules_ptr,
            std::ptr::null_mut(),
            Some(remote_block_filter_saved),
            Box::into_raw(state) as gtk::glib::ffi::gpointer,
        );
    }
}

/// `webkit_user_content_filter_store_save` 완료 콜백.
///
/// # Safety
/// GIO 가 부르는 `GAsyncReadyCallback` 이다. `user_data` 는 위 함수가
/// `Box::into_raw` 로 넘긴 `FilterSaveState` 이고 이 호출이 유일한 소비자다.
unsafe extern "C" fn remote_block_filter_saved(
    _source: *mut gtk::glib::gobject_ffi::GObject,
    result: *mut gtk::gio::ffi::GAsyncResult,
    user_data: gtk::glib::ffi::gpointer,
) {
    use gtk::glib::translate::ToGlibPtr;

    // SAFETY: 위 주석의 계약. 포인터는 한 번만 되찾는다.
    let state = unsafe { Box::from_raw(user_data.cast::<FilterSaveState>()) };
    let mut err: *mut gtk::glib::ffi::GError = std::ptr::null_mut();
    // SAFETY: `state.store` 는 저장을 시작한 그 저장소이고, `result` 는 GIO 가 준
    // 이 호출의 결과다. 실패면 널을 주고 `err` 를 채운다.
    let filter = unsafe {
        webkit2gtk::ffi::webkit_user_content_filter_store_save_finish(state.store, result, &mut err)
    };
    // SAFETY: 저장소 참조는 `compile_remote_block_filter` 가 만든 것이고 여기가 끝이다
    // (비동기 작업은 자기 참조를 따로 잡는다).
    unsafe { gtk::glib::gobject_ffi::g_object_unref(state.store.cast()) };
    if filter.is_null() {
        // SAFETY: 실패 시 GIO 가 채워 준 GError — 메시지를 읽고 해제한다.
        let msg = unsafe {
            let m = if err.is_null() {
                String::from("(원인 미상)")
            } else {
                std::ffi::CStr::from_ptr((*err).message)
                    .to_string_lossy()
                    .into_owned()
            };
            if !err.is_null() {
                gtk::glib::ffi::g_error_free(err);
            }
            m
        };
        tracing::warn!("원격 차단 content filter 컴파일 실패: {msg} — 서브리소스 차단이 안 걸린다");
        return;
    }
    let filter = ContentFilter(filter);
    if state.block_remote.get() {
        // SAFETY: 방금 받은 유효한 필터를 이 webview 의 매니저에 붙인다. 매니저는
        // 상태 상자가 살아 있는 동안 유효하다.
        unsafe {
            webkit2gtk::ffi::webkit_user_content_manager_add_filter(
                state.ucm.to_glib_none().0,
                filter.0,
            )
        };
    }
    *state.slot.borrow_mut() = Some(filter);
}

/// 창·디스플레이 종류와 라이브러리 초기화는 Permanent, X 창 생성·조회 실패는 Transient로 분류한다.
fn perm(msg: impl std::fmt::Display) -> super::WebViewCreateError {
    super::WebViewCreateError::Permanent(msg.to_string())
}

fn transient(msg: impl std::fmt::Display) -> super::WebViewCreateError {
    super::WebViewCreateError::Transient(msg.to_string())
}

impl PlatformWebView {
    pub fn new(
        window: &(impl HasWindowHandle + HasDisplayHandle),
        bounds: WebViewBounds,
        scale_factor: f64,
        surface_id: u32,
        key_bridge: Rc<dyn WebViewKeySink>,
    ) -> Result<Self, super::WebViewCreateError> {
        let parent_xid = match window.window_handle().map_err(perm)?.as_raw() {
            RawWindowHandle::Xlib(w) => w.window,
            _ => return Err(perm("Not an X11 window (Wayland is not supported)")),
        };

        let x11_display_ptr = match window.display_handle().map_err(perm)?.as_raw() {
            RawDisplayHandle::Xlib(d) => d
                .display
                .map(|p| p.as_ptr())
                .unwrap_or(std::ptr::null_mut()),
            _ => std::ptr::null_mut(),
        };

        if !gtk::is_initialized() {
            gtk::init().map_err(|e| perm(format!("GTK init failed: {e}")))?;
        }

        let xlib =
            x11_dl::xlib::Xlib::open().map_err(|e| perm(format!("Failed to open Xlib: {e}")))?;

        let physical = bounds.to_physical(scale_factor);
        let x = physical.x as i32;
        let y = physical.y as i32;
        let w = physical.width as u32;
        let h = physical.height as u32;

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
            return Err(perm("Failed to get X11 display"));
        }

        // Create X11 child window
        // SAFETY: display는 위에서 null 체크 통과한 유효한 X11 Display*.
        // parent_xid는 winit이 만든 활성 X11 윈도우. 호출은 main thread.
        let x11_window = unsafe {
            (xlib.XCreateSimpleWindow)(display, parent_xid as _, x, y, w.max(1), h.max(1), 0, 0, 0)
        };

        if x11_window == 0 {
            return Err(transient("XCreateSimpleWindow failed"));
        }

        // SAFETY: 같은 생성 스레드에서 유효한 display와 방금 만든 창을 사용한다.
        // GDK는 다른 연결로 창을 조회하므로 XFlush만으로 부족하다. XSync로 서버의 생성 처리를 기다린다.
        unsafe {
            (xlib.XMapWindow)(display, x11_window);
            (xlib.XSync)(display, 0 /* discard = False */);
        }

        let gdk_display = gtk::gdk::Display::default().ok_or_else(|| perm("No GDK display"))?;

        let x11_gdk_display: gdkx11::X11Display = gdk_display
            .downcast()
            .map_err(|_| perm("GDK display is not X11"))?;

        let gdk_window =
            match crate::platform::x11_gdk_window::foreign_gdk_window(&x11_gdk_display, x11_window)
            {
                Ok(w) => w,
                Err(e) => {
                    // SAFETY: 방금 만든 창의 GDK 래핑이 실패했으므로 넘기지 않은 X 창을 여기서 지운다.
                    unsafe { (xlib.XDestroyWindow)(display, x11_window) };
                    // SAFETY: 위와 같은 유효한 display. 파괴 요청을 서버로 내보낸다 —
                    // 여기서는 왕복이 필요 없다(뒤에서 이 창을 조회하지 않는다).
                    unsafe { (xlib.XFlush)(display) };
                    return Err(transient(e));
                }
            };

        let gtk_window = gtk::Window::new(gtk::WindowType::Toplevel);
        let gdk_win_clone = gdk_window.clone();
        gtk_window.connect_realize(move |w| {
            // foreign 창 연결에 실패하면 별도 GTK 창에 그려질 수 있어 로그로 남긴다.
            if w.window().is_none() {
                tracing::warn!(
                    "WebView surface {surface_id}: GTK window realized without a GDK window; \
                     the page will render outside the parent window"
                );
                return;
            }
            w.set_window(gdk_win_clone.clone());
        });

        let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
        gtk_window.add(&vbox);

        let webview = WebView::new();
        vbox.pack_start(&webview, true, true, 0);

        // 탐색 정책만으로 서브리소스를 막을 수 없어 content filter도 별도로 설치한다.
        let block_remote = Rc::new(Cell::new(true));
        let pending_navigations = Rc::new(RefCell::new(Vec::new()));
        let script_gate: Rc<RefCell<Option<ScriptGate>>> = Rc::new(RefCell::new(None));
        {
            let block = block_remote.clone();
            let pending_nav = pending_navigations.clone();
            let gate = script_gate.clone();
            webview.connect_decide_policy(move |wv, decision, decision_type| {
                let uri = match decision_type {
                    PolicyDecisionType::Response => decision
                        .downcast_ref::<ResponsePolicyDecision>()
                        .and_then(|d| d.request())
                        .and_then(|r| r.uri()),
                    PolicyDecisionType::NavigationAction | PolicyDecisionType::NewWindowAction => {
                        decision
                            .downcast_ref::<NavigationPolicyDecision>()
                            .and_then(|d| d.navigation_action())
                            .and_then(|a| a.request())
                            .and_then(|r| r.uri())
                    }
                    _ => None,
                };
                // navigation action에는 frame 정보가 없어 main frame 응답에서만 문서 JS를 정한다(ADR-0053).
                if matches!(decision_type, PolicyDecisionType::Response)
                    && let Some(gate) = gate.borrow().as_ref()
                    && decision
                        .downcast_ref::<ResponsePolicyDecision>()
                        .is_some_and(|d| d.is_main_frame_main_resource())
                    && let Some(uri) = &uri
                {
                    set_js(wv, gate.main_response(uri.as_str()));
                }
                if let Some(uri) = &uri
                    && !matches!(decision_type, PolicyDecisionType::Response)
                {
                    tracing::debug!(
                        "WebView surface {surface_id}: navigation {decision_type:?} {}",
                        uri.as_str()
                    );
                }
                // 탐색 시도는 차단 여부와 별개로 기록한다. 응답 정책 이벤트는 제외한다.
                if matches!(
                    decision_type,
                    PolicyDecisionType::NavigationAction | PolicyDecisionType::NewWindowAction
                ) && let Some(uri) = &uri
                {
                    let user_gesture = decision
                        .downcast_ref::<NavigationPolicyDecision>()
                        .and_then(|d| d.navigation_action())
                        .is_some_and(|a| a.is_user_gesture());
                    pending_nav.borrow_mut().push(PendingNavigation {
                        url: uri.as_str().to_string(),
                        user_gesture,
                    });
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

        // 실패 후 Finished가 와도 Failed를 Done으로 덮지 않는다.
        let nav_state = Rc::new(Cell::new(NavState::Idle));
        {
            let nav = nav_state.clone();
            let gate = script_gate.clone();
            // load-changed는 main frame 로드에서만 온다(ADR-0053 측정). 화면 문서는 commit에서 기록한다.
            webview.connect_load_changed(move |wv, event| match event {
                LoadEvent::Started => {
                    tracing::debug!("WebView surface {surface_id}: load started");
                    nav.set(NavState::Loading);
                    if let Some(gate) = gate.borrow().as_ref() {
                        set_js(wv, gate.load_started());
                    }
                }
                LoadEvent::Committed => {
                    if let Some(gate) = gate.borrow().as_ref() {
                        let uri = wv.uri();
                        set_js(wv, gate.committed(uri.as_deref()));
                    }
                }
                LoadEvent::Finished => {
                    if nav.get() != NavState::Failed {
                        tracing::debug!("WebView surface {surface_id}: load finished");
                        nav.set(NavState::Done);
                    }
                    if let Some(js) = gate.borrow().as_ref().and_then(|g| g.finished()) {
                        set_js(wv, js);
                    }
                }
                _ => {} // Redirected 는 무시
            });
        }
        {
            // web process가 commit 전에 종료되면 load-failed도 load-changed Finished도 오지 않는다(실측).
            // 여기서 로드를 끝내지 않으면 스크립트 게이트가 로드 중에 머물러 허용을 거절한다.
            let nav = nav_state.clone();
            let gate = script_gate.clone();
            webview.connect_web_process_terminated(move |wv, reason| {
                tracing::warn!(
                    "WebView surface {surface_id}: WebKit web process terminated ({reason:?})"
                );
                nav.set(NavState::Failed);
                if let Some(gate) = gate.borrow().as_ref() {
                    gate.failed();
                }
                if let Some(js) = gate.borrow().as_ref().and_then(|g| g.finished()) {
                    set_js(wv, js);
                }
            });
        }
        {
            let nav = nav_state.clone();
            let gate = script_gate.clone();
            webview.connect_load_failed(move |_wv, _event, failing_uri, error| {
                tracing::warn!(
                    "WebView surface {surface_id}: WebKitGTK load-failed \
                     uri={failing_uri} err={error}"
                );
                nav.set(NavState::Failed);
                if let Some(gate) = gate.borrow().as_ref() {
                    gate.failed();
                }
                true // 기본 에러 페이지 억제(host error chrome 사용)
            });
        }

        // 키는 WebKit 기본 처리 전에 host 정책에 묻고 클릭은 페이지에도 전달한다.
        {
            let bridge = key_bridge.clone();
            webview.connect_key_press_event(move |_wv, ev| {
                // 이 press 신호에는 repeat 플래그가 없어 반복 입력도 host로 전달한다.
                if ev.is_modifier() {
                    return gtk::glib::Propagation::Proceed;
                }
                let Some(key) = gdk_keyval_to_winit_key(ev.keyval()) else {
                    return gtk::glib::Propagation::Proceed;
                };
                if bridge.capture_key(
                    surface_id,
                    key,
                    x11_keycode_to_physical(ev.hardware_keycode()),
                    gdk_state_to_winit_mods(ev.state()),
                ) {
                    gtk::glib::Propagation::Stop
                } else {
                    gtk::glib::Propagation::Proceed
                }
            });
        }
        {
            let bridge = key_bridge.clone();
            webview.connect_button_press_event(move |_wv, _ev| {
                bridge.note_focus(surface_id);
                gtk::glib::Propagation::Proceed
            });
        }

        // 필터는 비동기로 붙는다. 그전에 온 원격 요청까지 막는다고 보장하지 않는다.
        let ucm = WebViewExt::user_content_manager(&webview);
        let content_filter: Rc<RefCell<Option<ContentFilter>>> = Rc::new(RefCell::new(None));
        compile_remote_block_filter(ucm.clone(), block_remote.clone(), content_filter.clone());

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
            script_gate,
            parent_x11_window: parent_xid as _,
            gdk_window,
            ucm,
            content_filter,
            gtk_size: Cell::new((w.max(1) as i32, h.max(1) as i32)),
            applied_input_shape: RefCell::new(None),
        })
    }

    /// Xlib 핸들 접근은 생성 스레드에 한정한다. release 빌드에서도 확인한다.
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
        let physical = bounds.to_physical(scale_factor);
        let x = physical.x as i32;
        let y = physical.y as i32;
        let w = physical.width as i32;
        let h = physical.height as i32;

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

        // GTK 크기·allocation은 GDK 창 배율로 나눈 논리 px다. X 창과 같은 물리 크기를 덮도록
        // 올림한다. 물리 px를 그대로 주면 GDK 배율만큼 페이지 viewport가 X 창보다 커진다.
        let gdk_scale = self.gdk_window.scale_factor().max(1);
        let gtk_w = (w.max(1) + gdk_scale - 1) / gdk_scale;
        let gtk_h = (h.max(1) + gdk_scale - 1) / gdk_scale;
        self.gtk_size.set((gtk_w, gtk_h));
        self.gtk_window.resize(gtk_w, gtk_h);

        // foreign X 창의 크기 변경을 GTK가 자동 반영하지 못하므로 allocation도 직접 갱신한다.
        self.gtk_window
            .size_allocate(&gtk::Allocation::new(0, 0, gtk_w, gtk_h));
    }

    /// host 가 받아야 하는 입력 영역(분할선 hit 띠·창 리사이즈 밴드)을 이 창의 입력에서 뺀다.
    /// `holes` 는 이 창 기준 물리 px `[x, y, 너비, 높이]` 다. 빠진 곳의 포인터 이벤트는 X 서버가
    /// 부모 창(winit)으로 보낸다 — 입력 shape 밖의 자손 창으로는 내려가지 않는다. 화면에는 그대로
    /// 페이지가 보인다. GDK 단위로 바꿀 때 구멍을 바깥쪽으로 반올림해 host 쪽 띠가 줄지 않게 한다.
    pub fn set_input_holes(&self, holes: &[[i32; 4]]) {
        self.assert_origin_thread();
        let size = self.gtk_size.get();
        if self
            .applied_input_shape
            .borrow()
            .as_ref()
            .is_some_and(|(s, h)| *s == size && h.as_slice() == holes)
        {
            return;
        }
        let scale = self.gdk_window.scale_factor().max(1);
        let rect = |x, y, w, h| gtk::cairo::RectangleInt::new(x, y, w, h);
        let region = gtk::cairo::Region::create_rectangle(&rect(0, 0, size.0, size.1));
        for &[x, y, w, h] in holes {
            let x0 = x.div_euclid(scale);
            let y0 = y.div_euclid(scale);
            let x1 = (x + w + scale - 1).div_euclid(scale);
            let y1 = (y + h + scale - 1).div_euclid(scale);
            if let Err(e) = region.subtract_rectangle(&rect(x0, y0, x1 - x0, y1 - y0)) {
                tracing::warn!("WebView input shape: region subtract failed: {e}");
                return;
            }
        }
        self.gdk_window.input_shape_combine_region(&region, 0, 0);
        *self.applied_input_shape.borrow_mut() = Some((size, holes.to_vec()));
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

    /// 이 WebView 내부에 포커스가 있을 때만 부모 창으로 돌린다.
    /// 숨기기와 포커스 반환은 별개이며 다른 앱의 포커스를 가져오면 안 된다.
    pub fn release_keyboard_focus(&self) {
        self.assert_origin_thread();
        if !self.x11_focus_is_inside() {
            return;
        }
        // SAFETY: self 가 살아있으면 x11_display 는 valid 하고 parent_x11_window 는
        // 이 webview 를 만든 winit 창(부모)이다. 호출은 origin thread(위 assert).
        unsafe {
            (self.xlib.XSetInputFocus)(
                self.x11_display as _,
                self.parent_x11_window,
                x11_dl::xlib::RevertToParent,
                x11_dl::xlib::CurrentTime,
            );
        }
        // SAFETY: 위와 동일(valid display, origin thread). 요청을 즉시 밀어낸다.
        unsafe {
            (self.xlib.XFlush)(self.x11_display as _);
        }
    }

    /// X focus가 이 WebView 안에 있는지. X focus는 서버 전역이라 참이면 이 앱이 OS 포커스를 가진 것이다.
    /// 자식 창이 focus를 가지면 winit이 부모 창에 Focused(false)를 보내므로 호출부가 창 활성 판정에 함께 쓴다.
    pub fn holds_keyboard_focus(&self) -> bool {
        self.assert_origin_thread();
        self.x11_focus_is_inside()
    }

    fn x11_focus_is_inside(&self) -> bool {
        let mut focus: std::os::raw::c_ulong = 0;
        let mut revert: std::os::raw::c_int = 0;
        // SAFETY: display 는 valid(위 호출부가 origin thread 를 이미 확인). 두 out
        // 파라미터는 살아있는 스택 변수의 주소다.
        unsafe {
            (self.xlib.XGetInputFocus)(self.x11_display as _, &mut focus, &mut revert);
        }
        if focus <= 1 {
            return false;
        }
        let mut w = focus;
        // 손상된 부모 관계에서 무한 순회하지 않도록 깊이를 제한한다. 더 깊은 자손은 확인하지 못한다.
        for _ in 0..32 {
            if w == self.x11_window {
                return true;
            }
            let Some(parent) = self.x11_parent_of(w) else {
                return false;
            };
            if parent == 0 || parent == w {
                return false;
            }
            w = parent;
        }
        false
    }

    fn x11_parent_of(&self, window: std::os::raw::c_ulong) -> Option<std::os::raw::c_ulong> {
        let mut root: std::os::raw::c_ulong = 0;
        let mut parent: std::os::raw::c_ulong = 0;
        let mut children: *mut std::os::raw::c_ulong = std::ptr::null_mut();
        let mut nchildren: std::os::raw::c_uint = 0;
        // SAFETY: display 는 valid(origin thread 확인 완료), out 파라미터는 전부 살아있는
        // 스택 변수 주소다. children 은 Xlib 이 할당하며 바로 아래에서 해제한다.
        let ok = unsafe {
            (self.xlib.XQueryTree)(
                self.x11_display as _,
                window,
                &mut root,
                &mut parent,
                &mut children,
                &mut nchildren,
            )
        };
        if !children.is_null() {
            // SAFETY: 바로 위 XQueryTree 가 할당해 돌려준 배열이며 여기서만 해제한다.
            unsafe {
                (self.xlib.XFree)(children.cast());
            }
        }
        (ok != 0).then_some(parent)
    }

    pub fn nav_state(&self) -> NavState {
        self.nav_state.get()
    }

    pub fn take_pending_navigations(&self) -> Vec<PendingNavigation> {
        std::mem::take(&mut *self.pending_navigations.borrow_mut())
    }

    pub fn load_url(&self, url: &str) {
        self.nav_state.set(NavState::Loading);
        self.webview.load_uri(url);
    }

    pub fn load_html(&self, html: &str) {
        self.nav_state.set(NavState::Loading);
        self.webview.load_html(html, None);
    }

    pub fn set_zoom(&self, factor: f64) {
        self.webview.set_zoom_level(factor);
    }

    /// 게이트가 붙은 webview에서는 전역 sandbox 설정으로 받고, 문서 단위 판단을 적용한다.
    pub fn set_javascript_enabled(&self, enabled: bool) {
        let js = match self.script_gate.borrow().as_ref() {
            Some(gate) => gate.set_sandbox(!enabled),
            None => enabled,
        };
        set_js(&self.webview, js);
    }

    /// 문서 단위 스크립트 허용을 붙인다. 첫 로드 전에 호출한다.
    /// page cache를 끄면 뒤로·앞으로 가기도 응답 단계를 거쳐 모든 문서가 지문을 가진다(ADR-0053).
    pub fn attach_script_gate(&self, gate: ScriptGate) {
        if let Some(settings) = WebViewExt::settings(&self.webview) {
            settings.set_enable_page_cache(false);
        }
        set_js(&self.webview, gate.effective_js());
        *self.script_gate.borrow_mut() = Some(gate);
    }

    pub fn reload(&self) {
        self.webview.reload();
    }

    /// 사용자 탐색 조작을 debug IPC에서 재현한다.
    #[cfg(debug_assertions)]
    pub fn debug_history(&self, action: super::DebugHistoryAction) {
        match action {
            super::DebugHistoryAction::Back => self.webview.go_back(),
            super::DebugHistoryAction::Forward => self.webview.go_forward(),
            super::DebugHistoryAction::Reload => self.webview.reload(),
            super::DebugHistoryAction::Stop => self.webview.stop_loading(),
        }
    }

    /// 현재 Linux에서는 적용하지 않고 로그만 남긴다.
    pub fn set_color_scheme(&self, scheme: super::ColorScheme) {
        tracing::debug!("set_color_scheme({scheme:?}) is not implemented for Linux WebKitGTK");
    }

    /// 탐색 정책과 content filter에 차단 상태를 적용한다. 필터 준비 전·준비 실패 시 적용 범위가 다르다.
    pub fn set_remote_content_allowed(&self, allowed: bool) {
        self.block_remote.set(!allowed);
        self.apply_remote_block_filter();
        tracing::debug!("Linux WebKitGTK set_remote_content_allowed({allowed})");
    }

    /// 중복 설치를 피하려고 기존 ID의 필터를 제거한 뒤 필요하면 다시 붙인다.
    /// 컴파일 중이면 완료 콜백이 그때의 상태를 확인한다.
    fn apply_remote_block_filter(&self) {
        use gtk::glib::translate::ToGlibPtr;

        let Some(ucm) = &self.ucm else { return };
        ucm.remove_filter_by_id(REMOTE_BLOCK_FILTER_ID);
        if self.block_remote.get()
            && let Some(filter) = self.content_filter.borrow().as_ref()
        {
            // SAFETY: 컴파일이 끝난 유효한 필터이고 매니저는 self 가 소유한다.
            unsafe {
                webkit2gtk::ffi::webkit_user_content_manager_add_filter(
                    ucm.to_glib_none().0,
                    filter.0,
                )
            };
        }
    }
}

impl Drop for PlatformWebView {
    fn drop(&mut self) {
        self.assert_origin_thread();
        // GDK 연결의 지연 요청이 이미 지운 X 창을 참조할 수 있어 정리 중 오류 트랩으로 기록한다.
        let trap: Option<gdkx11::X11Display> = self.gdk_window.display().downcast().ok();
        if let Some(d) = &trap {
            d.error_trap_push();
        }
        // SAFETY: 생성 스레드 확인 뒤 이 객체가 소유한 WebView를 정리한다. Rc/raw pointer 필드로 Send를 구현하지 않는다.
        unsafe {
            self.webview.destroy();
        }
        // X 창을 지우기 전에 GTK의 숨기기·닫기 요청을 처리한다.
        self.gtk_window.hide();
        pump_gtk();
        // foreign GdkWindow는 GTK 위젯 소유 창이 아니므로 widget destroy 대신 close를 사용한다.
        self.gtk_window.close();
        pump_gtk();
        // GDK는 자기 연결로 요청을 내고 X 창은 아래에서 winit 연결로 지운다. 서버는 연결 사이의 처리 순서를
        // 보장하지 않으므로 GDK 연결을 sync해 trap 전에 낸 요청(drop 직전 set_visible(false)의 hide 등)까지 먼저 처리시킨다.
        self.gdk_window.display().sync();
        // SAFETY: 이 객체가 생성한 X 창을 생성 스레드에서 지운다. GDK의 후속 오류는 위 트랩으로 기록한다.
        unsafe {
            (self.xlib.XDestroyWindow)(self.x11_display as _, self.x11_window);
            (self.xlib.XSync)(self.x11_display as _, 0 /* discard = False */);
        }
        pump_gtk();
        if let Some(d) = &trap {
            let code = d.error_trap_pop();
            if code != 0 {
                tracing::warn!(
                    x11_window = self.x11_window,
                    error_code = code,
                    "webview 정리 중 X 에러 — 에러 트랩이 abort 를 막았다"
                );
            }
        }
    }
}

/// GTK의 대기 이벤트를 처리한다. 창을 먼저 지운 뒤 지연된 unmap 요청이 나가는 일을 줄인다.
/// 전역 GTK 큐를 처리하므로 이 WebView의 콜백만 실행한다고 볼 수는 없다.
fn pump_gtk() {
    while gtk::events_pending() {
        gtk::main_iteration_do(false);
    }
}

/// X11 keycode에서 8을 빼 winit이 사용하는 evdev scancode로 변환한다.
fn x11_keycode_to_physical(hardware_keycode: u16) -> winit::keyboard::PhysicalKey {
    use winit::platform::scancode::PhysicalKeyExtScancode;
    let Some(evdev) = (hardware_keycode as u32).checked_sub(8) else {
        return winit::keyboard::PhysicalKey::Unidentified(winit::keyboard::NativeKeyCode::Xkb(
            hardware_keycode as u32,
        ));
    };
    winit::keyboard::PhysicalKey::from_scancode(evdev)
}

/// GDK modifier를 winit 값으로 변환한다. Linux의 alt는 MOD1/ALT다.
fn gdk_state_to_winit_mods(state: gtk::gdk::ModifierType) -> winit::keyboard::ModifiersState {
    use gtk::gdk::ModifierType;
    use winit::keyboard::ModifiersState;
    let mut mods = ModifiersState::empty();
    mods.set(
        ModifiersState::CONTROL,
        state.contains(ModifierType::CONTROL_MASK),
    );
    mods.set(
        ModifiersState::SHIFT,
        state.contains(ModifierType::SHIFT_MASK),
    );
    mods.set(ModifiersState::ALT, state.contains(ModifierType::MOD1_MASK));
    mods.set(
        ModifiersState::SUPER,
        state.contains(ModifierType::SUPER_MASK),
    );
    mods
}

/// 알려진 named key와 Unicode 문자를 변환한다. 매핑할 수 없으면 페이지에 남긴다.
fn gdk_keyval_to_winit_key(keyval: gtk::gdk::keys::Key) -> Option<winit::keyboard::Key> {
    use gtk::gdk::keys::constants as k;
    use winit::keyboard::{Key, NamedKey};

    let named = match keyval {
        k::Tab | k::ISO_Left_Tab => NamedKey::Tab,
        k::Return | k::KP_Enter => NamedKey::Enter,
        k::BackSpace => NamedKey::Backspace,
        k::Delete | k::KP_Delete => NamedKey::Delete,
        k::Insert | k::KP_Insert => NamedKey::Insert,
        k::Home | k::KP_Home => NamedKey::Home,
        k::End | k::KP_End => NamedKey::End,
        k::Page_Up | k::KP_Page_Up => NamedKey::PageUp,
        k::Page_Down | k::KP_Page_Down => NamedKey::PageDown,
        k::Up | k::KP_Up => NamedKey::ArrowUp,
        k::Down | k::KP_Down => NamedKey::ArrowDown,
        k::Left | k::KP_Left => NamedKey::ArrowLeft,
        k::Right | k::KP_Right => NamedKey::ArrowRight,
        k::Escape => NamedKey::Escape,
        k::space | k::KP_Space => NamedKey::Space,
        k::F1 => NamedKey::F1,
        k::F2 => NamedKey::F2,
        k::F3 => NamedKey::F3,
        k::F4 => NamedKey::F4,
        k::F5 => NamedKey::F5,
        k::F6 => NamedKey::F6,
        k::F7 => NamedKey::F7,
        k::F8 => NamedKey::F8,
        k::F9 => NamedKey::F9,
        k::F10 => NamedKey::F10,
        k::F11 => NamedKey::F11,
        k::F12 => NamedKey::F12,
        _ => {
            let c = keyval.to_unicode()?;
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
    use winit::keyboard::{Key, ModifiersState, NamedKey};

    #[test]
    fn keyval_maps_to_character_and_named() {
        use gtk::gdk::keys::constants as k;
        assert_eq!(
            gdk_keyval_to_winit_key(k::d),
            Some(Key::Character("d".into()))
        );
        assert_eq!(
            gdk_keyval_to_winit_key(k::equal),
            Some(Key::Character("=".into()))
        );
        assert_eq!(
            gdk_keyval_to_winit_key(k::Escape),
            Some(Key::Named(NamedKey::Escape))
        );
        assert_eq!(gdk_keyval_to_winit_key(k::Control_L), None);
    }

    #[test]
    fn modifier_state_maps_to_winit() {
        use gtk::gdk::ModifierType;
        let mods = gdk_state_to_winit_mods(ModifierType::CONTROL_MASK | ModifierType::MOD1_MASK);
        assert!(mods.control_key());
        assert!(mods.alt_key());
        assert!(!mods.shift_key());
        assert_eq!(
            gdk_state_to_winit_mods(ModifierType::empty()),
            ModifiersState::empty()
        );
    }
}
