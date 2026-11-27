//! W2(`webview_sync::key_release::release_webview_key_focus`): WebView 는 키 쥘 곳이 그 surface 이고 이번 프레임에
//! 드러날 때만 OS 키 포커스를 지킨다. 장면은 좌우 두 surface(L·R)의 실제 엔진이고 엔진 포커스는 L 이다. WebView 는
//! 시험용이고 overlay 값은 동기화처럼 `has_egui_overlay_open` 이다.

use std::cell::Cell;

use super::repro::open_host_popup;
use super::{PANE_L, SURFACE_L, SURFACE_R, scene};
use crate::adapters::ui::popup::command_palette::COMMAND_PALETTE_POPUP_ID;
use crate::adapters::ui::popup::{PopupId, PopupScope, PopupState, defs};
use crate::app::webview_sync::key_release::{
    KeyFocusWebView, note_native_webview_focus, release_webview_key_focus,
};
use crate::core::CoreState;
use crate::intent::OpenRequest;
use crate::state::RequestContext;
use crate::state::webview_overlay::OverlayMode;

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

fn holding() -> FakeWebView {
    let wv = FakeWebView::default();
    wv.holds.set(true);
    wv
}

fn defs_scene() -> (
    RequestContext,
    crate::runtime::engine_session::EngineSession,
) {
    let (mut state, session) = scene(PANE_L);
    for def in defs::all_defs() {
        state.popups.register_def(def, 1.0);
    }
    (state, session)
}

/// 동기화 한 번. overlay 값은 동기화처럼 지금 상태에서 읽는다.
fn sync(
    state: &mut RequestContext,
    core: &CoreState,
    mode: OverlayMode,
    wvs: &[(u32, &FakeWebView)],
) {
    let overlay_open = state.has_egui_overlay_open();
    release_webview_key_focus(state, core, mode, overlay_open, wvs, &Default::default());
}

/// 창 범위 덮개 정의 `id` 를 등록하고 연다.
fn open_window_cover(state: &mut RequestContext, id: PopupId, focused: bool) {
    let mut p = PopupState::new(
        id,
        "cover".to_string(),
        egui::vec2(120.0, 60.0),
        PopupScope::Window,
    );
    p.look = p.look.covering(true);
    p.declared_look = p.look;
    state.popups.register(p);
    let request = OpenRequest::new(id, PopupScope::Window).centered();
    let request = if focused { request.focused() } else { request };
    state
        .popups
        .open_request(&request, focused)
        .expect("덮개가 열린다");
}

fn notice(state: &mut RequestContext) {
    assert!(crate::adapters::ui::restore_notice::push(
        state,
        crate::adapters::ui::restore_notice::RestoreNoticeEntry {
            id: 1,
            title: "title".into(),
            cause: "cause".into(),
            label: "label".into(),
            path: "/a".into(),
            body: "body".into(),
        },
    ));
}

/// T-W2-1: Given 창 범위 팝업(명령 팔레트)이 포커스 노드일 때, When 동기화하면, Then OS 키 포커스를 쥔 모든 WebView
/// 에서 회수한다.
#[test]
fn a_window_popup_focus_node_releases_every_webview() {
    let (mut state, mut session) = defs_scene();
    state
        .popups
        .open_request(
            &OpenRequest::new(COMMAND_PALETTE_POPUP_ID, PopupScope::Window)
                .centered()
                .focused(),
            true,
        )
        .expect("팔레트가 열린다");
    let (l, r) = (holding(), holding());
    let engine = session.borrow_mut();
    sync(
        &mut state,
        &engine.read(),
        OverlayMode::Opaque,
        &[(SURFACE_L, &l), (SURFACE_R, &r)],
    );
    assert_eq!((l.released.get(), r.released.get()), (1, 1));
}

