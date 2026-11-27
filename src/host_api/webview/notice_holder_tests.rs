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

#[derive(Clone, Copy, Debug)]
enum N {
    Click(u32),
    Focus(u32),
}

/// Ctrl+V 붙여넣기 바인딩 다리에 지금 상태의 키 쥘 곳과 결과표를 게시한다. 모든 surface 의 binding 이 맞다.
fn published(state: &mut MainViewState, core: &crate::core::CoreState) -> WebViewKeyBridge {
    published_bound(state, core, &|_| true)
}

/// `published` 와 같되 binding 판정 `bound` 를 게시에 넘긴다.
fn published_bound(
    state: &mut MainViewState,
    core: &crate::core::CoreState,
    bound: &dyn Fn(u32) -> bool,
) -> WebViewKeyBridge {
    let bridge = WebViewKeyBridge::new();
    bridge.set_policy(HostShortcutPolicy::new(ShortcutSources {
        edit: vec![(WebViewEditAction::Paste, vec!["ctrl+v".to_string()])],
        ..Default::default()
    }));
    crate::app::webview_sync::key_release::publish_key_holder(state, core, &bridge, bound);
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

/// 알림을 넣은 뒤 다리의 예측과, 그 알림을 실제로 소비한 뒤의 키 쥘 곳을 돌려준다. 둘이 다르면 실패한다. 게시와
/// 소비는 같은 binding 판정 `bound` 를 받는다.
fn agree(
    state: &mut MainViewState,
    core: &crate::core::CoreState,
    notices: &[N],
    bound: &dyn Fn(u32) -> bool,
) -> Option<u32> {
    let bridge = published_bound(state, core, bound);
    notify(&bridge, notices);
    let forecast = bridge.holder_after_pending();
    crate::app::webview_keys::consume_native_focus_requests(state, core, &bridge, bound);
    let real = crate::app::webview_sync::key_release::webview_key_holder(state, core)
        .filter(|&sid| bound(sid));
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

/// GTK 대기 이벤트를 건마다 소비하지 않고 비우는 펌프(native 메뉴 poll, 맞지 않은 view 의 Drop)는 그리기가
/// 덮개를 바꾼 뒤이자 프레임 동기화 게시 전에 돈다. Given 앞 게시가 창 덮개를 담았고 그 덮개가 닫혔다, When 펌프 안에서
/// [B 누름, Ctrl+V] 가 오면, Then B 의 붙여넣기는 닫힌 뒤 표로 열린다. 거꾸로 앞 게시 뒤 창 덮개가 열렸으면 펌프 안의
/// [B 누름, Ctrl+V] 는 막힌다. 대조: 각 장면의 앞 게시값은 반대로 판정한다.
#[test]
fn a_gtk_pump_judges_its_edit_keys_with_the_state_before_the_pump() {
    use crate::app::webview_sync::key_release::{Bindings, publish_before_gtk_pump};
    let bound = Bindings::from([A, B]);
    let pressed_paste = |bridge: &WebViewKeyBridge| {
        bridge.note_press(B);
        paste_on(bridge, B)
    };
    let (mut state, mut session) = two_surfaces();
    let engine = session.borrow_mut();
    let cover = open_cover(&mut state, PopupScope::Window);
    let bridge = published(&mut state, engine.core);
    bridge.note_press(B);
    assert!(!paste_on(&bridge, B), "대조: 덮개를 담은 표는 B 를 막는다");
    bridge.retire_view(B);
    state
        .popups
        .close_instance(cover, PopupCloseReason::Command);
    let ran = publish_before_gtk_pump(&mut state, engine.core, &bridge, &bound, || {
        pressed_paste(&bridge)
    });
    assert!(ran, "닫힌 덮개의 표가 펌프 안 B 의 붙여넣기를 막는다");

    let (mut state, mut session) = two_surfaces();
    let engine = session.borrow_mut();
    let bridge = published(&mut state, engine.core);
    bridge.note_press(B);
    assert!(paste_on(&bridge, B), "대조: 덮개 전 표는 B 를 연다");
    bridge.retire_view(B);
    open_cover(&mut state, PopupScope::Window);
    let ran = publish_before_gtk_pump(&mut state, engine.core, &bridge, &bound, || {
        pressed_paste(&bridge)
    });
    assert!(!ran, "새 창 덮개 밑 B 의 붙여넣기가 펌프 안에서 열린다");
}

/// Given binding 이 맞지 않은 view A·B 가 있고 A 의 Drop 이 펌프를 돌며 그 안에서 아직 남은 B 가 누름 알림을 넣는다,
/// When 동기화가 둘을 지우면(`drop_stale_views`), Then 끝난 뒤 대기열에 B 의 항목이 없어 같은 surface 로 새로 만든
/// view 가 옛 B 의 누름을 받지 않는다. 대조: B 가 지울 view 가 아니면 그 누름은 남는다.
#[test]
fn a_press_queued_while_an_earlier_stale_view_drops_is_retired_with_its_view() {
    use crate::app::webview_sync::key_release::{Bindings, drop_stale_views};
    struct Pumping<'a> {
        bridge: &'a WebViewKeyBridge,
        press: Option<u32>,
    }
    impl Drop for Pumping<'_> {
        fn drop(&mut self) {
            if let Some(sid) = self.press {
                self.bridge.note_press(sid);
            }
        }
    }
    for (stale, left) in [(&[A, B][..], false), (&[A][..], true)] {
        let (mut state, mut session) = two_surfaces();
        let engine = session.borrow_mut();
        let bridge = published(&mut state, engine.core);
        let pumping = |press| Pumping {
            bridge: &bridge,
            press,
        };
        let mut views =
            std::collections::HashMap::from([(A, pumping(Some(B))), (B, pumping(None))]);
        drop_stale_views(
            &mut state,
            engine.core,
            &bridge,
            &Bindings::new(),
            &mut views,
            stale,
        );
        assert_eq!(bridge.has_pending_press(), left, "지운 view {stale:?}");
    }
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
        assert_eq!(agree(&mut state, engine.core, notices, &|_| true), want);
    }
}

