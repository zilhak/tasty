//! Windows WebView2 wrapper.
//! Reference: wry/src/webview2/mod.rs (MIT license, Tauri)
//!
//! Creates a child HWND inside the parent window, then hosts a WebView2
//! controller inside it. Requires WebView2 runtime (Edge Chromium).

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::rc::Rc;
use std::sync::mpsc;

use webview2_com::{Microsoft::Web::WebView2::Win32::*, *};
use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

use super::keys::WebViewKeyBridge;
use super::{NavState, WebViewBounds};

pub struct PlatformWebView {
    hwnd: HWND,
    controller: ICoreWebView2Controller,
    webview: ICoreWebView2,
    /// blocked response 생성에 필요해 보관(요청 가로채기 핸들러가 사용).
    _environment: ICoreWebView2Environment,
    /// 원격 허용 여부(기본 false=차단). WebResourceRequested 핸들러가 매 요청 read.
    allow_remote: Rc<Cell<bool>>,
    /// navigation 생명주기 상태(기본 Idle). NavigationStarting/Completed 핸들러가 갱신,
    /// host sync_webviews 가 `nav_state()` 로 read 해 chrome/가시성에 쓴다. 콜백이 전부
    /// controller pump thread(=winit main thread) 발화라 `Rc<Cell>` 로 충분(allow_remote 동일).
    nav_state: Rc<Cell<NavState>>,
    /// NavigationStarting 이 캡처한, 아직 host 에 통지되지 않은 navigation 시도 URL 큐
    /// (도착 순서 보존). host `sync_webviews` 가 매 프레임 `take_pending_navigations` 로
    /// 비우고 plugin 에 forward — "원격 http(s) 차단"(WebResourceRequested, 이 큐와 무관하게
    /// 독립 동작)과는 별개로 차단 여부와 무관하게 모든 navigation 시도마다 쌓인다.
    pending_navigations: Rc<RefCell<Vec<String>>>,
    /// 부모 winit HWND. `release_keyboard_focus` 가 키보드 포커스를 여기로 되돌린다
    /// (overlay 가 열려 webview 를 숨길 때).
    parent_hwnd: HWND,
}

impl PlatformWebView {
    pub fn new(
        window: &impl HasWindowHandle,
        bounds: WebViewBounds,
        scale_factor: f64,
        surface_id: u32,
        key_bridge: Rc<WebViewKeyBridge>,
    ) -> std::result::Result<Self, String> {
        let parent = match window.window_handle().map_err(|e| e.to_string())?.as_raw() {
            RawWindowHandle::Win32(w) => HWND(w.hwnd.get() as *mut std::ffi::c_void),
            _ => return Err("Not a Win32 window".to_string()),
        };

        // SAFETY: WebView2 컨트롤러는 main thread (winit event loop)에서만 생성/소멸한다.
        // WNDCLASSEXW.lpfnWndProc에 DefWindowProcW를 transmute로 함수 포인터로 대입하는데,
        // DefWindowProcW의 시그니처와 WNDPROC 시그니처가 호환됨을 windows-rs가 보장.
        // CreateWindowExW의 parent HWND는 winit이 살아있는 동안 valid.
        // CreateCoreWebView2EnvironmentWithOptions/Controller는 async 핸들러를 통해
        // mpsc로 결과를 회수하며 wait_with_pump가 메시지 펌프를 돌려 deadlock 방지.
        unsafe {
            // CoInitializeEx는 같은 thread에서 이전에 APARTMENTTHREADED로 초기화돼 있으면
            // S_FALSE를, 다른 모드로 초기화돼 있으면 RPC_E_CHANGED_MODE를 돌려준다 — WebView2는
            // 두 케이스 모두에서 동작하므로 결과를 다음 호출 분기 재료로 쓰지 않는다.
            // (다른 모드여도 이후 CreateCoreWebView2EnvironmentWithOptions가 자체적으로 실패.)
            let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            if hr.is_err() {
                tracing::trace!("CoInitializeEx returned non-success HRESULT: {hr:?}");
            }

            // Create container child HWND
            let class_name = w!("TASTY_WEBVIEW");
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(std::mem::transmute(DefWindowProcW as *const () as usize)),
                hInstance: GetModuleHandleW(None).unwrap_or_default().into(),
                lpszClassName: class_name,
                ..Default::default()
            };
            RegisterClassExW(&wc);

            let physical = bounds.to_physical(scale_factor);
            let x = physical.x as i32;
            let y = physical.y as i32;
            let w = physical.width as i32;
            let h = physical.height as i32;

            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                class_name,
                PCWSTR::null(),
                WS_CHILD | WS_CLIPCHILDREN | WS_VISIBLE,
                x,
                y,
                w,
                h,
                Some(parent),
                None,
                None,
                None,
            )
            .map_err(|e| format!("CreateWindowExW failed: {e}"))?;

