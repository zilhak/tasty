//! Windows WebView2 wrapper.
//! Reference: wry/src/webview2/mod.rs (MIT license, Tauri)
//! 부모 안의 child HWND에 WebView2 controller를 만든다. WebView2 runtime이 필요하다.

// 이유: native 포인터·핸들 호출을 묶는 FFI 모듈이다. 파일 단위 면제는 새 코드에도 적용되므로 안전 래퍼로 옮기면 범위를 줄여야 한다.
#![allow(clippy::multiple_unsafe_ops_per_block)]

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

use super::keys::WebViewKeySink;
use super::{NavState, PendingNavigation, WebViewBounds};

pub struct PlatformWebView {
    hwnd: HWND,
    controller: ICoreWebView2Controller,
    webview: ICoreWebView2,
    _environment: ICoreWebView2Environment,
    allow_remote: Rc<Cell<bool>>,
    nav_state: Rc<Cell<NavState>>,
    /// 원격 차단 여부와 별도로 기록한 NavigationStarting 시도 큐.
    pending_navigations: Rc<RefCell<Vec<PendingNavigation>>>,
    parent_hwnd: HWND,
}

/// 창 핸들·종류 오류는 Permanent, 이후 native 생성·등록 오류는 Transient로 분류한다.
/// 실제 일시 오류인지 모두 확인한 분류는 아니며 호출자의 재시도 상한이 필요하다.
fn perm(msg: impl std::fmt::Display) -> super::WebViewCreateError {
    super::WebViewCreateError::Permanent(msg.to_string())
}

fn transient(msg: impl std::fmt::Display) -> super::WebViewCreateError {
    super::WebViewCreateError::Transient(msg.to_string())
}

