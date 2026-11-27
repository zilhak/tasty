//! N1: 키 판정 다리가 대기열에 남은 누름 알림을 결과표(`MainViewState::press_forecast`)로 적용한 키 쥘 곳
//! (`WebViewKeyBridge::holder_after_pending`)은, 같은 알림을 실제로 소비한 뒤(`consume_native_focus_requests`)의 키 쥘
//! 곳(`key_release::webview_key_holder`)과 같다. 장면은 좌우 두 surface A(11, 선택)·B(12)의 실제 엔진이다.

use winit::keyboard::{Key, ModifiersState, NativeKeyCode, PhysicalKey};

use super::tests::two_surfaces;
use crate::adapters::ui::popup::{PopupCloseReason, PopupScope, PopupState};
use crate::host_api::webview::keys::{
    HostShortcutPolicy, ShortcutSources, WebViewEditAction, WebViewKeyBridge, WebViewKeySink,
};
use crate::state::MainViewState;

const A: u32 = 11;
const B: u32 = 12;
/// 엔진에 없는 surface(무효 알림).
const GONE: u32 = 99;

#[derive(Clone, Copy)]
enum N {
    Click(u32),
    Focus(u32),
}

/// Ctrl+V 붙여넣기 바인딩 다리에 지금 상태의 키 쥘 곳과 결과표를 게시한다.
fn published(state: &mut MainViewState, core: &crate::core::CoreState) -> WebViewKeyBridge {
    let bridge = WebViewKeyBridge::new();
    bridge.set_policy(HostShortcutPolicy::new(ShortcutSources {
        edit: vec![(WebViewEditAction::Paste, vec!["ctrl+v".to_string()])],
        ..Default::default()
    }));
    crate::app::webview_sync::key_release::publish_key_holder(state, core, &bridge);
    bridge
}

fn notify(bridge: &WebViewKeyBridge, notices: &[N]) {
    for n in notices {
        match *n {
            N::Click(sid) => bridge.note_press(sid),
            N::Focus(sid) => bridge.note_focus(sid),
        }
    }
}

/// 알림을 넣은 뒤 다리의 예측과, 그 알림을 실제로 소비한 뒤의 키 쥘 곳을 돌려준다. 둘이 다르면 실패한다.
fn agree(state: &mut MainViewState, core: &crate::core::CoreState, notices: &[N]) -> Option<u32> {
    let bridge = published(state, core);
    notify(&bridge, notices);
    let forecast = bridge.holder_after_pending();
    crate::app::webview_keys::consume_native_focus_requests(state, core, &bridge);
    let real = crate::app::webview_sync::key_release::webview_key_holder(state, core);
    assert_eq!(forecast, real, "예측과 실제 소비 뒤 키 쥘 곳이 다르다");
    forecast
}

fn paste_on(bridge: &WebViewKeyBridge, sid: u32) -> bool {
    let unknown = PhysicalKey::Unidentified(NativeKeyCode::Unidentified);
    let v = Key::Character("v".into());
    !bridge.blocks_edit_key(sid, &v, &unknown, ModifiersState::CONTROL)
        && bridge.edit_action(sid, &v, &unknown, ModifiersState::CONTROL)
            == Some(WebViewEditAction::Paste)
}

/// 덮개 정의를 `scope` 에 연다.
fn open_cover(
    state: &mut MainViewState,
    scope: PopupScope,
) -> crate::adapters::ui::popup::PopupInstanceId {
    const COVER: crate::adapters::ui::popup::PopupId = "notice_holder_cover";
    state.popups.register(PopupState::new(
        COVER,
        "cover".to_string(),
        egui::vec2(120.0, 60.0),
        scope.clone(),
    ));
    let mut request = crate::intent::OpenRequest::new(COVER, scope).focused();
    request.covers_scope = Some(true);
    state
        .popups
        .open_request(&request, true)
        .expect("덮개가 열린다")
}

/// X1·r2: Given A 가 키 쥘 곳이다, When B 누름 알림이 대기열에 있는 채(어느 펌프도 소비하지 않음) B·A 의 Ctrl+V 가
/// native 콜백으로 오면, Then B 의 Ctrl+V 는 B 에서 실행되고 A 의 것은 막힌다. Linux 메뉴 종료 펌프·다른 view 를 지울
/// 때의 펌프처럼 대기열을 건마다 소비하지 않는 GTK 펌프의 [B 누름, Ctrl+V] 도 이 판정이다. 대조: 알림이 없으면 A 다.
#[test]
fn a_linux_edit_key_after_a_press_in_one_drained_pump_runs_in_the_pressed_page() {
    let (mut state, mut session) = two_surfaces();
    let engine = session.borrow_mut();
    let bridge = published(&mut state, engine.core);
    assert!(paste_on(&bridge, A) && !paste_on(&bridge, B), "대조");
    bridge.note_press(B);
    assert!(paste_on(&bridge, B), "누른 B 의 붙여넣기가 막혔다");
    assert!(!paste_on(&bridge, A), "누르기 전 A 의 붙여넣기가 실행된다");
    bridge.set_modal_open(true);
    assert!(
        !paste_on(&bridge, B),
        "OS 모달은 대기 알림과 관계없이 막는다"
    );
}