            // Create WebView2 environment
            let (env_tx, env_rx) = mpsc::channel();
            CreateCoreWebView2EnvironmentWithOptions(
                PCWSTR::null(),
                PCWSTR::null(),
                None,
                &CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(
                    move |_hr, env| {
                        if let Err(e) = env_tx.send(env) {
                            tracing::warn!("WebView2 env handoff failed: {e}");
                        }
                        Ok(())
                    },
                )),
            )
            .map_err(|e| format!("CreateEnvironment failed: {e}"))?;

            let env = webview2_com::wait_with_pump(env_rx)
                .map_err(|e| format!("Environment wait failed: {e}"))?
                .ok_or("No environment returned")?;

            // Create controller
            let (ctrl_tx, ctrl_rx) = mpsc::channel();
            env.CreateCoreWebView2Controller(
                hwnd,
                &CreateCoreWebView2ControllerCompletedHandler::create(Box::new(
                    move |_hr, ctrl| {
                        if let Err(e) = ctrl_tx.send(ctrl) {
                            tracing::warn!("WebView2 controller handoff failed: {e}");
                        }
                        Ok(())
                    },
                )),
            )
            .map_err(|e| format!("CreateController failed: {e}"))?;

            let controller = webview2_com::wait_with_pump(ctrl_rx)
                .map_err(|e| format!("Controller wait failed: {e}"))?
                .ok_or("No controller returned")?;

            // Set bounds
            controller
                .SetBounds(RECT {
                    left: 0,
                    top: 0,
                    right: w,
                    bottom: h,
                })
                .map_err(|e| format!("SetBounds failed: {e}"))?;

            let webview: ICoreWebView2 = controller
                .CoreWebView2()
                .map_err(|e| format!("CoreWebView2 failed: {e}"))?;

            // WebView2 는 기본적으로(`AreBrowserAcceleratorKeysEnabled`=true) Ctrl+F/Ctrl+P/
            // F3/F5/F12 등 브라우저 accelerator 키 세트를 자체 소비한다 — 예를 들어 Ctrl+F 는
            // 페이지 JS 에 keydown 이 전달되기도 전에 WebView2 자신의 네이티브 find 바를 띄운다.
            // tasty 는 임베디드 콘텐츠 뷰어이지 브라우저 chrome 이 아니므로(Linux WebKitGTK/
            // macOS WKWebView 모두 이런 기본 accelerator 세트 자체가 없음 — 대응 파일 참조)
            // 이 세트를 통째로 끈다. markdown plugin 의 문서-내 검색(render.rs
            // `find_in_page_script`)이 스스로 Ctrl+F 를 처리하려면 그 keydown 이 페이지에
            // 도달해야 하므로 필수. `ICoreWebView2Settings3`(1.0.774+) 캐스트 실패/API 실패는
            // 치명적이지 않게 로그만(구형 WebView2 런타임 폴백 — accelerator 가로챔이 남지만
            // 문서 로드 자체는 계속 동작).
            match webview.Settings() {
                Ok(settings) => match settings.cast::<ICoreWebView2Settings3>() {
                    Ok(settings3) => {
                        if let Err(e) = settings3.SetAreBrowserAcceleratorKeysEnabled(false) {
                            tracing::warn!(
                                "WebView2 SetAreBrowserAcceleratorKeysEnabled failed: {e}"
                            );
                        }
                    }
                    Err(e) => tracing::warn!(
                        "WebView2 ICoreWebView2Settings3 cast failed (구형 런타임?): {e}"
                    ),
                },
                Err(e) => tracing::warn!("WebView2 Settings() failed: {e}"),
            }

            // 원격 콘텐츠 차단: 모든 리소스 요청을 WebResourceRequested 로 가로채,
            // allow_remote=false 일 때 http/https URI 면 403 빈 응답으로 대체한다(기본 차단).
            let allow_remote = Rc::new(Cell::new(false));
            webview
                .AddWebResourceRequestedFilter(w!("*"), COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL)
                .map_err(|e| format!("AddWebResourceRequestedFilter failed: {e}"))?;
            let env_cb = env.clone();
            let allow_cb = allow_remote.clone();
            // webview2-com 0.38 (windows 0.61) 의 add_WebResourceRequested 는
            // token 을 EventRegistrationToken 이 아닌 *mut i64 로 받는다.
            let mut token: i64 = 0;
            let handler = WebResourceRequestedEventHandler::create(Box::new(
                move |_sender, args| -> windows::core::Result<()> {
                    let Some(args) = args else { return Ok(()) };
                    let request = args.Request()?;
                    // webview2-com 0.38 (windows 0.61): Uri 는 반환값이 아니라
                    // *mut PWSTR out-param 으로 결과를 돌려준다.
                    let mut uri = windows::core::PWSTR::null();
                    request.Uri(&mut uri)?;
                    let uri_str = uri.to_string().unwrap_or_default();
                    // WebView2 가 할당한 URI 문자열은 호출자가 CoTaskMemFree 로 해제해야 한다.
                    CoTaskMemFree(Some(uri.0 as *const c_void));
                    let is_remote =
                        uri_str.starts_with("http://") || uri_str.starts_with("https://");
                    if is_remote && !allow_cb.get() {
                        let resp =
                            env_cb.CreateWebResourceResponse(None, 403, w!("Blocked"), w!(""))?;
                        args.SetResponse(&resp)?;
                    }
                    Ok(())
                },
            ));
            webview
                .add_WebResourceRequested(&handler, &mut token)
                .map_err(|e| format!("add_WebResourceRequested failed: {e}"))?;

            // navigation 생명주기 콜백. start→Loading, completed→Done/Failed(IsSuccess out-param).
            // 토큰은 WebResourceRequested 와 동일하게 *mut i64(EventRegistrationToken 아님).
            let nav_state = Rc::new(Cell::new(NavState::Idle));
            let pending_navigations: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
            let nav_start = nav_state.clone();
            let pending_nav = pending_navigations.clone();
            let mut tok_start: i64 = 0;
            let h_start = NavigationStartingEventHandler::create(Box::new(
                move |_sender, args| -> windows::core::Result<()> {
                    nav_start.set(NavState::Loading);
                    // navigation 시도 URL 캡처 — 아래 원격 차단(WebResourceRequested)과
                    // 독립적으로, 차단 여부와 무관하게 항상 기록한다.
                    if let Some(args) = args {
                        let mut uri = windows::core::PWSTR::null();
                        args.Uri(&mut uri)?;
                        let uri_str = uri.to_string().unwrap_or_default();
                        // WebView2 가 할당한 URI 문자열은 호출자가 CoTaskMemFree 로 해제해야
                        // 한다(WebResourceRequested 핸들러와 동일 컨벤션).
                        CoTaskMemFree(Some(uri.0 as *const c_void));
                        pending_nav.borrow_mut().push(uri_str);
                    }
                    Ok(())
                },
            ));
            webview
                .add_NavigationStarting(&h_start, &mut tok_start)
                .map_err(|e| format!("add_NavigationStarting failed: {e}"))?;

            let nav_done = nav_state.clone();
            let mut tok_done: i64 = 0;
            let h_done = NavigationCompletedEventHandler::create(Box::new(
                move |_sender, args| -> windows::core::Result<()> {
                    let Some(args) = args else { return Ok(()) };
                    // windows 0.61: IsSuccess 는 BOOL out-param(WebResourceRequested 의 Uri 와 동일 컨벤션).
                    let mut is_success = BOOL(0);
                    args.IsSuccess(&mut is_success)?;
                    if is_success.as_bool() {
                        nav_done.set(NavState::Done);
                    } else {
                        let mut status = COREWEBVIEW2_WEB_ERROR_STATUS::default();
                        if let Err(e) = args.WebErrorStatus(&mut status) {
                            tracing::warn!("WebView2 WebErrorStatus query failed: {e}");
                        }
                        // 사유는 로그 전용 — 화면 error chrome 은 URL 만 보여준다.
                        tracing::warn!("WebView2 navigation failed: status={status:?}");
                        nav_done.set(NavState::Failed);
                    }
                    Ok(())
                },
            ));
            webview
                .add_NavigationCompleted(&h_done, &mut tok_done)
                .map_err(|e| format!("add_NavigationCompleted failed: {e}"))?;

            // 키 포워딩 — `AcceleratorKeyPressed` 는 정확히 이 목적을 위한 API 다
            // (WebView2 가 페이지에 키를 넘기기 **전에** host 에게 먼저 묻는다).
            // `SetHandled(true)` 면 페이지가 그 키를 보지 못한다.
            let key_bridge_cb = key_bridge.clone();
            let mut tok_key: i64 = 0;
            let h_key = AcceleratorKeyPressedEventHandler::create(Box::new(
                move |_sender, args| -> windows::core::Result<()> {
                    let Some(args) = args else { return Ok(()) };
                    let mut kind = COREWEBVIEW2_KEY_EVENT_KIND::default();
                    args.KeyEventKind(&mut kind)?;
                    // press 만 포워딩한다(up 은 무시). Alt 조합은 SYSTEM_KEY_DOWN 으로 온다.
                    if kind != COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN
                        && kind != COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN
                    {
                        return Ok(());
                    }
                    let mut status = COREWEBVIEW2_PHYSICAL_KEY_STATUS::default();
                    args.PhysicalKeyStatus(&mut status)?;
                    // auto-repeat 제외 — 직전에 이미 눌려 있던 키는 반복 이벤트다.
                    if status.WasKeyDown.as_bool() {
                        return Ok(());
                    }
                    let mut vk: u32 = 0;
                    args.VirtualKey(&mut vk)?;
                    let Some(key) = vk_to_winit_key(vk) else {
                        return Ok(());
                    };
                    if key_bridge_cb.capture_key(surface_id, key, current_winit_mods()) {
                        args.SetHandled(true)?;
                    }
                    Ok(())
                },
            ));
            controller
                .add_AcceleratorKeyPressed(&h_key, &mut tok_key)
                .map_err(|e| format!("add_AcceleratorKeyPressed failed: {e}"))?;

            // 클릭 등으로 webview 가 포커스를 가져가면 host 모델 포커스를 맞춘다 —
            // 그 클릭은 winit 에 도달하지 않아 `try_click_to_activate` 가 안 돈다.
            let key_bridge_focus = key_bridge.clone();
            let mut tok_focus: i64 = 0;
            let h_focus = FocusChangedEventHandler::create(Box::new(
                move |_sender, _args| -> windows::core::Result<()> {
                    key_bridge_focus.note_focus(surface_id);
                    Ok(())
                },
            ));
            controller
                .add_GotFocus(&h_focus, &mut tok_focus)
                .map_err(|e| format!("add_GotFocus failed: {e}"))?;

            Ok(Self {
                hwnd,
                controller,
                webview,
                _environment: env,
                allow_remote,
                nav_state,
                pending_navigations,
                parent_hwnd: parent,
            })
        }
    }

    /// 키보드 포커스를 부모 winit 창으로 되돌린다(overlay 개시 시). 숨기는 것과
    /// 포커스를 놓는 것은 Win32 에서도 별개라, 회수하지 않으면 방금 연 popup 이
    /// 키를 못 받는다.
    ///
    /// **키보드 포커스가 실제로 이 webview 자식 창 안에 있을 때만** 회수한다 —
    /// Linux/macOS 백엔드와 같은 규칙이다. 포그라운드 잠금이 알아서 막아주리라 기대하지
    /// 않고 명시적으로 건다: 그렇지 않으면 IPC 로 popup 을 여는 것만으로 tasty 가
    /// 사용자 포커스에 손대는 셈이 된다(불가침 원칙 1, `docs/identity.md`).
    pub fn release_keyboard_focus(&self) {
        if !self.focus_is_inside() {
            return;
        }
        // SAFETY: parent_hwnd 는 이 webview 를 만든 winit 창이고 self 가 살아있는
        // 동안 valid. 호출은 main thread(winit event loop).
        unsafe {
            let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(self.parent_hwnd));
            // reason: 포커스 이동 실패는 창이 이미 사라지는 중이라는 뜻이라 되돌릴
            // 것이 없다. 직전 포커스 HWND 를 돌려주는 API 라 에러가 아니라 None 이
            // 정상 경로에도 나오므로 로그도 남기지 않는다.
        }
    }

    /// 현재 키보드 포커스가 이 webview 창 자신이거나 그 하위 창인지.
    fn focus_is_inside(&self) -> bool {
        // SAFETY: 호출은 main thread(winit event loop). GetFocus 는 인자가 없고, 이
        // 스레드 메시지 큐가 활성이 아니면 널 HWND 를 돌려준다 — 다른 앱이 포커스를
        // 쥔 상황이 여기서 걸러진다.
        let focus = unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetFocus() };
        if focus.is_invalid() {
            return false;
        }
        if focus == self.hwnd {
            return true;
        }
        // SAFETY: self.hwnd 는 self 수명 동안 valid 하고 focus 는 바로 위에서 널이 아님을
        // 확인했다.
        unsafe { windows::Win32::UI::WindowsAndMessaging::IsChild(self.hwnd, focus).as_bool() }
    }

    pub fn set_bounds(&self, bounds: WebViewBounds, scale_factor: f64) {
        let physical = bounds.to_physical(scale_factor);
        let x = physical.x as i32;
        let y = physical.y as i32;
        let w = physical.width as i32;
        let h = physical.height as i32;

        // SAFETY: SetBounds/SetWindowPos는 self가 살아있는 동안 hwnd/controller가 valid
        // 함을 Drop 시점에 정리. 호출은 main thread에서 일어남.
        unsafe {
            if let Err(e) = self.controller.SetBounds(RECT {
                left: 0,
                top: 0,
                right: w,
                bottom: h,
            }) {
                tracing::warn!("WebView2 SetBounds failed: {e}");
            }
            if let Err(e) = SetWindowPos(
                self.hwnd,
                None,
                x,
                y,
                w,
                h,
                SWP_ASYNCWINDOWPOS | SWP_NOACTIVATE | SWP_NOZORDER,
            ) {
                tracing::warn!("SetWindowPos failed: {e}");
            }
        }
    }

    pub fn set_visible(&self, visible: bool) {
        // SAFETY: self가 살아있으면 hwnd/controller 모두 valid (Drop이 정리).
        unsafe {
            // ShowWindow는 BOOL을 반환하지만 windows-rs는 Result로 wrapping —
            // "이전 상태가 visible이었는가"라서 첫 호출 시 Err 형태일 수 있다. 무해.
            let _ = ShowWindow(self.hwnd, if visible { SW_SHOW } else { SW_HIDE }); // 반환은 "이전 visible 상태" — 첫 호출 Err 형태 가능, 무해(위 주석 참조).
            if let Err(e) = self.controller.SetIsVisible(visible) {
                tracing::warn!("WebView2 SetIsVisible failed: {e}");
            }
        }
    }

    /// 현재 navigation 생명주기 상태(NavigationStarting/Completed 핸들러가 갱신).
    pub fn nav_state(&self) -> NavState {
        self.nav_state.get()
    }

    /// NavigationStarting 이 캡처한 navigation 시도 URL 을 도착 순서대로 비워서 반환한다.
    /// host `sync_webviews` 가 매 프레임 호출해 plugin 에 forward.
    pub fn take_pending_navigations(&self) -> Vec<String> {
        std::mem::take(&mut *self.pending_navigations.borrow_mut())
    }

    pub fn load_url(&self, url: &str) {
        // 콜백이 늦게 와도 즉시 spinner 가 뜨도록 Loading 선반영(첫 프레임 깜빡임 방지).
        self.nav_state.set(NavState::Loading);
        // SAFETY: HSTRING은 호출 끝까지 살아있고 Navigate는 main thread 호출.
        unsafe {
            let url = HSTRING::from(url);
            if let Err(e) = self.webview.Navigate(&url) {
                tracing::warn!("WebView2 Navigate failed: {e}");
            }
        }
    }

    pub fn load_html(&self, html: &str) {
        // Loading 선반영(load_url 과 동일 — 콜백 지연 대비).
        self.nav_state.set(NavState::Loading);
        // SAFETY: HSTRING은 호출 끝까지 살아있고 NavigateToString은 main thread 호출.
        unsafe {
            let html = HSTRING::from(html);
            if let Err(e) = self.webview.NavigateToString(&html) {
                tracing::warn!("WebView2 NavigateToString failed: {e}");
            }
        }
    }

    /// Content zoom (1.0 = 100%). WebView2 `ICoreWebView2Controller::SetZoomFactor`.
    pub fn set_zoom(&self, factor: f64) {
        // SAFETY: controller는 self가 살아있는 동안 valid, main thread 호출.
        unsafe {
            if let Err(e) = self.controller.SetZoomFactor(factor) {
                tracing::warn!("WebView2 SetZoomFactor failed: {e}");
            }
        }
    }

    /// JavaScript 실행 허용 여부. WebView2 `ICoreWebView2Settings::IsScriptEnabled` —
    /// 다음 네비게이션부터 적용. host 는 "Sandbox scripts" on(기본) → `enabled=false`.
    pub fn set_javascript_enabled(&self, enabled: bool) {
        // SAFETY: webview/settings는 self가 살아있는 동안 valid, main thread 호출.
        unsafe {
            match self.webview.Settings() {
                Ok(settings) => {
                    if let Err(e) = settings.SetIsScriptEnabled(enabled) {
                        tracing::warn!("WebView2 SetIsScriptEnabled failed: {e}");
                    }
                }
                Err(e) => tracing::warn!("WebView2 Settings() failed: {e}"),
            }
        }
    }

    /// `prefers-color-scheme` 강제. WebView2 Profile.PreferredColorScheme 으로 가능하나
    /// 현재 no-op — 후속(`ICoreWebView2_13::Profile` 캐스팅). `scheme` 만 로깅.
    pub fn set_color_scheme(&self, scheme: super::ColorScheme) {
        tracing::debug!(
            "set_color_scheme({scheme:?}) — Windows WebView2 no-op (후속: Profile.PreferredColorScheme)"
        );
    }

    /// 원격(http/https) 콘텐츠 허용 여부. `new()` 에서 등록한 WebResourceRequested
    /// 핸들러가 매 요청마다 이 플래그를 read 해 `false`면 원격 URI 를 403 으로 차단한다.
    /// 여기서는 플래그만 갱신(다음 리소스 요청부터 반영).
    pub fn set_remote_content_allowed(&self, allowed: bool) {
        self.allow_remote.set(allowed);
        tracing::debug!("Windows WebView2 set_remote_content_allowed({allowed})");
    }
}

