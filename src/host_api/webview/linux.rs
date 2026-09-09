//! Linux WebKitGTK wrapper (X11 only).
//! Reference: wry/src/webkitgtk/mod.rs (MIT license, Tauri)
//!
//! Creates an X11 child window inside the parent, then hosts a GTK window
//! with a WebKitGTK WebView inside it.

// 이유: 이 파일 전체가 FFI 경계라 unsafe op 이 한 블록에 묶이는 것이 구조다 —
//       raw 포인터·핸들을 넘기는 호출은 그 사이에 안전한 문장을 끼울 자리가 없다.
//       그래서 자리마다 같은 사유를 반복하는 대신 파일 단위로 면제한다.
//       ★ 이 파일이 FFI 묶음이 아니게 되면(래퍼가 안전한 타입을 노출하게 되면)
//         이 줄을 지워라 — 파일 단위 면제는 그 안의 새 위반도 함께 가린다.
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

use super::keys::WebViewKeyBridge;
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
    /// 부모 winit X11 창. `release_keyboard_focus` 가 키보드 포커스를 여기로
    /// 되돌린다(overlay 가 열려 webview 를 숨길 때).
    parent_x11_window: std::os::raw::c_ulong,
    /// `x11_window` 을 감싼 foreign GDK 창. `Drop` 이 이 창의 GDK 디스플레이를 꺼내
    /// 에러 트랩을 걸 때만 쓴다(그 이유는 `Drop` 주석).
    ///
    /// **`destroy_notify()` 를 부르지 마라.** "네이티브 창을 남이 파괴했다" 라는 뜻이라
    /// 여기에 맞아 보이지만, 실측(2026-09-08, 격리 홈)에서는 그 호출이 오히려 죽는 쪽을
    /// 늘렸다 — webview 탭 둘인 창 닫기가 4 회 중 1 회 사망에서 6 회 중 5 회 사망이 됐다.
    gdk_window: gtk::gdk::Window,
    /// 이 webview 의 user content manager. 원격 서브리소스 차단 필터를 여기에
    /// 붙였다 뗀다. WebKitGTK 가 안 주면 `None` — 그때는 서브리소스 차단이 없다.
    ucm: Option<UserContentManager>,
    /// 컴파일이 끝난 원격 차단 필터. 저장이 비동기라 `new()` 직후에는 비어 있고,
    /// 완료 콜백이 채운다(macOS 의 `content_rule_list` 와 같은 형태).
    content_filter: Rc<RefCell<Option<ContentFilter>>>,
}

/// 컴파일된 content filter 의 소유권. `WebKitUserContentFilter` 는 GObject 가 아니라
/// ref-count 되는 boxed 타입이고 webkit2gtk 2.0.2 의 안전한 바인딩이 이 타입을
/// 통째로 건너뛰었다(`user_content_manager.rs` 의 `add_filter` 는 주석 처리돼 있다).
/// 그래서 소유권을 여기서 직접 진다 — Drop 이 unref 한다.
struct ContentFilter(*mut webkit2gtk::ffi::WebKitUserContentFilter);

impl Drop for ContentFilter {
    fn drop(&mut self) {
        // SAFETY: 이 포인터는 `webkit_user_content_filter_store_save_finish` 가 준
        // full ref 이고 이 타입만이 소유한다. Drop 은 한 번만 돈다.
        unsafe { webkit2gtk::ffi::webkit_user_content_filter_unref(self.0) };
    }
}

/// 원격 서브리소스를 막는 content-blocker 규칙. 이 JSON 스키마는 macOS 백엔드가
/// `WKContentRuleList` 에 넣는 것과 **같다** — 두 플랫폼이 같은 문장을 쓴다.
/// 근거·대안(왜 `send-request` 도 프록시도 아닌지)·재검토 조건은
/// `docs/adr/0250-linux-blocks-remote-subresources-with-a-webkit-content-filter.md`.
const REMOTE_BLOCK_RULES: &str =
    r#"[{"trigger":{"url-filter":"^https?://"},"action":{"type":"block"}}]"#;