/// 알림 순서: 같은 surface 의 누름·포커스 획득은 어느 순서든 그 surface, 뒤 surface 의 포커스 획득은 그 surface, 뒤의
/// 무효 누름은 앞 값을 지킨다.
#[test]
fn pending_notices_apply_in_arrival_order() {
    for (notices, want) in [
        (&[N::Click(B), N::Focus(B)][..], Some(B)),
        (&[N::Focus(B), N::Click(B)][..], Some(B)),
        (&[N::Click(B), N::Focus(A)][..], Some(A)),
        (&[N::Click(B), N::Click(GONE)][..], Some(B)),
        (&[N::Focus(B)][..], Some(B)),
    ] {
        let (mut state, mut session) = two_surfaces();
        let engine = session.borrow_mut();
        assert_eq!(agree(&mut state, engine.core, notices), want);
    }
}

/// 덮개: B 만 덮는 덮개가 포커스일 때 B 누름은 덮개가 가진다(키 쥘 곳 없음, 선택은 A). 그 뒤 A 포커스 획득은 선택이
/// 바뀌지 않아 덮개가 남고, A 누름은 A 다. B 포커스 획득도 덮개를 넘지 못한다. 창 전체 덮개는 어느 누름도 surface 에 주지 않는다. 덮개를 연 뒤·닫은 뒤의 게시가 결과표를 바꾼다.
#[test]
fn a_covered_press_goes_to_its_cover() {
    for (notices, want) in [
        (&[N::Click(B)][..], None),
        (&[N::Click(B), N::Focus(A)][..], None),
        (&[N::Click(B), N::Click(A)][..], Some(A)),
        (&[N::Focus(B)][..], None),
    ] {
        let (mut state, mut session) = two_surfaces();
        let engine = session.borrow_mut();
        open_cover(&mut state, PopupScope::Surface(B));
        state.normalize_focus(engine.core);
        assert_eq!(agree(&mut state, engine.core, notices), want);
    }
    let (mut state, mut session) = two_surfaces();
    let engine = session.borrow_mut();
    let cover = open_cover(&mut state, PopupScope::Window);
    for notices in [&[N::Click(B)][..], &[N::Click(A)][..], &[N::Focus(B)][..]] {
        assert_eq!(agree(&mut state, engine.core, notices), None);
    }
    state
        .popups
        .close_instance(cover, PopupCloseReason::Command);
    assert_eq!(agree(&mut state, engine.core, &[N::Click(B)]), Some(B));
}

/// 포커스 획득은 팝업·주소 띠를 놓지 않는다: A 범위 검색 바가 포커스면 A 포커스 획득은 검색 바를 남기고(키 쥘 곳 없음)
/// B 포커스 획득은 선택을 옮겨 경로 밖 검색 바를 푼다. 주소 띠 편집 중의 포커스 획득은 주소 띠를 남기고, 누름은 놓는다.
#[test]
fn a_focus_notice_keeps_the_node_it_does_not_release() {
    use crate::adapters::ui::search_bar::SEARCH_BAR_POPUP_ID;
    for (notices, want) in [
        (&[N::Focus(A)][..], None),
        (&[N::Focus(B)][..], Some(B)),
        (&[N::Click(A)][..], Some(A)),
    ] {
        let (mut state, mut session) = two_surfaces();
        let engine = session.borrow_mut();
        state
            .popups
            .open_request(
                &crate::intent::OpenRequest::new(SEARCH_BAR_POPUP_ID, PopupScope::Surface(A))
                    .focused(),
                true,
            )
            .expect("검색 바가 열린다");
        assert_eq!(agree(&mut state, engine.core, notices), want);
    }
    for (notices, want) in [(&[N::Focus(B)][..], None), (&[N::Click(B)][..], Some(B))] {
        let (mut state, mut session) = two_surfaces();
        let engine = session.borrow_mut();
        state.html_address.set_focused_for_test(A);
        assert_eq!(agree(&mut state, engine.core, notices), want);
    }
}

/// 창 전체를 막는 노드(설정 열기 요청, 키를 가진 복원 안내)가 있으면 어느 알림도 surface 에 키를 주지 않는다.
#[test]
fn a_window_blocking_node_keeps_every_notice_off_the_pages() {
    for blocker in 0..2 {
        let (mut state, mut session) = two_surfaces();
        let engine = session.borrow_mut();
        if blocker == 0 {
            state.settings_open_requested = true;
        } else {
            for def in crate::adapters::ui::popup::defs::all_defs() {
                state.popups.register_def(def, 1.0);
            }
            assert!(crate::adapters::ui::restore_notice::push(
                &mut state,
                crate::adapters::ui::restore_notice::RestoreNoticeEntry {
                    id: 1,
                    title: "title".into(),
                    cause: "cause".into(),
                    label: "label".into(),
                    path: "/a".into(),
                    body: "body".into(),
                },
            ));
            crate::adapters::ui::restore_notice::move_inside_focus(
                &egui::Context::default(),
                &mut state.popups,
                1,
            );
        }
        for notices in [&[N::Click(B)][..], &[N::Focus(B)][..]] {
            assert_eq!(
                agree(&mut state, engine.core, notices),
                None,
                "막는 노드 {blocker}"
            );
        }
    }
}