/// T-W2-2: Given R 의 Surface 범위 팝업이 포커스 노드이고 L WebView 가 OS 키 포커스를 쥘 때, When 동기화하면, Then L
/// 에서 회수한다.
#[test]
fn a_surface_popup_focus_node_in_r_releases_the_webview_in_l() {
    let (mut state, mut session) = defs_scene();
    open_host_popup(&mut state, SURFACE_R, true);
    let l = holding();
    let engine = session.borrow_mut();
    sync(
        &mut state,
        &engine.read(),
        OverlayMode::Opaque,
        &[(SURFACE_L, &l)],
    );
    assert_eq!(l.released.get(), 1);
}

/// T-W2-3: Given 포커스를 가져오지 않은 plugin 팝업이 열려 있고 모드가 `Hide` 가 아니며 L 이 포커스 노드일 때, When
/// 동기화하면, Then L WebView 는 OS 키 포커스를 지킨다.
#[test]
fn an_open_passive_plugin_popup_keeps_the_focused_webview() {
    let (mut state, mut session) = defs_scene();
    state.plugin_popup_open = true;
    let l = holding();
    let engine = session.borrow_mut();
    sync(
        &mut state,
        &engine.read(),
        OverlayMode::Opaque,
        &[(SURFACE_L, &l)],
    );
    assert_eq!(l.released.get(), 0);
    assert!(l.holds.get());
}

/// T-W2-4: Given 안쪽 컨트롤이 키를 갖지 않은 복원 안내가 떠 있을 때, When 동기화하면, Then 포커스 노드 L 의
/// WebView 는 지킨다.
#[test]
fn an_unfocused_restore_notice_keeps_the_focused_webview() {
    let (mut state, mut session) = defs_scene();
    notice(&mut state);
    let l = holding();
    let engine = session.borrow_mut();
    sync(
        &mut state,
        &engine.read(),
        OverlayMode::Hide,
        &[(SURFACE_L, &l)],
    );
    assert_eq!(l.released.get(), 0);
}

/// T-W2-5: Given 복원 안내 안쪽 컨트롤이 키를 가질 때, When 동기화하면, Then 키 쥘 곳이 없어 L WebView 에서
/// 회수한다.
#[test]
fn a_restore_notice_holding_keys_releases_the_webview() {
    let (mut state, mut session) = defs_scene();
    notice(&mut state);
    crate::adapters::ui::restore_notice::move_inside_focus(
        &egui::Context::default(),
        &mut state.popups,
        1,
    );
    let l = holding();
    let engine = session.borrow_mut();
    sync(
        &mut state,
        &engine.read(),
        OverlayMode::Opaque,
        &[(SURFACE_L, &l)],
    );
    assert_eq!(l.released.get(), 1);
}

/// T-W2-6: Given 다른 W1 항 없이 L 을 덮는 passive host 팝업 하나만 열렸을 때, When 동기화하면, Then 정규화로 그
/// 덮개가 포커스 노드라 L WebView 에서 회수한다.
#[test]
fn a_lone_passive_host_cover_over_the_focused_html_surface_releases_it() {
    let (mut state, mut session) = defs_scene();
    let cover = open_host_popup(&mut state, SURFACE_L, false);
    assert!(state.popups.instance_is_visible_passive(cover));
    let l = holding();
    let engine = session.borrow_mut();
    sync(
        &mut state,
        &engine.read(),
        OverlayMode::Opaque,
        &[(SURFACE_L, &l)],
    );
    assert_eq!(l.released.get(), 1);
}

/// T-W2-7: Given L 이 포커스 노드이고 다른 곳의 포커스 없는 팝업으로 overlay 가 열렸으며 모드가 `Hide` 일 때, When
/// 동기화하면, Then L 은 이번 프레임에 숨으므로 회수한다.
#[test]
fn a_hide_mode_overlay_releases_the_focused_webview() {
    let (mut state, mut session) = defs_scene();
    open_host_popup(&mut state, SURFACE_R, true);
    state.popups.set_focus(None);
    let l = holding();
    let engine = session.borrow_mut();
    sync(
        &mut state,
        &engine.read(),
        OverlayMode::Hide,
        &[(SURFACE_L, &l)],
    );
    assert_eq!(l.released.get(), 1);
}

