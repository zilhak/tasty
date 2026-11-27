//! WKWebView 편집 selector 실행.

use objc2::{MainThreadOnly, msg_send};
use objc2_app_kit::NSView;

/// WKWebView의 NSResponder 편집 selector를 실행한다. 대상 frame과 편집 가능 여부는 WebKit이 판단한다.
pub(super) fn run_editing_command(view: &NSView, action: super::super::WebViewEditAction) {
    use super::super::WebViewEditAction as A;
    let selector = match action {
        A::Copy => objc2::sel!(copy:),
        A::Cut => objc2::sel!(cut:),
        A::Paste => objc2::sel!(paste:),
        A::SelectAll => objc2::sel!(selectAll:),
        A::Undo => objc2::sel!(undo:),
        A::Redo => objc2::sel!(redo:),
    };
    // SAFETY: main thread AppKit 뷰에 응답 여부만 묻는다.
    let responds: bool = unsafe { msg_send![view, respondsToSelector: selector] };
    if !responds && matches!(action, A::Undo | A::Redo) {
        // undo:/redo:는 뷰가 아니라 응답자 체인의 undoManager가 처리할 수 있다.
        let app = objc2_app_kit::NSApplication::sharedApplication(view.mtm());
        let null = std::ptr::null::<objc2::runtime::AnyObject>();
        // SAFETY: main thread AppKit. target nil은 first responder부터 응답자 체인을 탄다.
        let handled: bool = unsafe { msg_send![&*app, sendAction: selector, to: null, from: null] };
        if !handled {
            tracing::warn!("webview: edit selector {selector:?} is not handled");
        }
        return;
    }
    if !responds {
        tracing::warn!("webview: edit selector {selector:?} is not handled");
        return;
    }
    // SAFETY: 응답을 확인한 편집 action selector에 sender nil을 넘긴다. 반환값은 쓰지 않는다.
    let _ignored: *mut objc2::runtime::AnyObject = unsafe {
        msg_send![view, performSelector: selector, withObject: std::ptr::null::<objc2::runtime::AnyObject>()]
    };
}