impl Drop for PlatformWebView {
    fn drop(&mut self) {
        // SAFETY: controller.Close()는 webview2 자원 해제, DestroyWindow는 child HWND 정리.
        // 둘 다 self가 처음 만들어진 main thread에서 Drop이 호출된다는 전제 (PlatformWebView는
        // !Send/!Sync 기본 — COM 객체 포함).
        unsafe {
            // Drop 정리 — 이미 닫혔거나 HWND가 사라진 경우(예: 윈도우가 먼저 죽음)에도
            // 호스트가 추가로 할 수 있는 일이 없으므로 trace로만 흔적.
            if let Err(e) = self.controller.Close() {
                tracing::trace!("WebView2 controller Close failed: {e}");
            }
            if let Err(e) = DestroyWindow(self.hwnd) {
                tracing::trace!("DestroyWindow failed: {e}");
            }
        }
    }
}

/// 현재 modifier 키 상태 → winit `ModifiersState`.
///
/// `AcceleratorKeyPressed` args 는 modifier 상태를 싣지 않으므로 키보드 상태를
/// 직접 읽는다. Windows 에서 바인딩 토큰 `alt` 는 winit `ALT` 에 대응하고 `option`
/// 은 쓰이지 않는다(`docs/design/policies/key-mapping.md`).
fn current_winit_mods() -> winit::keyboard::ModifiersState {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
    };
    use winit::keyboard::ModifiersState;
    let down = |vk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY| -> bool {
        // SAFETY: GetKeyState 는 호출 스레드의 키보드 상태만 읽는 순수 조회 API 다.
        (unsafe { GetKeyState(vk.0 as i32) } as u16 & 0x8000) != 0
    };
    let mut mods = ModifiersState::empty();
    mods.set(ModifiersState::CONTROL, down(VK_CONTROL));
    mods.set(ModifiersState::SHIFT, down(VK_SHIFT));
    mods.set(ModifiersState::ALT, down(VK_MENU));
    mods.set(ModifiersState::SUPER, down(VK_LWIN) || down(VK_RWIN));
    mods
}

