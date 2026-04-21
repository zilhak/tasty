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

        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

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
                        let _ = env_tx.send(env);
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
                        let _ = ctrl_tx.send(ctrl);
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

        unsafe {
            let _ = self.controller.SetBounds(RECT {
                left: 0,
                top: 0,
                right: w,
                bottom: h,
            });
            let _ = SetWindowPos(
                self.hwnd,
                None,
                x,
                y,
                w,
                h,
                SWP_ASYNCWINDOWPOS | SWP_NOACTIVATE | SWP_NOZORDER,
            );
        }
    }

    pub fn set_visible(&self, visible: bool) {
        unsafe {
            let _ = ShowWindow(self.hwnd, if visible { SW_SHOW } else { SW_HIDE });
            let _ = self.controller.SetIsVisible(visible);
        }
    }

    pub fn load_url(&self, url: &str) {
        unsafe {
            let url = HSTRING::from(url);
            let _ = self.webview.Navigate(&url);
        }
    }

    pub fn load_html(&self, html: &str) {
        unsafe {
            let html = HSTRING::from(html);
            let _ = self.webview.NavigateToString(&html);
        }
    }
}

impl Drop for PlatformWebView {
    fn drop(&mut self) {
        unsafe {
            let _ = self.controller.Close();
            let _ = DestroyWindow(self.hwnd);
        }
    }
}
