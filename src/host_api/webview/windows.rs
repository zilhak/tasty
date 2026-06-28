//! Windows WebView2 wrapper.
//! Reference: wry/src/webview2/mod.rs (MIT license, Tauri)
//!
//! Creates a child HWND inside the parent window, then hosts a WebView2
//! controller inside it. Requires WebView2 runtime (Edge Chromium).

use std::cell::Cell;
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
}

impl PlatformWebView {
    pub fn new(
        window: &impl HasWindowHandle,
        bounds: WebViewBounds,
        scale_factor: f64,
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
                lpfnWndProc: Some(std::mem::transmute(DefWindowProcW as usize)),
                hInstance: GetModuleHandleW(None).unwrap_or_default().into(),
                lpszClassName: class_name,
                ..Default::default()
            };
            RegisterClassExW(&wc);

            let x = (bounds.x * scale_factor) as i32;
            let y = (bounds.y * scale_factor) as i32;
            let w = (bounds.width * scale_factor) as i32;
            let h = (bounds.height * scale_factor) as i32;

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
            let nav_start = nav_state.clone();
            let mut tok_start: i64 = 0;
            let h_start = NavigationStartingEventHandler::create(Box::new(
                move |_sender, _args| -> windows::core::Result<()> {
                    nav_start.set(NavState::Loading);
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

            Ok(Self {
                hwnd,
                controller,
                webview,
                _environment: env,
                allow_remote,
                nav_state,
            })
        }
    }

    pub fn set_bounds(&self, bounds: WebViewBounds, scale_factor: f64) {
        let x = (bounds.x * scale_factor) as i32;
        let y = (bounds.y * scale_factor) as i32;
        let w = (bounds.width * scale_factor) as i32;
        let h = (bounds.height * scale_factor) as i32;

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
