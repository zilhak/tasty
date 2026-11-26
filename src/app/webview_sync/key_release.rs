//! W2: WebView 는 키 쥘 곳(`key_focus::webview_key_holder_of`)이 그 surface 이고 이번 프레임에 드러날 때만 OS 키
//! 포커스를 쥘 수 있다. 동기화(`sync_webviews`)가 W1 모드 결정과 표시 술어 관측 뒤, 표시 적용 전에 이 판정 하나를
//! 부른다. backend 는 자기 자식이 실제로 포커스를 가졌을 때만 회수한다.

use crate::state::MainViewState;
use crate::state::key_focus::webview_key_holder_of;
use crate::state::webview_overlay::OverlayMode;

/// OS 키 포커스를 쥘 수 있는 WebView. 창의 `PlatformWebView` 와 시험용 WebView 가 구현한다.
pub(crate) trait KeyFocusWebView {
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

/// 키 쥘 곳이 아닌 WebView, 또는 이번 프레임에 숨는(모드가 `Hide` 이고 overlay 가 열림) 키 쥘 곳 WebView 가 OS 키
/// 포커스를 쥐면 회수하고 그 surface 의 회수 기록(`MainViewState::webview_key_released`)을 세운다. OS 키 포커스를 쥔
/// WebView 가 있으면 이 창이 OS 포커스를 가진 것이라(`host_window_has_os_focus`) 다른 앱의 포커스를 바꾸지 않는다.
/// 기록은 재회수를 막지 않는다(매 동기화의 `holds_keyboard_focus` 관측이 정한다). 기록은 그 surface 가 키 쥘 곳이
/// 되거나 사라지거나 native 포커스 알림이 오면(`note_native_webview_focus`) 지운다. 입구에서 포커스를 정규화해 그
/// 패스의 닫기와 plugin 덮음 갱신까지 반영한, 다음 입력이 볼 포커스 노드를 읽는다. 돌려준 키 쥘 곳을 동기화가
/// native 편집 키 다리에 게시한다(`WebViewKeyBridge::set_key_holder`).
pub(crate) fn release_webview_key_focus<W: KeyFocusWebView>(
    state: &mut MainViewState,
    core: &crate::core::CoreState,
    overlay_mode: OverlayMode,
    overlay_open: bool,
    webviews: &[(u32, &W)],
) -> Option<u32> {
    state.normalize_focus(core);
    let holder = webview_key_holder_of(
        state.focus_node(core),
        crate::adapters::ui::restore_notice::has_keyboard_focus(&state.popups),
    );
    let revealed = !(overlay_open && overlay_mode == OverlayMode::Hide);
    state
        .webview_key_released
        .retain(|sid| Some(*sid) != holder && webviews.iter().any(|(s, _)| s == sid));
    for &(sid, wv) in webviews {
        if (holder != Some(sid) || !revealed) && wv.holds_keyboard_focus() {
            wv.release_keyboard_focus();
            state.webview_key_released.insert(sid);
        }
    }
    holder
}

/// native 포커스 알림(사용자가 그 native 페이지를 누름)이 그 surface 의 회수 기록을 지운다. 누름이 포커스 노드를 그
/// surface 로 옮기지 못하면 다음 동기화가 다시 회수한다.
pub(crate) fn note_native_webview_focus(state: &mut MainViewState, sid: u32) {
    state.webview_key_released.remove(&sid);
}