/// Given A 가 키 쥘 곳이고 B 의 WebView binding 이 지금 surface 와 맞지 않는다(엔진이 B 의 실행 인스턴스를
/// 바꿨고 동기화가 아직 view 를 바꾸지 않음), When 옛 B 의 누름·포커스 획득 알림이 대기열에 있으면, Then 예측은 실제
/// 소비처럼 그 알림을 건너뛰어 A 를 지키고, A 의 붙여넣기가 열리고 B 의 것은 막힌다. 뒤 A 포커스 획득은 그대로 A 다.
/// 대조: binding 이 맞으면 B 다.
#[test]
fn a_press_whose_binding_is_not_current_keeps_the_holder_in_the_forecast() {
    let stale_b = |sid| sid != B;
    for (scene, notices, want_stale, want_bound) in [
        ("[Click B]", &[N::Click(B)][..], Some(A), Some(B)),
        ("[Focus B]", &[N::Focus(B)][..], Some(A), Some(B)),
        (
            "[Click B, Focus A]",
            &[N::Click(B), N::Focus(A)][..],
            Some(A),
            Some(A),
        ),
    ] {
        let (mut state, mut session) = two_surfaces();
        let engine = session.borrow_mut();
        let bridge = published_bound(&mut state, engine.core, &stale_b);
        notify(&bridge, notices);
        assert!(
            paste_on(&bridge, A) && !paste_on(&bridge, B),
            "{scene}: 옛 B 알림이 A 의 붙여넣기를 막았다"
        );
        drop(bridge);
        let (mut state, mut session) = two_surfaces();
        let engine = session.borrow_mut();
        assert_eq!(
            agree(&mut state, engine.core, notices, &stale_b),
            want_stale,
            "{scene}: 예측과 실제 소비 뒤 키 쥘 곳"
        );
        let (mut state, mut session) = two_surfaces();
        let engine = session.borrow_mut();
        assert_eq!(
            agree(&mut state, engine.core, notices, &|_| true),
            want_bound,
            "{scene}: 대조"
        );
    }
}

