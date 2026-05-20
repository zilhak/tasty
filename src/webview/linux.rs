//! Linux WebKitGTK wrapper (X11 only).
//! Reference: wry/src/webkitgtk/mod.rs (MIT license, Tauri)
//!
//! Creates an X11 child window inside the parent, then hosts a GTK window
//! with a WebKitGTK WebView inside it.

use gtk::glib::Cast;
use gtk::prelude::*;
use webkit2gtk::{WebView, WebViewExt};
use winit::raw_window_handle::{
    HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle,
};

use super::WebViewBounds;

pub struct PlatformWebView {
    webview: WebView,
    gtk_window: gtk::Window,
    x11_window: std::os::raw::c_ulong,
    xlib: x11_dl::xlib::Xlib,
    x11_display: *mut std::os::raw::c_void,
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
        // SAFETY: XOpenDisplay(null)는 DISPLAY env에서 기본 디스플레이를 연다.
        // 호출 실패 시 null 반환 — 아래 is_null 체크로 처리.
        // PlatformWebView::new는 winit event loop (main thread)에서만 호출되므로
        // Xlib 단일 thread 가정 충족.
        let display = if x11_display_ptr.is_null() {
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

        gtk_window.show_all();

        Ok(Self {
            webview,
            gtk_window,
            x11_window,
            xlib,
            x11_display: display as _,
        })
    }

    pub fn set_bounds(&self, bounds: WebViewBounds, scale_factor: f64) {
        let x = (bounds.x * scale_factor) as i32;
        let y = (bounds.y * scale_factor) as i32;
        let w = (bounds.width * scale_factor) as i32;
        let h = (bounds.height * scale_factor) as i32;

        // SAFETY: self가 살아있으면 x11_display/x11_window 모두 valid (Drop이 정리).
        // 호출은 main thread (winit event loop) 흐름에서만 일어남.
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
        if visible {
            // SAFETY: self valid; main thread.
            unsafe {
                (self.xlib.XMapWindow)(self.x11_display as _, self.x11_window);
                (self.xlib.XFlush)(self.x11_display as _);
            }
            self.gtk_window.show_all();
        } else {
            // SAFETY: self valid; main thread.
            unsafe {
                (self.xlib.XUnmapWindow)(self.x11_display as _, self.x11_window);
                (self.xlib.XFlush)(self.x11_display as _);
            }
            self.gtk_window.hide();
        }
    }

    pub fn load_url(&self, url: &str) {
        self.webview.load_uri(url);
    }

    pub fn load_html(&self, html: &str) {
        self.webview.load_html(html, None);
    }
}

impl Drop for PlatformWebView {
    fn drop(&mut self) {
        // SAFETY: Drop은 self가 마지막으로 살아있는 시점. webview.destroy()와
        // XDestroyWindow는 같은 display 인스턴스에서 한 번씩 호출. 호출은
        // PlatformWebView가 생성된 main thread에서만 일어난다 (!Send 기본).
        unsafe {
            self.webview.destroy();
            (self.xlib.XDestroyWindow)(self.x11_display as _, self.x11_window);
        }
        self.gtk_window.close();
    }
}

// SAFETY: PlatformWebView는 main thread에서만 생성/조작되지만, AppState 보관 목적상
// Send를 요구하는 컨테이너에 넣어야 한다. 실제 thread 이동은 발생하지 않음 (단일 thread
// affinity가 호출 측에서 유지됨). Sync는 의도적으로 추가하지 않는다.
unsafe impl Send for PlatformWebView {}