/// T-W2-8: Given 포커스 노드가 창 범위 팝업이라 L 에서 한 번 회수했을 때, When 키 쥘 곳과 기록이 그대로인 채 native
/// 포커스 알림 없이 L 이 다시 OS 포커스를 얻고 동기화하면, Then 다시 회수한다.
#[test]
fn a_webview_that_regains_os_focus_while_not_the_holder_is_released_again() {
    let (mut state, mut session) = defs_scene();
    open_window_cover(&mut state, "w2_cover_top", true);
    let l = holding();
    let engine = session.borrow_mut();
    sync(
        &mut state,
        &engine.read(),
        OverlayMode::Opaque,
        &[(SURFACE_L, &l)],
    );
    l.holds.set(true);
    sync(
        &mut state,
        &engine.read(),
        OverlayMode::Opaque,
        &[(SURFACE_L, &l)],
    );
    assert_eq!(l.released.get(), 2);
}

/// T-W2-9: Given 창 범위 팝업이 포커스 노드인 구간에서 L 을 회수했을 때, When 같은 구간에 R WebView 가 OS 키 포커스를
/// 쥔 채 나타나 동기화하면, Then R 도 회수한다.
#[test]
fn a_webview_appearing_later_in_the_same_overlay_is_released() {
    let (mut state, mut session) = defs_scene();
    open_window_cover(&mut state, "w2_cover_top", true);
    let (l, r) = (holding(), holding());
    let engine = session.borrow_mut();
    sync(
        &mut state,
        &engine.read(),
        OverlayMode::Opaque,
        &[(SURFACE_L, &l)],
    );
    sync(
        &mut state,
        &engine.read(),
        OverlayMode::Opaque,
        &[(SURFACE_L, &l), (SURFACE_R, &r)],
    );
    assert_eq!((l.released.get(), r.released.get()), (1, 1));
}

/// T-W2-10: Given R 의 비-passive host 팝업이 열린 채 포커스만 잃고 L 이 포커스 노드이며 모드가 `Hide` 가 아닐 때,
/// When 동기화하면, Then L WebView 는 지킨다.
#[test]
fn an_unfocused_host_popup_elsewhere_keeps_the_focused_webview() {
    let (mut state, mut session) = defs_scene();
    open_host_popup(&mut state, SURFACE_R, true);
    state.popups.set_focus(None);
    let l = holding();
    let engine = session.borrow_mut();
    sync(
        &mut state,
        &engine.read(),
        OverlayMode::Opaque,
        &[(SURFACE_L, &l)],
    );
    assert_eq!(l.released.get(), 0);
}

/// T-W2-11: Given R pane 의 포커스 잃은 이름 바꾸기 팝업(탭 이름)이 열려 있고 L 이 포커스 노드이며 모드가 `Hide` 가 아닐 때, When
/// 동기화하면, Then L WebView 는 지킨다.
#[test]
fn an_unfocused_rename_popup_keeps_the_focused_webview() {
    let (mut state, mut session) = defs_scene();
    let rename = crate::adapters::ui::rename_popup::RENAME_POPUP_ID;
    state
        .popups
        .open_request(
            &OpenRequest::new(rename, PopupScope::Pane(super::PANE_R))
                .centered()
                .focused(),
            true,
        )
        .expect("이름 바꾸기가 열린다");
    state.popups.set_focus(None);
    assert!(state.popups.is_open(rename));
    let l = holding();
    let engine = session.borrow_mut();
    sync(
        &mut state,
        &engine.read(),
        OverlayMode::Opaque,
        &[(SURFACE_L, &l)],
    );
    assert_eq!(l.released.get(), 0);
}

