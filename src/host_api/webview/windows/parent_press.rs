//! TASTY_WEBVIEW 창 클래스의 창 프로시저. 누름 알림 등록부와 분배는 `webview::parent_press`에 있다.

use std::rc::Rc;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;

use crate::host_api::webview::keys::WebViewKeySink;
pub(super) use crate::host_api::webview::parent_press::ParentPressGuard;

/// `root`(TASTY_WEBVIEW 자식 창)가 받은 자식 누름 알림을 이 surface 의 누름으로 알리게 등록한다.
pub(super) fn register(
    root: HWND,
    surface_id: u32,
    sink: Rc<dyn WebViewKeySink>,
) -> ParentPressGuard {
    crate::host_api::webview::parent_press::register(root.0 as isize, surface_id, sink)
}

/// 등록한 창의 `WM_PARENTNOTIFY`를 분배하고 모든 메시지를 기본 처리에 넘긴다.
pub(super) unsafe extern "system" fn webview_window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    crate::host_api::webview::parent_press::dispatch(hwnd.0 as isize, msg, wparam.0);
    // SAFETY: 받은 인자를 그대로 기본 처리에 넘긴다.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}