impl PlatformWebView {
    pub fn new(
        window: &impl HasWindowHandle,
        bounds: WebViewBounds,
        scale_factor: f64,
        surface_id: u32,
        key_bridge: Rc<dyn WebViewKeySink>,
    ) -> std::result::Result<Self, super::WebViewCreateError> {
        let parent = match window.window_handle().map_err(perm)?.as_raw() {
            RawWindowHandle::Win32(w) => HWND(w.hwnd.get() as *mut std::ffi::c_void),
            _ => return Err(perm("Not a Win32 window")),
        };

        // SAFETY: winit main thread에서 유효한 부모 HWND를 받아 child window와 COM 객체를 만든다.
        // WNDPROC는 Win32 호출 시그니처를 사용한다. 비동기 결과를 기다리는 동안 wait_with_pump가 메시지를 처리한다.
        unsafe {
            // COM 초기화 결과는 로그만 남기고 이후 WebView2 생성 결과로 진행 여부를 판단한다.
            let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            if hr.is_err() {
                tracing::trace!("CoInitializeEx returned non-success HRESULT: {hr:?}");
            }

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
            .map_err(|e| transient(format!("CreateWindowExW failed: {e}")))?;

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
            .map_err(|e| transient(format!("CreateEnvironment failed: {e}")))?;

            let env = webview2_com::wait_with_pump(env_rx)
                .map_err(|e| transient(format!("Environment wait failed: {e}")))?
                .ok_or_else(|| transient("No environment returned"))?;

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
            .map_err(|e| transient(format!("CreateController failed: {e}")))?;

            let controller = webview2_com::wait_with_pump(ctrl_rx)
                .map_err(|e| transient(format!("Controller wait failed: {e}")))?
                .ok_or_else(|| transient("No controller returned"))?;

            controller
                .SetBounds(RECT {
                    left: 0,
                    top: 0,
                    right: w,
                    bottom: h,
                })
                .map_err(|e| transient(format!("SetBounds failed: {e}")))?;

            let webview: ICoreWebView2 = controller
                .CoreWebView2()
                .map_err(|e| transient(format!("CoreWebView2 failed: {e}")))?;

            // 브라우저 자체 accelerator가 페이지의 찾기 키 등을 먼저 소비하지 않도록 끈다.
            // 구형 API나 호출 실패는 로그 후 계속하므로 그 경우 기존 accelerator가 남을 수 있다.
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

            // 차단 상태에서 http(s) 요청을 빈 403 응답으로 대체한다.
            let allow_remote = Rc::new(Cell::new(false));
            webview
                .AddWebResourceRequestedFilter(w!("*"), COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL)
                .map_err(|e| transient(format!("AddWebResourceRequestedFilter failed: {e}")))?;
            let env_cb = env.clone();
            let allow_cb = allow_remote.clone();
            let mut token: i64 = 0;
            let handler = WebResourceRequestedEventHandler::create(Box::new(
                move |_sender, args| -> windows::core::Result<()> {
                    let Some(args) = args else { return Ok(()) };
                    let request = args.Request()?;
                    let mut uri = windows::core::PWSTR::null();
                    request.Uri(&mut uri)?;
                    let uri_str = uri.to_string().unwrap_or_default();
                    // WebView2가 할당한 URI를 CoTaskMemFree로 해제한다.
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
                .map_err(|e| transient(format!("add_WebResourceRequested failed: {e}")))?;

            let nav_state = Rc::new(Cell::new(NavState::Idle));
            let pending_navigations: Rc<RefCell<Vec<PendingNavigation>>> =
                Rc::new(RefCell::new(Vec::new()));
            let nav_start = nav_state.clone();
            let pending_nav = pending_navigations.clone();
            let mut tok_start: i64 = 0;
            let h_start = NavigationStartingEventHandler::create(Box::new(
                move |_sender, args| -> windows::core::Result<()> {
                    tracing::debug!("WebView surface {surface_id}: load started");
                    nav_start.set(NavState::Loading);
                    if let Some(args) = args {
                        let mut uri = windows::core::PWSTR::null();
                        args.Uri(&mut uri)?;
                        let uri_str = uri.to_string().unwrap_or_default();
                        // 반환 URI의 할당은 호출자가 해제한다.
                        CoTaskMemFree(Some(uri.0 as *const c_void));
                        // 제스처 여부 조회 실패도 탐색 자체는 기록하되 false로 남긴다.
                        let mut user_initiated = BOOL(0);
                        let user_gesture = match args.IsUserInitiated(&mut user_initiated) {
                            Ok(()) => user_initiated.as_bool(),
                            Err(e) => {
                                tracing::warn!(
                                    "WebView surface {surface_id}: IsUserInitiated failed ({e}); \
                                     the navigation is reported as not user-initiated"
                                );
                                false
                            }
                        };
                        pending_nav.borrow_mut().push(PendingNavigation {
                            url: uri_str,
                            user_gesture,
                        });
                    }
                    Ok(())
                },
            ));
            webview
                .add_NavigationStarting(&h_start, &mut tok_start)
                .map_err(|e| transient(format!("add_NavigationStarting failed: {e}")))?;

            let nav_done = nav_state.clone();
            let mut tok_done: i64 = 0;
            let h_done = NavigationCompletedEventHandler::create(Box::new(
                move |_sender, args| -> windows::core::Result<()> {
                    let Some(args) = args else { return Ok(()) };
                    let mut is_success = BOOL(0);
                    args.IsSuccess(&mut is_success)?;
                    if is_success.as_bool() {
                        tracing::debug!("WebView surface {surface_id}: load finished");
                        nav_done.set(NavState::Done);
                    } else {
                        let mut status = COREWEBVIEW2_WEB_ERROR_STATUS::default();
                        if let Err(e) = args.WebErrorStatus(&mut status) {
                            tracing::warn!("WebView2 WebErrorStatus query failed: {e}");
                        }
                        tracing::warn!(
                            "WebView surface {surface_id}: WebView2 navigation failed: status={status:?}"
                        );
                        nav_done.set(NavState::Failed);
                    }
                    Ok(())
                },
            ));
            webview
                .add_NavigationCompleted(&h_done, &mut tok_done)
                .map_err(|e| transient(format!("add_NavigationCompleted failed: {e}")))?;

            let key_bridge_cb = key_bridge.clone();
            let mut tok_key: i64 = 0;
            let h_key = AcceleratorKeyPressedEventHandler::create(Box::new(
                move |_sender, args| -> windows::core::Result<()> {
                    let Some(args) = args else { return Ok(()) };
                    let mut kind = COREWEBVIEW2_KEY_EVENT_KIND::default();
                    args.KeyEventKind(&mut kind)?;
                    if kind != COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN
                        && kind != COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN
                    {
                        return Ok(());
                    }
                    let mut status = COREWEBVIEW2_PHYSICAL_KEY_STATUS::default();
                    args.PhysicalKeyStatus(&mut status)?;
                    if status.WasKeyDown.as_bool() {
                        return Ok(());
                    }
                    let mut vk: u32 = 0;
                    args.VirtualKey(&mut vk)?;
                    let Some(key) = vk_to_winit_key(vk) else {
                        return Ok(());
                    };
                    let physical =
                        win32_scancode_to_physical(status.ScanCode, status.IsExtendedKey.as_bool());
                    if key_bridge_cb.capture_key(surface_id, key, physical, current_winit_mods()) {
                        args.SetHandled(true)?;
                    }
                    Ok(())
                },
            ));
            controller
                .add_AcceleratorKeyPressed(&h_key, &mut tok_key)
                .map_err(|e| transient(format!("add_AcceleratorKeyPressed failed: {e}")))?;

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
                .map_err(|e| transient(format!("add_GotFocus failed: {e}")))?;

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

    /// 이 WebView 또는 자손이 키보드 포커스를 가졌을 때만 부모로 돌린다.
    pub fn release_keyboard_focus(&self) {
        if !self.focus_is_inside() {
            return;
        }
        // SAFETY: parent_hwnd 는 이 webview 를 만든 winit 창이고 self 가 살아있는
        // 동안 valid. 호출은 main thread(winit event loop).
        unsafe {
            let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(self.parent_hwnd));
            // reason: 반환은 이전 포커스 핸들이며 None만으로 실패를 판별할 수 없어 무시한다.
        }
    }

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
            // 반환값은 이전 표시 상태이며 현재 표시 요청의 성공 여부로 해석하지 않는다.
            let _ = ShowWindow(self.hwnd, if visible { SW_SHOW } else { SW_HIDE }); // 이유: 이전 표시 상태 반환은 사용하지 않는다.
            if let Err(e) = self.controller.SetIsVisible(visible) {
                tracing::warn!("WebView2 SetIsVisible failed: {e}");
            }
        }
    }

    pub fn nav_state(&self) -> NavState {
        self.nav_state.get()
    }

    pub fn take_pending_navigations(&self) -> Vec<PendingNavigation> {
        std::mem::take(&mut *self.pending_navigations.borrow_mut())
    }

    pub fn load_url(&self, url: &str) {
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
        self.nav_state.set(NavState::Loading);
        // SAFETY: HSTRING은 호출 끝까지 살아있고 NavigateToString은 main thread 호출.
        unsafe {
            let html = HSTRING::from(html);
            if let Err(e) = self.webview.NavigateToString(&html) {
                tracing::warn!("WebView2 NavigateToString failed: {e}");
            }
        }
    }

    pub fn set_zoom(&self, factor: f64) {
        // SAFETY: controller는 self가 살아있는 동안 valid, main thread 호출.
        unsafe {
            if let Err(e) = self.controller.SetZoomFactor(factor) {
                tracing::warn!("WebView2 SetZoomFactor failed: {e}");
            }
        }
    }

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

    /// 현재 Windows에서는 적용하지 않고 로그만 남긴다.
    pub fn set_color_scheme(&self, scheme: super::ColorScheme) {
        tracing::debug!("set_color_scheme({scheme:?}) is not implemented for Windows WebView2");
    }

    /// 다음 요청부터 WebResourceRequested가 읽을 허용 여부를 갱신한다.
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
            // 이미 닫힌 핸들 등 정리 오류도 trace에 남긴다.
            if let Err(e) = self.controller.Close() {
                tracing::trace!("WebView2 controller Close failed: {e}");
            }
            if let Err(e) = DestroyWindow(self.hwnd) {
                tracing::trace!("DestroyWindow failed: {e}");
            }
        }
    }
}

/// extended 키는 0xE000과 scancode를 합쳐 winit 물리 키로 변환한다.
fn win32_scancode_to_physical(scancode: u32, extended: bool) -> winit::keyboard::PhysicalKey {
    use winit::platform::scancode::PhysicalKeyExtScancode;
    let ex = (scancode & 0xff) | if extended { 0xe000 } else { 0 };
    winit::keyboard::PhysicalKey::from_scancode(ex)
}

/// callback 인자에 없는 modifier 상태를 현재 스레드의 키보드 상태에서 읽는다.
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

/// 알려진 named key와 문자·숫자·기호만 변환한다. 나머지는 None이다.
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
        c @ 0x41..=0x5A => {
            return Some(Key::Character(((c as u8 + 32) as char).to_string().into()));
        }
        c @ 0x30..=0x39 => return Some(Key::Character(((c as u8) as char).to_string().into())),
        c @ 0x60..=0x69 => {
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
        assert_eq!(vk_to_winit_key(0x11), None); // VK_CONTROL
    }
}