/// T-W2-12: Given 창 범위 덮개가 포커스 노드라 L 을 회수해 기록을 세웠을 때, When L 의 native 포커스 알림이 기록을
/// 지우고(덮여 포커스 노드는 그대로) L 이 다시 OS 포커스를 얻어 동기화하면, Then 다시 회수한다.
#[test]
fn a_native_focus_notice_clears_the_record_and_the_next_sync_releases_again() {
    let (mut state, mut session) = defs_scene();
    open_window_cover(&mut state, "w2_cover_top", true);
    let l = holding();
    let engine = session.borrow_mut();
    sync(
        &mut state,
        &engine.read(),
        OverlayMode::Opaque,
        &[(SURFACE_L, &l)],
    );
    assert!(state.webview_key_released.contains(&SURFACE_L));
    note_native_webview_focus(&mut state, SURFACE_L);
    assert!(!state.webview_key_released.contains(&SURFACE_L));
    l.holds.set(true);
    sync(
        &mut state,
        &engine.read(),
        OverlayMode::Opaque,
        &[(SURFACE_L, &l)],
    );
    assert_eq!(l.released.get(), 2);
}

/// T-W2-13: Given 부모·자식이 아닌 창 범위 덮개 둘 가운데 아래는 passive, 위는 포커스 팝업일 때, When 그 패스에 위
/// 팝업이 닫히고 첫 동기화가 오면, Then 정규화로 passive 덮개가 포커스 노드라 L WebView 에서 회수한다.
#[test]
fn closing_the_top_window_cover_releases_the_webview_under_a_passive_cover_in_the_same_pass() {
    let (mut state, mut session) = defs_scene();
    open_window_cover(&mut state, "w2_cover_below", false);
    open_window_cover(&mut state, "w2_cover_top", true);
    state.popups.close("w2_cover_top");
    assert_eq!(state.popups.focus(), None, "닫기가 관리자 칸을 비운다");
    let l = holding();
    let engine = session.borrow_mut();
    sync(
        &mut state,
        &engine.read(),
        OverlayMode::Opaque,
        &[(SURFACE_L, &l)],
    );
    assert_eq!(l.released.get(), 1);
}

/// 기록은 키 쥘 곳이 된 surface 와 사라진 surface 에서 지운다.
#[test]
fn the_record_clears_for_the_holder_and_for_a_gone_surface() {
    let (mut state, mut session) = defs_scene();
    let engine = session.borrow_mut();
    state
        .webview_key_released
        .extend([SURFACE_L, SURFACE_R, 999]);
    let (l, r) = (FakeWebView::default(), FakeWebView::default());
    sync(
        &mut state,
        &engine.read(),
        OverlayMode::Opaque,
        &[(SURFACE_L, &l), (SURFACE_R, &r)],
    );
    assert_eq!(
        state
            .webview_key_released
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        [SURFACE_R]
    );
}

/// T-W2-14: Given L 이 포커스 노드이고 사용자가 R 페이지를 막 눌러 R WebView 가 OS 키 포커스를 쥐었으며 그 누름
/// 알림이 아직 소비되지 않았을 때(Windows 는 누름 메시지와 다시 그리기가 한 회차 안에 차례로 온다), When 프레임
/// 동기화가 회수를 판정하면, Then 알림을 먼저 소비해 R 이 포커스 노드가 되고 R WebView 는 키 포커스를 지킨다. 대조:
/// 알림이 없으면 같은 동기화가 R 에서 회수한다.
#[test]
fn a_frame_sync_consumes_a_pending_press_notice_before_it_releases_the_pressed_webview() {
    use crate::app::webview_sync::key_release::release_after_native_focus_notices;
    for (notice, released, holder) in [(true, 0, Some(SURFACE_R)), (false, 1, Some(SURFACE_L))] {
        let (mut state, mut session) = defs_scene();
        let bridge = crate::webview::WebViewKeyBridge::default();
        bridge.set_key_holder(Some(SURFACE_L));
        if notice {
            crate::host_api::webview::keys::WebViewKeySink::note_focus(&bridge, SURFACE_R);
        }
        let r = holding();
        let engine = session.borrow_mut();
        let overlay_open = state.has_egui_overlay_open();
        let (got, moved) = release_after_native_focus_notices(
            &mut state,
            &engine.read(),
            &bridge,
            &|_| true,
            OverlayMode::Opaque,
            overlay_open,
            &[(SURFACE_R, &r)],
        );
        assert_eq!(r.released.get(), released, "notice={notice}");
        assert_eq!(got, holder, "notice={notice}");
        assert_eq!(moved, notice, "notice={notice}");
        assert!(!bridge.has_focus_requests(), "알림은 소비됐다");
    }
}