/// Win32 virtual-key → winit `Key`. 바인딩 매칭에 쓰이는 표현만 만들면 되므로
/// `binding.rs` 가 이름으로 아는 named key 집합과 문자/숫자/기호만 다루고, 나머지는
/// `None`(백엔드는 그대로 페이지에 흘린다).
fn vk_to_winit_key(vk: u32) -> Option<winit::keyboard::Key> {
    use winit::keyboard::{Key, NamedKey};
    let named = match vk as u16 {
        0x09 => NamedKey::Tab,
        0x0D => NamedKey::Enter,
        0x08 => NamedKey::Backspace,
        0x2E => NamedKey::Delete,
        0x2D => NamedKey::Insert,
        0x24 => NamedKey::Home,
        0x23 => NamedKey::End,
        0x21 => NamedKey::PageUp,
        0x22 => NamedKey::PageDown,
        0x26 => NamedKey::ArrowUp,
        0x28 => NamedKey::ArrowDown,
        0x25 => NamedKey::ArrowLeft,
        0x27 => NamedKey::ArrowRight,
        0x1B => NamedKey::Escape,
        0x20 => NamedKey::Space,
        0x70 => NamedKey::F1,
        0x71 => NamedKey::F2,
        0x72 => NamedKey::F3,
        0x73 => NamedKey::F4,
        0x74 => NamedKey::F5,
        0x75 => NamedKey::F6,
        0x76 => NamedKey::F7,
        0x77 => NamedKey::F8,
        0x78 => NamedKey::F9,
        0x79 => NamedKey::F10,
        0x7A => NamedKey::F11,
        0x7B => NamedKey::F12,
        // 문자·숫자는 소문자/숫자 문자로 올린다(`matches_binding` 이 대소문자 무시).
        c @ 0x41..=0x5A => {
            return Some(Key::Character(((c as u8 + 32) as char).to_string().into()));
        }
        c @ 0x30..=0x39 => return Some(Key::Character(((c as u8) as char).to_string().into())),
        c @ 0x60..=0x69 => {
            // 넘패드 0~9.
            return Some(Key::Character(
                ((c as u8 - 0x60 + b'0') as char).to_string().into(),
            ));
        }
        0xBB | 0x6B => return Some(Key::Character("=".into())), // OEM_PLUS / numpad ADD
        0xBD | 0x6D => return Some(Key::Character("-".into())), // OEM_MINUS / numpad SUBTRACT
        0xBC => return Some(Key::Character(",".into())),
        0xBE => return Some(Key::Character(".".into())),
        0xBF => return Some(Key::Character("/".into())),
        0xC0 => return Some(Key::Character("`".into())),
        0xDB => return Some(Key::Character("[".into())),
        0xDC => return Some(Key::Character("\\".into())),
        0xDD => return Some(Key::Character("]".into())),
        0xDE => return Some(Key::Character("'".into())),
        0xBA => return Some(Key::Character(";".into())),
        _ => return None,
    };
    Some(Key::Named(named))
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::keyboard::{Key, NamedKey};

    #[test]
    fn vk_maps_to_character_and_named() {
        assert_eq!(vk_to_winit_key(0x44), Some(Key::Character("d".into()))); // VK_D
        assert_eq!(vk_to_winit_key(0x32), Some(Key::Character("2".into()))); // VK_2
        assert_eq!(vk_to_winit_key(0xBB), Some(Key::Character("=".into()))); // VK_OEM_PLUS
        assert_eq!(vk_to_winit_key(0x1B), Some(Key::Named(NamedKey::Escape)));
        // modifier 자체는 매핑하지 않는다(단독으로는 어떤 단축키도 매칭되지 않는다).
        assert_eq!(vk_to_winit_key(0x11), None); // VK_CONTROL
    }
}
