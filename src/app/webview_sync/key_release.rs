//! W2: overlay 가 열린 동안 WebView 의 OS 키 포커스를 호스트 창으로 돌리는 판정. 동기화(`sync_webviews`)가 이 함수
//! 하나를 부른다. backend 는 자기 자식이 실제로 포커스를 가졌을 때만 회수한다.

use crate::view::main::redraw::host_window_has_os_focus;

/// OS 키 포커스를 쥘 수 있는 WebView. 창의 `PlatformWebView` 와 시험용 WebView 가 구현한다.
pub(super) trait KeyFocusWebView {
    fn holds_keyboard_focus(&self) -> bool;
    fn release_keyboard_focus(&self);
}

impl KeyFocusWebView for crate::webview::PlatformWebView {
    fn holds_keyboard_focus(&self) -> bool {
        crate::webview::PlatformWebView::holds_keyboard_focus(self)
    }

    fn release_keyboard_focus(&self) {
        crate::webview::PlatformWebView::release_keyboard_focus(self)
    }
}

/// overlay 를 열면 WebView 키보드 포커스를 호스트 창으로 돌린다. 숨기기만으로는 포커스가 해제되지 않는다. 닫을 때
/// 자동으로 native 자식에 포커스를 돌려주지는 않는다. 다른 앱의 포커스를 바꾸지 않도록 이 창이 OS 포커스를 가질 때만
/// 처리한다(`host_window_has_os_focus`). overlay 구간 처음에 한 번 회수하고 그 구간의 창 표지(`released`)를 세운다.
/// overlay 가 닫히면 표지를 지운다. 돌려준 값이 새 표지다.
pub(super) fn release_webview_key_focus<W: KeyFocusWebView>(
    overlay_open: bool,
    released: bool,
    window_focused: bool,
    webviews: &[&W],
) -> bool {
    if !overlay_open {
        return false;
    }
    if released
        || !host_window_has_os_focus(window_focused, || {
            webviews.iter().any(|wv| wv.holds_keyboard_focus())
        })
    {
        return released;
    }
    for wv in webviews {
        wv.release_keyboard_focus();
    }
    true
}

#[cfg(test)]
#[path = "key_release_tests.rs"]
mod tests;