/// X5: Given L 이 포커스 노드이고 R WebView 가 OS 키 포커스를 쥐었을 때, When 프레임 동기화가 회수를 판정하면, Then
/// 대기열에 회수 결과를 바꾸지 않는 키(L 이 넘긴 host 조합)만 남았으면 R 을 바로 회수한다. 키 뒤에 R 누름이 남았으면
/// 회수를 미루고 재판정 의무를 남긴다. 회차 끝 소비가 R 누름까지 적용해 대기열이 비면 의무가 한 번 다시 판정을
/// 예약하고(외부 다시 그리기 없이), 그 동기화는 R 을 키 쥘 곳으로 두고 회수하지 않는다.
#[test]
fn a_skipped_webview_release_retries_when_native_input_drains() {
    use crate::app::webview_sync::key_release::{
        release_after_native_focus_notices, release_retry_due,
    };
    use crate::host_api::webview::keys::{HostShortcutPolicy, ShortcutSources, WebViewKeySink};
    use winit::keyboard::{Key, ModifiersState, NativeKeyCode, PhysicalKey};
    for press_behind in [false, true] {
        let (mut state, mut session) = defs_scene();
        let bridge = crate::webview::WebViewKeyBridge::default();
        bridge.set_policy(HostShortcutPolicy::new(ShortcutSources {
            host: vec!["ctrl+j".to_string()],
            ..Default::default()
        }));
        bridge.set_key_holder(Some(SURFACE_L));
        let forwarded = bridge.forward_key(
            SURFACE_L,
            Key::Character("j".into()),
            PhysicalKey::Unidentified(NativeKeyCode::Unidentified),
            ModifiersState::CONTROL,
        );
        assert!(forwarded.is_some());
        if press_behind {
            bridge.note_press(SURFACE_R);
        }
        let r = holding();
        let engine = session.borrow_mut();
        let sync_once = |state: &mut RequestContext| {
            let overlay_open = state.has_egui_overlay_open();
            release_after_native_focus_notices(
                state,
                &engine.read(),
                &bridge,
                &|_| true,
                OverlayMode::Opaque,
                overlay_open,
                &[(SURFACE_R, &r)],
            )
            .0
        };
        let got = sync_once(&mut state);
        if !press_behind {
            assert_eq!(r.released.get(), 1, "키만 남았으면 바로 회수한다");
            assert_eq!(got, Some(SURFACE_L));
            assert!(!release_retry_due(&mut state, &bridge), "의무가 없다");
            continue;
        }
        assert_eq!(r.released.get(), 0, "R 누름이 남은 동안 회수하지 않는다");
        assert!(
            !release_retry_due(&mut state, &bridge),
            "대기열이 남은 동안은 다시 판정하지 않는다"
        );
        // 회차 끝 소비: 전달 키를 처리하고 R 누름을 적용한다.
        while let Some(input) = bridge.pop_input() {
            if let crate::webview::NativeInput::Press { surface_id, kind } = input {
                crate::app::webview_keys::press_and_publish(
                    &mut state,
                    &engine.read(),
                    &bridge,
                    surface_id,
                    kind,
                    &|_| true,
                );
            }
        }
        assert!(
            release_retry_due(&mut state, &bridge),
            "비면 한 번 예약한다"
        );
        assert!(!release_retry_due(&mut state, &bridge), "예약은 한 번이다");
        assert_eq!(sync_once(&mut state), Some(SURFACE_R));
        assert_eq!(r.released.get(), 0, "R 은 키 쥘 곳이라 회수하지 않는다");
    }
}

/// P4 대기열: L 이 키 쥘 곳인 다리에 [L 의 host 키(Ctrl+J), `press` 의 누름] 을 넣는다. 누름은 키 뒤라 동기화가 먼저
/// 소비하지 못한다.
fn key_then_press(press: u32) -> crate::webview::WebViewKeyBridge {
    use crate::host_api::webview::keys::WebViewKeySink;
    key_then(|bridge| bridge.note_press(press))
}

