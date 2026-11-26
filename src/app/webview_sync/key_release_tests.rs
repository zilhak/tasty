//! T-W2-0: W2 회수 판정(`release_webview_key_focus`)의 표 시험. 지금 입력에서 지금 결과(창 단위 한 번 회수)를 낸다.

use super::{KeyFocusWebView, release_webview_key_focus};
use std::cell::Cell;

#[derive(Default)]
struct FakeWebView {
    holds: Cell<bool>,
    released: Cell<u32>,
}

impl KeyFocusWebView for FakeWebView {
    fn holds_keyboard_focus(&self) -> bool {
        self.holds.get()
    }

    fn release_keyboard_focus(&self) {
        self.released.set(self.released.get() + 1);
        self.holds.set(false);
    }
}

fn webview(holds: bool) -> FakeWebView {
    let wv = FakeWebView::default();
    wv.holds.set(holds);
    wv
}

/// overlay 가 닫혀 있으면 회수하지 않고 표지를 지운다.
#[test]
fn a_closed_overlay_releases_nothing_and_clears_the_mark() {
    let a = webview(true);
    assert!(!release_webview_key_focus(false, true, true, &[&a]));
    assert_eq!(a.released.get(), 0);
}

/// overlay 구간 처음에 창이 OS 포커스를 가지면 모든 WebView 에 회수를 부르고 표지를 세운다.
#[test]
fn the_first_sync_of_an_overlay_releases_every_webview() {
    let (a, b) = (webview(true), webview(false));
    assert!(release_webview_key_focus(true, false, true, &[&a, &b]));
    assert_eq!((a.released.get(), b.released.get()), (1, 1));
}

/// 표지가 선 구간에서는 다시 부르지 않는다.
#[test]
fn a_released_overlay_does_not_release_again() {
    let a = webview(true);
    assert!(release_webview_key_focus(true, true, true, &[&a]));
    assert_eq!(a.released.get(), 0);
}

/// 창이 winit 포커스를 잃었어도 WebView 가 OS 포커스를 쥐었으면 그 창의 포커스로 보고 회수한다. 아무도 쥐지 않았으면
/// 회수하지 않고 표지도 세우지 않는다.
#[test]
fn an_unfocused_window_releases_only_while_a_webview_holds_the_os_focus() {
    let a = webview(true);
    assert!(release_webview_key_focus(true, false, false, &[&a]));
    assert_eq!(a.released.get(), 1);
    let b = webview(false);
    assert!(!release_webview_key_focus(true, false, false, &[&b]));
    assert_eq!(b.released.get(), 0);
}
