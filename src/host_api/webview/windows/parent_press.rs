//! host 입력 경로를 쓰지 못한 view의 누름 알림. 옛 WebView2 runtime에 `AllowHostInputProcessing`이 없거나 옵션 적용이
//! 실패한 view가 대상이다. 메시지 훅이 없으므로 다른 프로세스 Chromium 자식 창의 왼쪽 버튼 누름이 TASTY_WEBVIEW 창에
//! `WM_PARENTNOTIFY`로 오는 것을 받아 그 surface 의 누름 알림을 낸다(`click_gesture::note_parent_notify`). controller
//! `GotFocus`는 이미 포커스를 가진 controller를 다시 누르면 오지 않으므로 누름 알림을 대신하지 못한다.

use std::cell::RefCell;
use std::rc::Rc;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{DefWindowProcW, WM_PARENTNOTIFY};

use crate::host_api::webview::click_gesture::note_parent_notify;
use crate::host_api::webview::keys::WebViewKeySink;

struct Entry {
    root: HWND,
    surface_id: u32,
    sink: Rc<dyn WebViewKeySink>,
}

thread_local! {
    static ENTRIES: RefCell<Vec<Entry>> = const { RefCell::new(Vec::new()) };
}

/// 등록 하나. 놓으면 그 창의 등록을 지운다.
pub(super) struct ParentPressGuard {
    root: HWND,
}

impl Drop for ParentPressGuard {
    fn drop(&mut self) {
        let root = self.root;
        ENTRIES.with(|entries| entries.borrow_mut().retain(|e| e.root != root));
    }
}

/// `root`(TASTY_WEBVIEW 자식 창)가 받은 자식 누름 알림을 이 surface 의 누름으로 알리게 등록한다.
pub(super) fn register(
    root: HWND,
    surface_id: u32,
    sink: Rc<dyn WebViewKeySink>,
) -> ParentPressGuard {
    ENTRIES.with(|entries| {
        let mut entries = entries.borrow_mut();
        entries.retain(|e| e.root != root);
        entries.push(Entry {
            root,
            surface_id,
            sink,
        });
    });
    ParentPressGuard { root }
}

/// TASTY_WEBVIEW 창 클래스의 창 프로시저. 등록한 창의 `WM_PARENTNOTIFY`만 보고 나머지는 기본 처리에 넘긴다.
pub(super) unsafe extern "system" fn webview_window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_PARENTNOTIFY {
        let target = ENTRIES.with(|entries| {
            entries
                .borrow()
                .iter()
                .find(|e| e.root == hwnd)
                .map(|e| (e.surface_id, e.sink.clone()))
        });
        if let Some((surface_id, sink)) = target {
            note_parent_notify(sink.as_ref(), surface_id, (wparam.0 & 0xffff) as u32);
        }
    }
    // SAFETY: 받은 인자를 그대로 기본 처리에 넘긴다.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}
