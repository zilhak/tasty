//! Linux WebKitGTK wrapper (X11 only).
//! Reference: wry/src/webkitgtk/mod.rs (MIT license, Tauri)
//!
//! Creates an X11 child window inside the parent, then hosts a GTK window
//! with a WebKitGTK WebView inside it.

use gtk::prelude::*;
use webkit2gtk::{WebView, WebViewExt};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

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
        window: &impl HasWindowHandle,
        bounds: WebViewBounds,
        scale_factor: f64,
    ) -> Result<Self, String> {
        let (parent_xid, x11_display_ptr) =
            match window.window_handle().map_err(|e| e.to_string())?.as_raw() {
                RawWindowHandle::Xlib(w) => (
                    w.window,
                    w.display
                        .map(|d| d.as_ptr())
                        .unwrap_or(std::ptr::null_mut()),
                ),
                _ => return Err("Not an X11 window (Wayland is not supported)".to_string()),
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
            unsafe { (xlib.XOpenDisplay)(std::ptr::null()) }
        } else {
            x11_display_ptr as _
        };

        if display.is_null() {
            return Err("Failed to get X11 display".to_string());
        }

        // Create X11 child window
        let x11_window = unsafe {
            (xlib.XCreateSimpleWindow)(display, parent_xid as _, x, y, w.max(1), h.max(1), 0, 0, 0)
        };

        if x11_window == 0 {
            return Err("XCreateSimpleWindow failed".to_string());
        }

        unsafe {
            (xlib.XMapWindow)(display, x11_window);
            (xlib.XFlush)(display);
        }

        // Create GDK window from X11 window
        let gdk_display = gtk::gdk::Display::default().ok_or("No GDK display")?;

        let gdk_window = unsafe {
            use gdkx11::ffi::gdk_x11_window_foreign_new_for_display;
            use gtk::glib::translate::{ToGlibPtr, from_glib_full};
            let raw_display = gdkx11::X11Display::from(gdk_display.clone());
            let gdk_win: gtk::gdk::Window = from_glib_full(gdk_x11_window_foreign_new_for_display(
                raw_display.to_glib_none().0 as _,
                x11_window,
            ));
            gdk_win
        };

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
            unsafe {
                (self.xlib.XMapWindow)(self.x11_display as _, self.x11_window);
                (self.xlib.XFlush)(self.x11_display as _);
            }
            self.gtk_window.show_all();
        } else {
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
        unsafe {
            self.webview.destroy();
            (self.xlib.XDestroyWindow)(self.x11_display as _, self.x11_window);
        }
        self.gtk_window.close();
    }
}

// Safety: WebView is managed on the main thread only
unsafe impl Send for PlatformWebView {}