/// Given 선택된 A 의 binding 이 지금 surface 와 맞지 않는다(엔진이 A 의 실행 인스턴스를 바꿨고 옛 A view 가 남음),
/// When 게시하면, Then 키 쥘 곳은 없고 옛 A 의 붙여넣기가 막힌다. 대기열이 비었을 때와 옛 A 의 누름이 든 때(무효라
/// 건너뜀) 둘 다다. 예측은 실제 소비 뒤 같은 판정의 키 쥘 곳과 같다. 대조: binding 이 맞으면 A 다.
#[test]
fn a_selected_surface_whose_binding_ended_publishes_no_holder() {
    let stale_a = |sid| sid != A;
    for (scene, notices) in [("빈 대기열", &[][..]), ("[Click A]", &[N::Click(A)][..])] {
        let (mut state, mut session) = two_surfaces();
        let engine = session.borrow_mut();
        let bridge = published_bound(&mut state, engine.core, &stale_a);
        notify(&bridge, notices);
        assert_eq!(bridge.holder_after_pending(), None, "{scene}: 게시값");
        assert!(!paste_on(&bridge, A), "{scene}: 옛 A 의 붙여넣기가 열린다");
        drop(bridge);
        let (mut state, mut session) = two_surfaces();
        let engine = session.borrow_mut();
        assert_eq!(
            agree(&mut state, engine.core, notices, &stale_a),
            None,
            "{scene}"
        );
        let (mut state, mut session) = two_surfaces();
        let engine = session.borrow_mut();
        let bridge = published(&mut state, engine.core);
        notify(&bridge, notices);
        assert_eq!(bridge.holder_after_pending(), Some(A), "{scene}: 대조");
        assert!(paste_on(&bridge, A), "{scene}: 대조 붙여넣기");
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
        assert_eq!(agree(&mut state, engine.core, notices, &|_| true), want);
    }
    let (mut state, mut session) = two_surfaces();
    let engine = session.borrow_mut();
    let cover = open_cover(&mut state, PopupScope::Window);
    for notices in [&[N::Click(B)][..], &[N::Click(A)][..], &[N::Focus(B)][..]] {
        assert_eq!(agree(&mut state, engine.core, notices, &|_| true), None);
    }
    state
        .popups
        .close_instance(cover, PopupCloseReason::Command);
    assert_eq!(
        agree(&mut state, engine.core, &[N::Click(B)], &|_| true),
        Some(B)
    );
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
        assert_eq!(agree(&mut state, engine.core, notices, &|_| true), want);
    }
    for (notices, want) in [(&[N::Focus(B)][..], None), (&[N::Click(B)][..], Some(B))] {
        let (mut state, mut session) = two_surfaces();
        let engine = session.borrow_mut();
        state.html_address.set_focused_for_test(A);
        assert_eq!(agree(&mut state, engine.core, notices, &|_| true), want);
    }
}

/// 포커스 획득은 팝업을 놓지 못해도 선택을 옮긴다. Given 선택은 A 이고 B 범위 검색 바(덮지 않는 팝업)가 포커스다,
/// When [B 포커스 획득, A 포커스 획득] 이 오면, Then B 포커스 획득은 검색 바를 남긴 채 선택을 B 로 옮기고, A 포커스
/// 획득은 선택을 A 로 되돌려 경로 밖이 된 검색 바를 풀어 A 가 키 쥘 곳이 된다(예측과 실제 소비가 같다). 대조: B 포커스
/// 획득만이면 검색 바가 남고, A 포커스 획득만이면 선택이 그대로라 검색 바가 남는다.
#[test]
fn a_focus_that_keeps_a_popup_still_moves_the_selection_for_the_next_focus() {
    use crate::adapters::ui::search_bar::SEARCH_BAR_POPUP_ID;
    for (notices, want) in [
        (&[N::Focus(B), N::Focus(A)][..], Some(A)),
        (&[N::Focus(B)][..], None),
        (&[N::Focus(A)][..], None),
        (&[N::Focus(GONE), N::Focus(A)][..], None),
    ] {
        let (mut state, mut session) = two_surfaces();
        let engine = session.borrow_mut();
        state
            .popups
            .open_request(
                &crate::intent::OpenRequest::new(SEARCH_BAR_POPUP_ID, PopupScope::Surface(B))
                    .focused(),
                true,
            )
            .expect("검색 바가 열린다");
        let label = format!("{notices:?}");
        assert_eq!(state.focused_surface_id(engine.core), Some(A), "{label}");
        let bridge = published(&mut state, engine.core);
        assert_eq!(
            bridge.holder_after_pending(),
            None,
            "{label}: 검색 바가 키를 쥔다"
        );
        assert_eq!(
            agree(&mut state, engine.core, notices, &|_| true),
            want,
            "{label}"
        );
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
                agree(&mut state, engine.core, notices, &|_| true),
                None,
                "막는 노드 {blocker}"
            );
        }
    }
}