/// `key_then_press` 와 같되 키 뒤 알림을 `notices` 가 넣는다.
fn key_then(
    notices: impl FnOnce(&crate::webview::WebViewKeyBridge),
) -> crate::webview::WebViewKeyBridge {
    use crate::host_api::webview::keys::{HostShortcutPolicy, ShortcutSources, WebViewKeySink};
    use winit::keyboard::{Key, ModifiersState, NativeKeyCode, PhysicalKey};
    let bridge = crate::webview::WebViewKeyBridge::default();
    bridge.set_policy(HostShortcutPolicy::new(ShortcutSources {
        host: vec!["ctrl+j".to_string()],
        ..Default::default()
    }));
    bridge.set_key_holder(Some(SURFACE_L));
    let forwarded = bridge.forward_key(
        SURFACE_L,
        Key::Character("j".into()),
        PhysicalKey::Unidentified(NativeKeyCode::Unidentified),
        ModifiersState::CONTROL,
    );
    assert!(forwarded.is_some(), "키가 대기열에 든다");
    notices(&bridge);
    bridge
}

/// 동기화의 회수 판정 한 번(`release_after_native_focus_notices`).
fn sync_with_notices(
    state: &mut RequestContext,
    core: &CoreState,
    bridge: &crate::webview::WebViewKeyBridge,
    mode: OverlayMode,
    wvs: &[(u32, &FakeWebView)],
) {
    let overlay_open = state.has_egui_overlay_open();
    crate::app::webview_sync::key_release::release_after_native_focus_notices(
        state,
        core,
        bridge,
        &|_| true,
        mode,
        overlay_open,
        wvs,
    );
}

/// P4 sid 별 보류: Given L 이 포커스 노드이고 R WebView 가 OS 키 포커스를 쥔다, When 키 뒤에 남은 누름이 L 의
/// 것이면, Then R 은 그 누름과 무관해 바로 회수하고 의무도 없다. 대조: 남은 누름이 R 의 것이면 R 만 보류하고 의무를
/// 남긴다(지금 R 이 덮이지 않아 그 누름이 R 을 키 쥘 곳으로 만든다).
#[test]
fn a_pending_press_holds_only_the_release_of_its_own_webview() {
    use crate::app::webview_sync::key_release::release_retry_due;
    for (press, released) in [(SURFACE_L, 1), (SURFACE_R, 0)] {
        let (mut state, mut session) = defs_scene();
        let bridge = key_then_press(press);
        let r = holding();
        let engine = session.borrow_mut();
        sync_with_notices(
            &mut state,
            &engine.read(),
            &bridge,
            OverlayMode::Opaque,
            &[(SURFACE_R, &r)],
        );
        assert_eq!(r.released.get(), released, "press={press}");
        assert_eq!(
            state.webview_release_due,
            released == 0,
            "press={press}: 보류한 후보가 있을 때만 의무가 남는다"
        );
        bridge.take_inputs();
        assert_eq!(release_retry_due(&mut state, &bridge), released == 0);
    }
}

/// P4 Hide: Given 키 뒤에 R 누름이 남았고 창의 egui overlay 가 열렸다(덮개 없음), When 모드가 Hide 라 R 이 이번
/// 프레임에 숨으면, Then 보류하지 않고 회수한다(숨는 WebView 는 키를 받을 수 없다). 대조: 모드가 Opaque 면 R 을
/// 보류한다.
#[test]
fn a_webview_hidden_by_the_overlay_is_released_despite_its_pending_press() {
    for (mode, released) in [(OverlayMode::Hide, 1), (OverlayMode::Opaque, 0)] {
        let (mut state, mut session) = defs_scene();
        let bridge = key_then_press(SURFACE_R);
        let r = holding();
        let engine = session.borrow_mut();
        crate::app::webview_sync::key_release::release_after_native_focus_notices(
            &mut state,
            &engine.read(),
            &bridge,
            &|_| true,
            mode,
            true,
            &[(SURFACE_R, &r)],
        );
        assert_eq!(r.released.get(), released, "{mode:?}");
        assert_eq!(state.webview_release_due, released == 0, "{mode:?}");
    }
}

