//! Windows WebView2 wrapper.
//! Reference: wry/src/webview2/mod.rs (MIT license, Tauri)
//!
//! Creates a child HWND inside the parent window, then hosts a WebView2
//! controller inside it. Requires WebView2 runtime (Edge Chromium).

use std::sync::mpsc;
use webview2_com::{Microsoft::Web::WebView2::Win32::*, *};
use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

use super::WebViewBounds;

pub struct PlatformWebView {
    hwnd: HWND,
    controller: ICoreWebView2Controller,
    webview: ICoreWebView2,
}

impl PlatformWebView {
    pub fn new(
        window: &impl HasWindowHandle,
        bounds: WebViewBounds,
        scale_factor: f64,
    ) -> std::result::Result<Self, String> {
        let parent = match window.window_handle().map_err(|e| e.to_string())?.as_raw() {
            RawWindowHandle::Win32(w) => HWND(w.hwnd.get() as *mut core::ffi::c_void),
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

            Ok(Self {
                hwnd,
                controller,
                webview,
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
            let _ = ShowWindow(self.hwnd, if visible { SW_SHOW } else { SW_HIDE });
            if let Err(e) = self.controller.SetIsVisible(visible) {
                tracing::warn!("WebView2 SetIsVisible failed: {e}");
            }
        }
    }

    pub fn load_url(&self, url: &str) {
        // SAFETY: HSTRING은 호출 끝까지 살아있고 Navigate는 main thread 호출.
        unsafe {
            let url = HSTRING::from(url);
            if let Err(e) = self.webview.Navigate(&url) {
                tracing::warn!("WebView2 Navigate failed: {e}");
            }
        }
    }

    pub fn load_html(&self, html: &str) {
        // SAFETY: HSTRING은 호출 끝까지 살아있고 NavigateToString은 main thread 호출.
        unsafe {
            let html = HSTRING::from(html);
            if let Err(e) = self.webview.NavigateToString(&html) {
                tracing::warn!("WebView2 NavigateToString failed: {e}");
            }
        }
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