/// 창을 덮은 노드 밑에서는 창 범위의 어느 것도 바뀌지 않는다. Given 전체화면 무대가 열렸다, When 쌓여 있던
/// B 누름·포커스 획득 알림을 소비하면, Then 밑 탭의 선택(A), A 주소 띠 편집, A 범위 검색 바 포커스가 각각 그대로다.
/// 대조: 무대가 없으면 같은 알림이 선택을 B 로 옮기고 주소 띠·검색 바를 놓는다.
#[test]
fn a_window_blocking_stage_keeps_queued_notices_from_the_tab_below() {
    use crate::adapters::ui::search_bar::SEARCH_BAR_POPUP_ID;
    for staged in [true, false] {
        for notices in [&[N::Click(B)][..], &[N::Focus(B)][..]] {
            for part in ["선택", "주소 띠", "팝업 포커스"] {
                let (mut state, mut session) = two_surfaces();
                let engine = session.borrow_mut();
                match part {
                    "주소 띠" => state.html_address.set_focused_for_test(A),
                    "팝업 포커스" => {
                        state
                            .popups
                            .open_request(
                                &crate::intent::OpenRequest::new(
                                    SEARCH_BAR_POPUP_ID,
                                    PopupScope::Surface(A),
                                )
                                .focused(),
                                true,
                            )
                            .expect("검색 바가 열린다");
                    }
                    _ => {}
                }
                if staged {
                    assert!(state.open_fullscreen_stage("blank"));
                }
                let focus = state.popups.focus();
                let bridge = published(&mut state, engine.core);
                notify(&bridge, notices);
                crate::app::webview_keys::consume_native_focus_requests(
                    &mut state,
                    engine.core,
                    &bridge,
                    &|_| true,
                );
                let kept = match part {
                    "선택" => state.focused_surface_id(engine.core) == Some(A),
                    "주소 띠" => state.html_address.has_keyboard_focus_on(A),
                    _ => state.popups.focus() == focus,
                };
                // 포커스 획득은 주소 띠를 놓지 않는다(`a_focus_notice_keeps_the_node_it_does_not_release`).
                let focus_keeps_address = matches!(notices[0], N::Focus(_)) && part == "주소 띠";
                assert_eq!(
                    kept,
                    staged || focus_keeps_address,
                    "무대 {staged} {part} {:?}",
                    matches!(notices[0], N::Click(_))
                );
            }
        }
    }
}

/// L1: Given A 가 키 쥘 곳일 때 A 가 Ctrl+V 를 받아 판정을 통과했다(Linux 는 확장 응답 때 다시 묻는다), When 응답 전에
/// 포커스 이탈 없이 키 쥘 곳만 B 로 옮겨 게시되면(host 쪽 포커스 이동), Then 응답 재검사는 A 의 붙여넣기를 버리지 않는다.
/// 받은 키는 이미 A 의 입력 순서에 들었다. 대조: 같은 자리에서 받을 때 판정(`blocks_edit_key`)은 A 를 막는다.
#[test]
fn a_linux_edit_reply_after_the_holder_moved_runs_in_the_pressed_page() {
    let (mut state, mut session) = two_surfaces();
    let engine = session.borrow_mut();
    let bridge = published(&mut state, engine.core);
    assert!(paste_on(&bridge, A));
    state.focus_surface_by_id(engine.core, B);
    crate::app::webview_sync::key_release::publish_key_holder(
        &mut state,
        engine.core,
        &bridge,
        &|_| true,
    );
    assert!(
        !bridge.edit_reply_blocked(A),
        "키 쥘 곳만 옮겼는데 응답을 버린다"
    );
    assert!(!paste_on(&bridge, A), "대조: 받을 때 판정은 A 를 막는다");
}