/// 보류는 남은 알림이 키 쥘 곳을 그 surface 로 만들 때만 한다(`WebViewKeyBridge::pending_holders`). Given 창 범위
/// 팔레트(덮개)가 포커스 노드이고 키 뒤에 R 누름이 남았다, When 모드가 Opaque·Hide 어느 쪽이든, Then R 누름은 덮개가
/// 가져 R 을 키 쥘 곳으로 만들지 못하므로 R 을 바로 회수하고 의무를 남기지 않는다. 앞 키가 덮개를 닫으면 그 누름은
/// 소비될 때 R 을 키 쥘 곳으로 만들 수 있지만, 이미 회수한 OS 키 포커스는 돌려주지 않는다. binding 이 맞지 않은 R 의
/// 누름도 회수한다. 대조: 덮개가 없고 binding 이 맞으면 R 을 보류한다.
#[test]
fn a_pending_press_that_cannot_hold_does_not_keep_the_release() {
    for (covered, bound, released) in [(true, true, 1), (false, false, 1), (false, true, 0)] {
        for mode in [OverlayMode::Opaque, OverlayMode::Hide] {
            let (mut state, mut session) = defs_scene();
            let bridge = key_then_press(SURFACE_R);
            if covered {
                state
                    .popups
                    .open_request(
                        &OpenRequest::new(COMMAND_PALETTE_POPUP_ID, PopupScope::Window)
                            .centered()
                            .focused(),
                        true,
                    )
                    .expect("팔레트가 열린다");
            }
            let r = holding();
            let engine = session.borrow_mut();
            crate::app::webview_sync::key_release::release_after_native_focus_notices(
                &mut state,
                &engine.read(),
                &bridge,
                &|sid| bound || sid != SURFACE_R,
                mode,
                false,
                &[(SURFACE_R, &r)],
            );
            let label = format!("덮개 {covered} binding {bound} {mode:?}");
            assert_eq!(r.released.get(), released, "{label}");
            assert_eq!(state.webview_release_due, released == 0, "{label}");
        }
    }
}

/// 보류는 남은 알림이 소비될 때 키 쥘 곳을 그 surface 로 만드는지로 고른다. Given L 의 주소 띠가 키를 쥐고 R WebView 가
/// OS 키 포커스를 쥔다, When 키 뒤에 R 포커스 획득만 남았으면, Then 포커스 획득은 주소 띠를 놓지 못해 R 을 키 쥘 곳으로
/// 만들지 못하므로 R 을 바로 회수하고 의무를 남기지 않는다. 대조: 키 뒤에 L 본문 누름이 먼저 있어 주소 띠를 놓으면 뒤
/// R 포커스 획득이 R 을 키 쥘 곳으로 만들어 R 회수를 보류한다.
#[test]
fn a_pending_focus_that_cannot_take_the_keys_does_not_keep_the_release() {
    use crate::host_api::webview::keys::WebViewKeySink;
    for (body_press, released) in [(false, 1), (true, 0)] {
        let (mut state, mut session) = defs_scene();
        state.html_address.set_focused_for_test(SURFACE_L);
        let bridge = key_then(|bridge| {
            if body_press {
                bridge.note_press(SURFACE_L);
            }
            bridge.note_focus(SURFACE_R);
        });
        let r = holding();
        let engine = session.borrow_mut();
        sync_with_notices(
            &mut state,
            &engine.read(),
            &bridge,
            OverlayMode::Opaque,
            &[(SURFACE_R, &r)],
        );
        let label = format!("L 본문 누름 {body_press}");
        assert_eq!(r.released.get(), released, "{label}");
        assert_eq!(state.webview_release_due, released == 0, "{label}");
    }
}