/// 저장소 안에서 위 규칙을 부르는 이름. `remove_filter_by_id` 가 이 값을 쓴다.
const REMOTE_BLOCK_FILTER_ID: &str = "tasty-block-remote";

/// 비동기 저장 콜백까지 살아 있어야 하는 것들. `Box::into_raw` 로 넘기고 콜백이
/// `Box::from_raw` 로 되찾아 떨군다.
struct FilterSaveState {
    store: *mut webkit2gtk::ffi::WebKitUserContentFilterStore,
    ucm: UserContentManager,
    block_remote: Rc<Cell<bool>>,
    slot: Rc<RefCell<Option<ContentFilter>>>,
}

/// content filter 저장소 디렉토리를 만들고 그 경로를 낸다. 못 만들면 사유를 남기고
/// `None` — 부르는 쪽은 그때 차단 없이 진행한다.
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

/// 원격 차단 필터를 컴파일해 저장하고, 끝나면 차단이 켜져 있을 때 붙인다.
///
/// WebKit 은 content filter 를 **디스크 저장소에 컴파일해 두고** 쓴다 — 그래서 이
/// 경로가 비동기다. 저장소는 tasty 홈 아래에 둔다(같은 규칙을 매번 다시 컴파일하지
/// 않도록 WebKit 이 알아서 재사용한다).
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