/// L1 대조: 응답 전에 A 경로를 덮는 덮개가 열리거나, 창 전체를 막는 노드가 생기거나, OS 모달이 열리면 응답을 버린다.
#[test]
fn a_linux_edit_reply_under_a_new_cover_is_dropped() {
    let (mut state, mut session) = two_surfaces();
    let engine = session.borrow_mut();
    let bridge = published(&mut state, engine.core);
    assert!(!bridge.edit_reply_blocked(A), "대조: 막는 것이 없다");
    open_cover(&mut state, PopupScope::Surface(A));
    crate::app::webview_sync::key_release::publish_key_holder(
        &mut state,
        engine.core,
        &bridge,
        &|_| true,
    );
    assert!(
        bridge.edit_reply_blocked(A),
        "A 를 덮은 덮개 뒤 응답을 실행한다"
    );
    assert!(
        !bridge.edit_reply_blocked(B),
        "덮이지 않은 B 는 막지 않는다"
    );
    let (mut state, mut session) = two_surfaces();
    let engine = session.borrow_mut();
    let bridge = published(&mut state, engine.core);
    state.settings_open_requested = true;
    crate::app::webview_sync::key_release::publish_key_holder(
        &mut state,
        engine.core,
        &bridge,
        &|_| true,
    );
    assert!(
        bridge.edit_reply_blocked(A),
        "창 전체를 막는 노드 뒤 응답을 실행한다"
    );
    state.settings_open_requested = false;
    crate::app::webview_sync::key_release::publish_key_holder(
        &mut state,
        engine.core,
        &bridge,
        &|_| true,
    );
    assert!(
        !bridge.edit_reply_blocked(A),
        "대조: 요청을 거두면 막지 않는다"
    );
    bridge.set_modal_open(true);
    assert!(bridge.edit_reply_blocked(A), "OS 모달 뒤 응답을 실행한다");
}

/// P1: 창 누름(`take_press_snapshot`)과 native 본문 누름(`press_surface_body`)은 같은 전이(`apply_press_target`)에
/// 이른다. 장면 1: B 를 덮는 passive 덮개 위 누름은 두 입구 모두 덮개를 포커스 위치로 하고 passive 를 풀며 선택은 A 다.
/// 장면 2: 말풍선이 키보드 포커스를 가진 채 본문을 누르면 두 입구 모두 말풍선 포커스를 끈다.
#[test]
fn window_and_native_entries_reach_the_same_press_result() {
    const COVER: crate::adapters::ui::popup::PopupId = "press_entry_cover";
    let scene = |window_entry: bool, covered: bool| {
        let (mut state, mut session) = two_surfaces();
        let engine = session.borrow_mut();
        state.tutorial.keyboard_focus = true;
        let pos = egui::pos2(10.0, 10.0);
        let mut cover = None;
        if covered {
            state.popups.register(PopupState::new(
                COVER,
                "cover".to_string(),
                egui::vec2(120.0, 60.0),
                PopupScope::Surface(B),
            ));
            let mut request = crate::intent::OpenRequest::new(COVER, PopupScope::Surface(B));
            request.covers_scope = Some(true);
            let iid = state
                .popups
                .open_request(&request, false)
                .expect("덮개가 열린다");
            assert!(state.popups.is_passive(COVER), "대조: passive 로 열렸다");
            cover = Some(crate::adapters::ui::popup::focus::PopupNode::Host(iid));
            let z = state.popups.instance(iid).expect("인스턴스").z_seq;
            state.host_popup_covers = vec![crate::adapters::ui::popup::occlusion::Occluder {
                rect: egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, 100.0)),
                z_seq: z,
            }];
        }
        if window_entry {
            state.take_press_snapshot(engine.core, pos, egui::PointerButton::Primary);
        } else {
            state.press_surface_body(engine.core, if covered { B } else { A });
        }
        // 인스턴스 순번은 장면마다 다르므로 포커스 위치는 "그 장면의 덮개인지"로 비교한다.
        let node = match state.focus_node(engine.core) {
            crate::state::focus_node::FocusNode::Popup(p) => {
                format!("popup cover={}", Some(p) == cover)
            }
            other => format!("{other:?}"),
        };
        (
            state.popups.focus().map(|p| Some(p) == cover),
            state.popups.is_passive(COVER),
            node,
            state.focused_surface_id(&engine.read()),
            state.tutorial.keyboard_focus,
        )
    };
    for covered in [true, false] {
        let window = scene(true, covered);
        assert_eq!(scene(false, covered), window, "덮개 {covered}");
        assert!(!window.4, "누름이 말풍선 포커스를 끄지 않았다");
        assert_eq!(window.3, Some(A));
        if covered {
            assert!(
                window.0 == Some(true) && !window.1,
                "덮개가 포커스를 받고 passive 가 풀린다"
            );
        }
    }
}