/// 보류 후보는 알림을 차례로 적용한 선택으로 고른다. Given 선택은 L 이고 R 범위 검색 바(덮지 않는 팝업)가 포커스이며
/// L WebView 가 OS 키 포커스를 쥔다, When 키 뒤에 [R 포커스 획득, L 포커스 획득] 이 남았으면, Then R 포커스 획득이
/// 검색 바를 남긴 채 선택을 R 로 옮기고 L 포커스 획득이 선택을 L 로 되돌려 검색 바를 풀므로 L 회수를 보류한다.
/// 대조: R 포커스 획득만, 또는 L 포커스 획득만(선택이 그대로라 검색 바가 남는다)이면 L 을 바로 회수한다.
#[test]
fn a_focus_back_after_a_focus_that_kept_the_popup_keeps_the_release() {
    use crate::adapters::ui::search_bar::SEARCH_BAR_POPUP_ID;
    use crate::host_api::webview::keys::WebViewKeySink;
    for (notices, released) in [
        (&[SURFACE_R, SURFACE_L][..], 0),
        (&[SURFACE_R][..], 1),
        (&[SURFACE_L][..], 1),
    ] {
        let (mut state, mut session) = defs_scene();
        state
            .popups
            .open_request(
                &OpenRequest::new(SEARCH_BAR_POPUP_ID, PopupScope::Surface(SURFACE_R)).focused(),
                true,
            )
            .expect("검색 바가 열린다");
        let bridge = key_then(|bridge| {
            for &sid in notices {
                bridge.note_focus(sid);
            }
        });
        let l = holding();
        let engine = session.borrow_mut();
        sync_with_notices(
            &mut state,
            &engine.read(),
            &bridge,
            OverlayMode::Opaque,
            &[(SURFACE_L, &l)],
        );
        let label = format!("포커스 획득 {notices:?}");
        assert_eq!(l.released.get(), released, "{label}");
        assert_eq!(state.webview_release_due, released == 0, "{label}");
    }
}

/// P4 정산: Given 키 뒤 R 누름 때문에 R 회수를 보류했다, When (a) 회차 끝 소비가 키만 처리한 뒤 R 의 view 를 지워
/// (`retire_view`) 대기열이 비면, Then 의무가 한 번 다시 판정을 예약한다. When (b) 키만 처리된 뒤 다음 동기화가 남은
/// R 누름을 선두에서 직접 소비하고 회수를 판정하면, Then 그 판정이 의무를 지워 대기열이 비어도 예약은 0번이다.
#[test]
fn a_held_release_settles_where_its_queue_empties_or_its_sync_applies() {
    use crate::app::webview_sync::key_release::release_retry_due;
    for retired in [true, false] {
        let (mut state, mut session) = defs_scene();
        let bridge = key_then_press(SURFACE_R);
        let r = holding();
        let engine = session.borrow_mut();
        let wvs = [(SURFACE_R, &r)];
        sync_with_notices(
            &mut state,
            &engine.read(),
            &bridge,
            OverlayMode::Opaque,
            &wvs,
        );
        assert!(state.webview_release_due, "R 회수를 보류했다");
        assert!(
            matches!(
                bridge.pop_input(),
                Some(crate::webview::NativeInput::Forwarded(_))
            ),
            "회차 끝 소비가 키를 처리했다"
        );
        if retired {
            bridge.retire_view(SURFACE_R);
            assert!(
                release_retry_due(&mut state, &bridge),
                "비면 한 번 예약한다"
            );
            assert!(!release_retry_due(&mut state, &bridge), "예약은 한 번이다");
            continue;
        }
        sync_with_notices(
            &mut state,
            &engine.read(),
            &bridge,
            OverlayMode::Opaque,
            &wvs,
        );
        assert!(!bridge.has_focus_requests(), "동기화가 R 누름을 소비했다");
        assert_eq!(r.released.get(), 0, "R 은 키 쥘 곳이 됐다");
        assert!(
            !release_retry_due(&mut state, &bridge),
            "회수를 적용한 동기화 뒤 예약은 0번이다"
        );
    }
}