/// 이 백엔드의 실패를 두 종류로 나누는 자리. **분류 근거를 한곳에 모아 둔다** —
/// 분기마다 흩어 두면 새 분기를 더할 때 무엇을 기준으로 골랐는지가 사라진다.
///
/// 기준: **다음 시도에 달라질 수 있는 입력이 있는가.**
/// - 창 종류(X11/Wayland) · GTK 초기화 · Xlib 적재 · 디스플레이 열기·종류 —
///   프로세스가 사는 동안 안 바뀐다 ⇒ `Permanent`.
/// - `XCreateSimpleWindow` 실패(서버 자원 고갈) · GDK 조회가 창을 못 찾음 —
///   서버 상태라 달라질 수 있다 ⇒ `Transient`.
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
        key_bridge: Rc<WebViewKeyBridge>,
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

        // Initialize GTK if not already initialized
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

        // SAFETY: 방금 만든 x11_window를 같은 display에 map → sync. 단일 thread, 같은 호출.
        //
        // 근거·재검토 조건: docs/adr/0159-a-null-gdk-window-is-a-value-not-a-crash.md
        // `XFlush` 가 아니라 `XSync` 인 것이 핵심이다 — 아래에서 이 창을 조회하는
        // 것은 **GDK 자기 연결**이고, 창을 만든 것은 winit 의 연결이다. `XFlush` 는
        // 소켓에 쓰기만 하고 서버가 처리했는지는 안 기다리므로, 두 연결 사이에
        // 순서 보장이 없어 GDK 쪽 조회가 생성보다 먼저 처리될 수 있다. 그러면
        // 서버는 "그런 창 없다" 로 답한다. `XSync` 는 왕복이라 반환 시점에 생성이
        // **처리 완료**돼 있고, 그 뒤에는 어느 연결이 물어도 창이 보인다.
        unsafe {
            (xlib.XMapWindow)(display, x11_window);
            (xlib.XSync)(display, 0 /* discard = False */);
        }

        // Create GDK window from X11 window
        let gdk_display = gtk::gdk::Display::default().ok_or_else(|| perm("No GDK display"))?;

        let x11_gdk_display: gdkx11::X11Display = gdk_display
            .downcast()
            .map_err(|_| perm("GDK display is not X11"))?;

        // 창이 없으면 NULL 이 온다. 바인딩은 그것을 패닉으로 바꾸므로 쓰지 않는다.
        let gdk_window =
            match crate::platform::x11_gdk_window::foreign_gdk_window(&x11_gdk_display, x11_window)
            {
                Ok(w) => w,
                Err(e) => {
                    // 여기서 그냥 돌아가면 방금 만든 X 창이 주인 없이 남는다. 호출부
                    // (`create_missing_webviews`)는 webview 가 없는 surface 를 **매 프레임**
                    // 다시 시도하므로, 정리하지 않으면 실패가 이어지는 동안 창이 쌓인다.
                    // SAFETY: 이 함수가 방금 같은 display 에 만든 창이고, 아직 누구에게도
                    // 넘기지 않았다(GDK 래핑이 실패한 자리다). 단일 thread.
                    unsafe { (xlib.XDestroyWindow)(display, x11_window) };
                    // SAFETY: 위와 같은 유효한 display. 파괴 요청을 서버로 내보낸다 —
                    // 여기서는 왕복이 필요 없다(뒤에서 이 창을 조회하지 않는다).
                    unsafe { (xlib.XFlush)(display) };
                    return Err(transient(e));
                }
            };

        // Create GTK window and bind to the GDK window
        let gtk_window = gtk::Window::new(gtk::WindowType::Toplevel);
        let gdk_win_clone = gdk_window.clone();
        gtk_window.connect_realize(move |w| {
            // realize 됐는데 GDK 창이 없으면 부모 안의 foreign 창으로 바꿔칠 자리가
            // 없다. 그냥 건너뛰면 WebKit 은 GTK 가 스스로 만든 **별개 toplevel** 에
            // 그리고, 부모 안에 만들어 map 해둔 X 자식 창은 배경 픽셀(검정)만 남긴다.
            // navigation 은 그래도 정상 완료하므로 다른 어떤 진단 줄도 남지 않는다 —
            // 화면만 비어 보이는 상태의 유일한 흔적이 이 줄이다.
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

        // Create WebView
        let webview = WebView::new();
        vbox.pack_start(&webview, true, true, 0);

        // 원격 콘텐츠 차단(기본 ON). 두 자리에서 막는다 — decide-policy 가 네비게이션을,
        // content filter 가 페이지 안의 서브리소스를 막는다. 두 자리가 필요한 이유는
        // decide-policy 가 최상위/프레임 네비게이션과 정책 협의 대상 응답에만 발화하기
        // 때문이다(실측 2026-09-08: `allow_remote_content=false` 인데 문서 안의 원격
        // `<img>` 가 그대로 떴다 — macOS 는 `WKContentRuleList`, Windows 는
        // `WebResourceRequested` 로 이미 서브리소스까지 막고 있어 결론이 갈렸다).
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
                LoadEvent::Started => {
                    tracing::debug!("WebView surface {surface_id}: load started");
                    nav.set(NavState::Loading);
                }
                LoadEvent::Finished => {
                    if nav.get() != NavState::Failed {
                        tracing::debug!("WebView surface {surface_id}: load finished");
                        nav.set(NavState::Done);
                    }
                }
                _ => {} // Redirected / Committed 은 무시
            });
        }
        {
            // web process 가 죽으면 load-failed 도 load-changed 도 오지 않는다 — nav 는
            // Loading 에 굳고 reveal 게이트가 영영 안 열린다. 그 사실이 남는 유일한 줄이다.
            let nav = nav_state.clone();
            webview.connect_web_process_terminated(move |_wv, reason| {
                tracing::warn!(
                    "WebView surface {surface_id}: WebKit web process terminated ({reason:?})"
                );
                nav.set(NavState::Failed);
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

        // 키 포워딩 + 모델 포커스 동기화. 두 시그널 모두 WebView 위젯에 `after` 없이
        // 연결하므로 WebKitGTK 의 클래스 핸들러보다 **먼저** 실행된다 — 키는 여기서
        // 소비 여부가 그 자리에서 정해지고(`Propagation::Stop` 이면 페이지가 못 본다),
        // 클릭은 항상 `Proceed` 로 흘려 페이지 동작을 건드리지 않는다.
        {
            let bridge = key_bridge.clone();
            webview.connect_key_press_event(move |_wv, ev| {
                // press 만 온다(release 는 별도 시그널). GDK 는 auto-repeat 도 같은
                // 시그널로 보내지만 press 이벤트에 repeat 플래그가 없다 — host 단축키는
                // 모두 edge 동작이라 반복 발화해도 사용자가 키를 누르고 있는 동안의
                // 의도와 일치한다(터미널 winit 경로도 repeat 를 걸러내지 않는다).
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
                // 클릭은 winit 에 도달하지 않으므로(`try_click_to_activate` 미실행)
                // host 모델 포커스를 여기서 대신 알려준다. 페이지 동작은 그대로.
                bridge.note_focus(surface_id);
                gtk::glib::Propagation::Proceed
            });
        }

        // 서브리소스 차단 필터는 디스크 컴파일이라 비동기다 — 지금 시작하고 완료
        // 콜백이 붙인다. 그 전에 도착하는 원격 요청은 못 막지만, 문서 로드 자체가
        // 이 뒤라 실사용에서 열리는 창은 없다(있으면 위 warn 이 남는다).
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
            parent_x11_window: parent_xid as _,
            gdk_window,
            ucm,
            content_filter,
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

    /// 키보드 포커스를 부모 winit 창으로 되돌린다. host 가 egui overlay 를 열어
    /// webview 를 숨길 때 호출한다 — 숨기는 것(`XUnmapWindow`)과 키보드 포커스를
    /// 놓는 것은 X11 에서 별개라, 회수하지 않으면 방금 연 popup 이 키를 못 받는다.
    ///
    /// **X 입력 포커스가 실제로 이 webview 창 안에 있을 때만** 회수한다. 무조건
    /// `XSetInputFocus` 를 부르면 IPC 로 popup 을 여는 것만으로 다른 앱이 쥐고 있던
    /// OS 키보드 포커스를 tasty 가 뺏는다 — 에이전트 행동이 사용자 포커스에 닿는
    /// 것이라 불가침 원칙 1 위반이다(`docs/identity.md`). 창 자체가 활성인지는
    /// 호출부(`sync_webviews`)가 `base.focused` 로 한 번 더 건다.
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

    /// 현재 X 입력 포커스가 이 webview 창 자신이거나 그 하위 창인지.
    fn x11_focus_is_inside(&self) -> bool {
        let mut focus: std::os::raw::c_ulong = 0;
        let mut revert: std::os::raw::c_int = 0;
        // SAFETY: display 는 valid(위 호출부가 origin thread 를 이미 확인). 두 out
        // 파라미터는 살아있는 스택 변수의 주소다.
        unsafe {
            (self.xlib.XGetInputFocus)(self.x11_display as _, &mut focus, &mut revert);
        }
        // None(0)/PointerRoot(1) 은 특정 창이 아니다 — 회수할 대상이 없다.
        if focus <= 1 {
            return false;
        }
        let mut w = focus;
        // 부모 체인을 거슬러 올라가며 이 창을 만나는지 본다. WebKit 이 만드는 내부
        // 창까지 쳐도 깊이는 얕아 상한 32 로 충분하고, 상한이 있어야 서버 상태가
        // 깨져도 무한 루프가 되지 않는다.
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

    /// `XQueryTree` 로 창의 부모 xid 를 얻는다(실패 시 `None`).
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

    /// 원격(http/https) 콘텐츠 허용 여부. `new()` 가 건 두 핸들러가 이 플래그를 read 한다 —
    /// decide-policy 는 원격 URI navigation/response 를 무시하고, `send-request` 는 원격
    /// 서브리소스 요청을 취소한다. 여기서는 플래그만 갱신(다음 요청부터 반영).
    pub fn set_remote_content_allowed(&self, allowed: bool) {
        self.block_remote.set(!allowed);
        self.apply_remote_block_filter();
        tracing::debug!("Linux WebKitGTK set_remote_content_allowed({allowed})");
    }

    /// 차단 상태를 user content manager 에 idempotent 하게 반영한다. 항상 먼저 지운
    /// 뒤 차단이면 다시 붙인다(중복 add 방지) — macOS 의 `apply_block_state` 와 같은
    /// 형태다. 필터가 아직 컴파일 중이면(`None`) 붙일 것이 없고, 완료 콜백이 그때의
    /// 플래그를 다시 읽어 적용한다.
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
        // 아래 순서를 지켜도 GDK 가 자기 연결에서 내는 요청까지 우리가 다 통제하지는
        // 못한다 — 소유자가 둘인 창이라 남는 경합이 있다. 그래서 정리하는 동안만
        // GDK 의 에러 트랩을 건다. 트랩은 abort 를 **값으로 바꾼다**: 트랩이 걸린
        // 동안 이 디스플레이에서 난 X 에러는 전역 핸들러(=abort)로 안 가고
        // `error_trap_pop` 의 반환값이 된다. 삼키지 않고 그 코드를 로그로 남긴다.
        //
        // **순서 수정 뒤의 마지막 그물이지, 순서 대신이 아니다.** 이것만 걸고 순서를
        // 그대로 두면 근본 경합이 남는다.
        let trap: Option<gdkx11::X11Display> = self.gdk_window.display().downcast().ok();
        if let Some(d) = &trap {
            d.error_trap_push();
        }
        // SAFETY: Drop은 self가 마지막으로 살아있는 시점. webview.destroy()와
        // (아래의) XDestroyWindow는 같은 display 인스턴스에서 한 번씩 호출. 호출은
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
        }
        // ── 여기부터 순서가 곧 내용이다 (아래 함수 주석 참조) ──
        // 1. GDK 가 이 창을 unmap 하는 것을 **창이 아직 있을 때** 시키고 그 요청이
        //    실제로 나갈 때까지 GTK 를 돌린다. 죽은 창에 나가던 `UnmapWindow` 가
        //    바로 이것이었다.
        self.gtk_window.hide();
        pump_gtk();
        // 2. toplevel 위젯을 놓는다. `destroy()` 는 쓸 수 없다 — 이 toplevel 의
        //    GdkWindow 는 우리가 `set_window` 으로 끼워 넣은 foreign 창이라
        //    `gtk_widget_unregister_window` 의 `user_data == widget` 단정이 깨지고
        //    GTK 가 `Bail out!` 으로 프로세스를 죽인다(실측 2026-09-08).
        self.gtk_window.close();
        pump_gtk();
        // 3. 이제야 X 창을 지운다. foreign 창은 GDK 가 파괴하지 않으므로 이 호출이
        //    필요하고, `XSync` + 마지막 펌프로 GDK 가 `DestroyNotify` 를 받아 자기
        //    상태를 맞추게 한다. **그것만으로 한 창에 webview 가 둘인 경우가 다 닫히지는
        //    않았다** — 뒤에 오는 `Drop` 이 앞 창의 낡은 상태를 건드려 4 회 중 1 회 죽었고,
        //    그 남은 자리를 위의 에러 트랩이 받는다(실측 2026-09-08).
        //
        // SAFETY: 위 블록과 같은 근거. GDK 가 이 창을 다 놓은 뒤라 이것이 마지막 파괴다.
        unsafe {
            (self.xlib.XDestroyWindow)(self.x11_display as _, self.x11_window);
            (self.xlib.XSync)(self.x11_display as _, 0 /* discard = False */);
        }
        pump_gtk();
        if let Some(d) = &trap {
            // `error_trap_pop` 은 서버와 왕복해 이 시점까지의 에러를 확정한 뒤 코드를
            // 돌려준다. 0 이 아니면 위 순서가 못 막은 자리가 남아 있다는 뜻이다.
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

/// 대기 중인 GTK 이벤트를 지금 처리한다. 이 레포가 GTK 를 winit 루프 안에서 돌릴 때
/// 쓰는 형태 그대로다(`platform::native_menu::linux` · `platform::system_tray`).
///
/// `PlatformWebView::drop` 이 이것을 쓰는 이유는 GDK 가 창을 **바로** 놓지 않기
/// 때문이다. `hide()`·`close()` 는 요청을 걸어 두고 돌아오고, 실제 X 요청은 다음
/// 메인 루프 반복에서 나간다. 그 사이에 X 창을 지우면 GDK 는 자기가 아는 살아 있는
/// ID 로 요청을 내고 그것이 `BadWindow` 가 된다 — GDK 는 Xlib 에러 핸들러를
/// **프로세스 전역**으로 걸어 두므로(`XSetErrorHandler` 는 연결별이 아니다) 그 자리에서
/// 프로세스가 통째로 abort 한다. 실측(2026-09-08, 격리 홈, debug 빌드): webview 탭이
/// 있는 창을 닫으면 `BadWindow ... request_code 10`(=`UnmapWindow`) 으로 죽었고,
/// 탭만 닫는 경로(부모가 살아 있는 경우)는 살아남았다.
///
/// 여기서 도는 것은 이 백엔드가 등록한 GTK 시그널(decide-policy · load-changed ·
/// 키 브리지)뿐이고 그것들은 `Rc<Cell>`/`Rc<RefCell>` 만 만지므로 `Drop` 으로
/// 재진입하지 않는다.
fn pump_gtk() {
    while gtk::events_pending() {
        gtk::main_iteration_do(false);
    }
}

/// X11 hardware keycode → winit `PhysicalKey`.
///
/// X11/Wayland 는 linux evdev scancode 에 `+8` 한 값을 keycode 로 쓴다 — winit 의
/// `PhysicalKeyExtScancode::from_scancode` 가 요구하는 것은 그 offset 을 뺀 evdev 값이다
/// (winit 문서: "A 32-bit linux scancode, which is X11/Wayland keycode subtracted by 8").
/// 그래서 이 백엔드는 winit X11 경로와 **같은 표**를 타고, 같은 물리 키에 대해 같은
/// `KeyCode` 를 낸다 — 비라틴 레이아웃 폴백이 두 경로에서 갈라지지 않는 근거다.
fn x11_keycode_to_physical(hardware_keycode: u16) -> winit::keyboard::PhysicalKey {
    use winit::platform::scancode::PhysicalKeyExtScancode;
    let Some(evdev) = (hardware_keycode as u32).checked_sub(8) else {
        return winit::keyboard::PhysicalKey::Unidentified(winit::keyboard::NativeKeyCode::Xkb(
            hardware_keycode as u32,
        ));
    };
    winit::keyboard::PhysicalKey::from_scancode(evdev)
}

/// GDK modifier 상태 → winit `ModifiersState`.
///
/// `matches_binding` 이 winit 규칙으로 판정하므로 여기서 표현을 맞춘다. Linux 에서
/// 바인딩 토큰 `alt` 는 winit `ALT`(GDK `MOD1_MASK`)에 대응하고 `option` 은 쓰이지
/// 않는다(`docs/design/policies/key-mapping.md` 의 위치 기반 추상화 — macOS 에서만
/// Command/Option 로 갈라진다).
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

/// GDK keyval → winit `Key`. 바인딩 매칭에 쓰이는 표현만 만들면 되므로 named key 는
/// `binding.rs` 가 이름으로 아는 집합(기능키·편집키·화살표)만 다루고, 나머지는
/// keyval 의 유니코드 표현을 `Key::Character` 로 올린다. 매핑되지 않는 keyval 은
/// `None` — 백엔드는 그런 키를 그대로 페이지에 흘린다.
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
        // modifier 자체는 유니코드가 없어 매핑되지 않는다(백엔드는 `is_modifier`
        // 로 먼저 거르지만, 변환 단계도 독립적으로 안전하다).
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