/// native 누름 전이는 그린 것을 바꾸면 변경을 돌려준다(입구가 그 값으로 redraw 를 요청한다). 장면 1: 정규화로
/// 이미 포커스 위치인 passive 덮개(A 범위, 창 범위)를 덮인 A 의 누름이 승격한다 → 참, passive 해제. 장면 2: 이미
/// 선택된 A 의 말풍선 키보드 포커스를 A 누름이 끈다 → 참. 대조: 두 장면 모두 같은 누름을 한 번 더 하면 거짓이다.
#[test]
fn a_press_that_only_promotes_a_cover_or_drops_the_tutorial_focus_reports_a_change() {
    const COVER: crate::adapters::ui::popup::PopupId = "press_change_cover";
    for scope in [PopupScope::Surface(A), PopupScope::Window] {
        let (mut state, mut session) = two_surfaces();
        let engine = session.borrow_mut();
        state.popups.register(PopupState::new(
            COVER,
            "cover".to_string(),
            egui::vec2(120.0, 60.0),
            scope.clone(),
        ));
        let mut request = crate::intent::OpenRequest::new(COVER, scope.clone());
        request.covers_scope = Some(true);
        let iid = state
            .popups
            .open_request(&request, false)
            .expect("덮개가 열린다");
        state.normalize_focus(engine.core);
        let node = Some(crate::adapters::ui::popup::focus::PopupNode::Host(iid));
        assert!(
            state.popups.focus() == node && state.popups.is_passive(COVER),
            "대조: 정규화가 passive 덮개를 포커스 위치로 했다 {scope:?}"
        );
        assert!(
            state.press_surface_body(engine.core, A),
            "passive 승격을 변경으로 알리지 않는다 {scope:?}"
        );
        assert!(!state.popups.is_passive(COVER), "passive 가 풀리지 않았다");
        assert!(
            !state.press_surface_body(engine.core, A),
            "대조: 두 번째 누름은 바꾸지 않는다 {scope:?}"
        );
    }
    let (mut state, mut session) = two_surfaces();
    let engine = session.borrow_mut();
    state.tutorial.keyboard_focus = true;
    assert!(
        state.press_surface_body(engine.core, A),
        "말풍선 포커스 해제를 변경으로 알리지 않는다"
    );
    assert!(!state.tutorial.keyboard_focus);
    assert!(
        !state.press_surface_body(engine.core, A),
        "대조: 두 번째 누름은 바꾸지 않는다"
    );
}

/// P2: Given B 를 덮는 덮개가 포커스이고 B 의 회수 기록이 있다, When [B 누름(덮개가 가짐), B 포커스 획득]을 소비하면,
/// Then 포커스 획득은 아무것도 바꾸지 않는다: 선택은 A, 포커스 노드는 덮개, B 회수 기록은 남는다. 대조: 덮개가 없으면
/// B 포커스 획득이 선택을 B 로 옮기고 기록을 지운다.
#[test]
fn a_focus_after_a_covered_click_changes_nothing() {
    for covered in [true, false] {
        let (mut state, mut session) = two_surfaces();
        let engine = session.borrow_mut();
        let cover = covered.then(|| open_cover(&mut state, PopupScope::Surface(B)));
        state.normalize_focus(engine.core);
        let notices: &[N] = if covered { &[N::Click(B)] } else { &[] };
        agree(&mut state, engine.core, notices, &|_| true);
        state.webview_key_released.insert(B);
        let holder = agree(&mut state, engine.core, &[N::Focus(B)], &|_| true);
        if covered {
            assert_eq!(state.focused_surface_id(&engine.read()), Some(A));
            assert_eq!(
                state.focus_node(engine.core),
                crate::state::focus_node::FocusNode::Popup(
                    crate::adapters::ui::popup::focus::PopupNode::Host(cover.unwrap())
                )
            );
            assert!(
                state.webview_key_released.contains(&B),
                "덮인 포커스 획득이 회수 기록을 지웠다"
            );
            assert_eq!(holder, None);
        } else {
            assert_eq!(state.focused_surface_id(&engine.read()), Some(B));
            assert!(!state.webview_key_released.contains(&B));
            assert_eq!(holder, Some(B));
        }
    }
}
