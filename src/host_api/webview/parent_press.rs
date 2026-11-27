//! host 입력 경로를 쓰지 못한 view의 누름 알림 등록부(Windows). 옛 WebView2 runtime에 `AllowHostInputProcessing`이
//! 없거나 옵션 적용이 실패한 view가 대상이다. 메시지 훅이 없으므로 다른 프로세스 Chromium 자식 창의 왼쪽 버튼 누름이
//! TASTY_WEBVIEW 창에 `WM_PARENTNOTIFY`로 오는 것을 받아 그 surface 의 누름 알림을 낸다
//! (`click_gesture::note_parent_notify`). controller `GotFocus`는 이미 포커스를 가진 controller를 다시 누르면 오지
//! 않으므로 누름 알림을 대신하지 못한다.
//!
//! 창 프로시저(`windows/parent_press.rs`)는 창 핸들을 정수로 넘겨 이 분배만 부른다. 그래서 등록과 분배가 Windows 밖
//! 시험에서도 컴파일되고, 시험이 분배 → 키 판정 → 대기열 소비를 한 장면으로 돈다.

use std::cell::RefCell;
use std::rc::Rc;

use crate::host_api::webview::click_gesture::note_parent_notify;
use crate::host_api::webview::keys::WebViewKeySink;

/// `WM_PARENTNOTIFY`.
pub(crate) const WM_PARENTNOTIFY: u32 = 0x0210;

struct Entry {
    root: isize,
    surface_id: u32,
    sink: Rc<dyn WebViewKeySink>,
}

thread_local! {
    static ENTRIES: RefCell<Vec<Entry>> = const { RefCell::new(Vec::new()) };
}

/// 등록 하나. 놓으면 그 창의 등록을 지운다.
pub(crate) struct ParentPressGuard {
    root: isize,
}

impl Drop for ParentPressGuard {
    fn drop(&mut self) {
        let root = self.root;
        ENTRIES.with(|entries| entries.borrow_mut().retain(|e| e.root != root));
    }
}

/// `root`(TASTY_WEBVIEW 자식 창 핸들)가 받은 자식 누름 알림을 이 surface 의 누름으로 알리게 등록한다.
pub(crate) fn register(
    root: isize,
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

/// 창 `root`가 받은 메시지 하나를 분배한다. 등록한 창의 `WM_PARENTNOTIFY`만 본다. 기본 처리는 창 프로시저가 한다.
pub(crate) fn dispatch(root: isize, msg: u32, wparam: usize) {
    if msg != WM_PARENTNOTIFY {
        return;
    }
    let target = ENTRIES.with(|entries| {
        (entries.borrow().iter())
            .find(|e| e.root == root)
            .map(|e| (e.surface_id, e.sink.clone()))
    });
    if let Some((surface_id, sink)) = target {
        note_parent_notify(sink.as_ref(), surface_id, (wparam & 0xffff) as u32);
    }
}
