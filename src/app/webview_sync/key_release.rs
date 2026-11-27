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
/// 패스의 닫기와 plugin 덮음 갱신까지 반영한, 다음 입력이 볼 포커스 노드를 읽는다(`webview_key_holder`). 돌려준 키
/// 쥘 곳을 동기화가 native 편집 키 다리에 게시한다(`WebViewKeyBridge::set_key_holder`). `held` 의 surface 는 회수
/// 후보여도 이번에는 회수하지 않는다(`release_after_native_focus_notices` 의 보류). 돌려준 둘째 값은 보류한 회수
/// 후보가 있었는지다.
pub(crate) fn release_webview_key_focus<W: KeyFocusWebView>(
    state: &mut MainViewState,
    core: &crate::core::CoreState,
    overlay_mode: OverlayMode,
    overlay_open: bool,
    webviews: &[(u32, &W)],
    held: &std::collections::BTreeSet<u32>,
) -> (Option<u32>, bool) {
    let holder = webview_key_holder(state, core);
    let revealed = !(overlay_open && overlay_mode == OverlayMode::Hide);
    state
        .webview_key_released
        .retain(|sid| Some(*sid) != holder && webviews.iter().any(|(s, _)| s == sid));
    let mut held_any = false;
    for &(sid, wv) in webviews {
        if (holder != Some(sid) || !revealed) && wv.holds_keyboard_focus() {
            if held.contains(&sid) {
                held_any = true;
                continue;
            }
            wv.release_keyboard_focus();
            state.webview_key_released.insert(sid);
        }
    }
    (holder, held_any)
}

/// 프레임 동기화의 회수 판정. 대기열 앞의 아직 소비하지 않은 native 누름 알림을 알림 소비와 같은 전이
/// (`webview_keys::consume_native_focus_requests`, binding 이 맞지 않는 surface 의 알림은 `bound` 가 거른다)로 먼저
/// 소비한 뒤 회수를 판정한다. 그 뒤에도 남은 누름은 모두 전달 키 뒤에 있어 지금 적용할 수 없다(앞 키가 탭·덮개·포커스를
/// 바꿀 수 있다). 그 누름의 surface 만 회수를 보류하고(`WebViewKeyBridge::pending_press_surfaces`) 나머지는 그대로
/// 회수한다. 이번 프레임에 숨는 WebView(모드가 `Hide` 이고 overlay 가 열림)는 보류하지 않는다. 보류한 후보가 있으면
/// 재판정 의무(`MainViewState::webview_release_due`)를 남기고, 없으면 이 판정이 의무를 지운다. 의무는 대기열이 비면
/// 동기화 한 번으로 정산한다(`release_retry_due`).
/// Windows 는 winit 이 누름 메시지와 다시 그리기를 한 회차 안에서 차례로 처리하므로, 누름 알림 뒤 회차 끝 소비
/// 전에 동기화가 올 수 있다. 그 동기화가 앞 포커스 노드로 판정하면 방금 누른 WebView 의 OS 키 포커스를 회수해
/// 뒤따르는 글자 키가 그 페이지에 가지 않는다. 돌려준 값은 키 쥘 곳과, 알림으로 포커스가 바뀌었는지다.
pub(crate) fn release_after_native_focus_notices<W: KeyFocusWebView>(
    state: &mut MainViewState,
    core: &crate::core::CoreState,
    bridge: &crate::webview::WebViewKeyBridge,
    bound: &dyn Fn(u32) -> bool,
    overlay_mode: OverlayMode,
    overlay_open: bool,
    webviews: &[(u32, &W)],
) -> (Option<u32>, bool) {
    let changed =
        crate::app::webview_keys::consume_native_focus_requests(state, core, bridge, bound);
    let hidden = overlay_open && overlay_mode == OverlayMode::Hide;
    let held = if hidden {
        std::collections::BTreeSet::new()
    } else {
        bridge.pending_press_surfaces()
    };
    let (holder, held_any) =
        release_webview_key_focus(state, core, overlay_mode, overlay_open, webviews, &held);
    state.webview_release_due = held_any;
    (holder, changed)
}

/// 미룬 회수를 다시 판정할 때인지. 의무가 있고 대기열을 다 소비했으면(소비, 그 view 를 지운 `retire_view`, 동기화의
/// 선두 알림 소비 어느 쪽으로 비었든) 의무를 지우고 참이다. 호출자는 그 창의 프레임 동기화를 한 번 예약한다(외부 다시
/// 그리기를 기다리지 않는다). 대기열이 비기 전에 동기화가 회수를 적용했으면 그 판정이 이미 의무를 지웠다.
pub(crate) fn release_retry_due(
    state: &mut MainViewState,
    bridge: &crate::webview::WebViewKeyBridge,
) -> bool {
    if !state.webview_release_due || bridge.has_focus_requests() {
        return false;
    }
    state.webview_release_due = false;
    true
}

/// 다음 native 입력이 볼 키 쥘 곳(`webview_key_holder`)을 창의 다리에 게시한다. native 콜백(Windows 메시지 훅, macOS
/// 키 진입점, Linux 키 신호)은 host 상태를 읽지 못하고 게시값으로 편집 키를 판정하므로, 콜백이 다음에 읽기 전에 키
/// 쥘 곳을 바꿀 수 있는 자리마다 부른다: 창 이벤트 처리 뒤(`App::publish_webview_key_holders`), 루프 끝 소비 전,
/// 누름 알림 전이 뒤(`webview_keys::press_and_publish`), 전달 키 처리 뒤(`webview_keys::dispatch_and_apply`), 프레임
/// 동기화. 자리 목록은 원본 가드(`webview_key_holder_publish_sites`)가 고정한다.
pub(crate) fn publish_key_holder(
    state: &mut MainViewState,
    core: &crate::core::CoreState,
    bridge: &crate::webview::WebViewKeyBridge,
) {
    bridge.set_key_holder(webview_key_holder(state, core));
    // 키 쥘 곳과 같은 정규화 뒤 상태로 대기 알림의 결과표를 함께 게시한다(N1).
    // TODO: S6c 입력 순서 경계 설계가 대체할 임시 장치.
    bridge.set_press_forecast(state.press_forecast(core));
}

/// 다음 입력이 볼 키 쥘 곳. 포커스를 정규화한 뒤 포커스 노드와 복원 안내 안쪽 키 포커스로 대응표
/// (`webview_key_holder_of`)를 읽는다. 프레임 동기화(`release_webview_key_focus`)와 native 포커스 알림 소비
/// (`webview_keys::consume_native_focus_requests`)가 이 값 하나를 다리에 게시한다.
pub(crate) fn webview_key_holder(
    state: &mut MainViewState,
    core: &crate::core::CoreState,
) -> Option<u32> {
    state.normalize_focus(core);
    webview_key_holder_of(
        state.focus_node(core),
        crate::adapters::ui::restore_notice::has_keyboard_focus(&state.popups),
    )
}

/// native 포커스 알림(사용자가 그 native 페이지를 누름)이 그 surface 의 회수 기록을 지운다. 누름이 포커스 노드를 그
/// surface 로 옮기지 못하면 다음 동기화가 다시 회수한다.
pub(crate) fn note_native_webview_focus(state: &mut MainViewState, sid: u32) {
    state.webview_key_released.remove(&sid);
}
