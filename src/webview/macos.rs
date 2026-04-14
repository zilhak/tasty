//! macOS WKWebView wrapper.
//! Reference: wry/src/wkwebview/mod.rs (MIT license, Tauri)

use objc2::{MainThreadMarker, MainThreadOnly};
use objc2::rc::Retained;
use objc2_app_kit::NSView;
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString, NSURL};
use objc2_web_kit::{WKWebView, WKWebViewConfiguration};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

use super::WebViewBounds;

pub struct PlatformWebView {
    webview: Retained<WKWebView>,
}

impl PlatformWebView {
    /// Create a WKWebView as a child of the given window, positioned at `bounds`.
    pub fn new(
        window: &impl HasWindowHandle,
        bounds: WebViewBounds,
        scale_factor: f64,
    ) -> Result<Self, String> {
        let mtm = MainThreadMarker::new()
            .ok_or_else(|| "Must be called from main thread".to_string())?;

        let ns_view_ptr = match window.window_handle().map_err(|e| e.to_string())?.as_raw() {
            RawWindowHandle::AppKit(w) => w.ns_view.as_ptr(),
            _ => return Err("Not an AppKit window".to_string()),
        };
        let ns_view: &NSView = unsafe { &*(ns_view_ptr as *const NSView) };

        unsafe {
            let config = WKWebViewConfiguration::new(mtm);

            let frame = logical_to_nsrect(ns_view, bounds, scale_factor);

            let webview = WKWebView::initWithFrame_configuration(
                WKWebView::alloc(mtm),
                frame,
                &config,
            );

            ns_view.addSubview(&webview);

            Ok(Self { webview })
        }
    }

    /// Update the webview position and size.
    pub fn set_bounds(&self, bounds: WebViewBounds, scale_factor: f64) {
        unsafe {
            if let Some(parent) = self.webview.superview() {
                let frame = logical_to_nsrect(&parent, bounds, scale_factor);
                self.webview.setFrame(frame);
            }
        }
    }

    /// Show or hide the webview.
    pub fn set_visible(&self, visible: bool) {
        self.webview.setHidden(!visible);
    }

    /// Navigate to a URL (supports file:// for local files).
    /// For file:// URLs, uses `loadFileURL:allowingReadAccessToURL:` with the
    /// parent directory as the access scope, so relative resources (CSS, JS,
    /// images, iframes) in the same directory tree are accessible.
    pub fn load_url(&self, url: &str) {
        unsafe {
            if let Some(path) = url.strip_prefix("file://") {
                // Use fileURLWithPath for proper percent-encoding of CJK paths
                let file_url = NSURL::fileURLWithPath(&NSString::from_str(path));
                // Allow read access to parent directory for relative resources
                let dir_path = std::path::Path::new(path)
                    .parent()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| "/".to_string());
                let dir_url = NSURL::fileURLWithPath_isDirectory(
                    &NSString::from_str(&dir_path),
                    true,
                );
                self.webview.loadFileURL_allowingReadAccessToURL(&file_url, &dir_url);
            } else {
                let ns_url = NSURL::URLWithString(&NSString::from_str(url));
                if let Some(ns_url) = ns_url {
                    let request = objc2_foundation::NSURLRequest::requestWithURL(&ns_url);
                    self.webview.loadRequest(&request);
                }
            }
        }
    }

    /// Load HTML string directly.
    pub fn load_html(&self, html: &str) {
        unsafe {
            let ns_html = NSString::from_str(html);
            let base_url = NSURL::URLWithString(&NSString::from_str("about:blank"));
            self.webview.loadHTMLString_baseURL(&ns_html, base_url.as_deref());
        }
    }
}

impl Drop for PlatformWebView {
    fn drop(&mut self) {
        self.webview.removeFromSuperview();
    }
}

/// Convert logical bounds (top-left origin) to NSRect,
/// handling macOS coordinate system (bottom-left origin for non-flipped views).
unsafe fn logical_to_nsrect(
    parent: &NSView,
    bounds: WebViewBounds,
    _scale_factor: f64,
) -> NSRect {
    let is_flipped = parent.isFlipped();
    let parent_h = parent.frame().size.height;

    let origin_y = if is_flipped {
        bounds.y
    } else {
        parent_h - bounds.y - bounds.height
    };

    NSRect {
        origin: NSPoint::new(bounds.x, origin_y),
        size: NSSize::new(bounds.width, bounds.height),
    }
}
