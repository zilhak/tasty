//! P3a: 대기열의 앞 입력이 누름 알림의 대상을 바꾼 뒤 그 알림을 소비하면 전이를 적용하지 않는다. 대상이 표시되지
//! 않은 탭에 있으면 `MainViewState::surface_displayed` 가, WebView binding 이 지금 surface 와 맞지 않으면
//! `webview_sync::webview_binding_current`(시험은 그 결과를 `bound` 로 넘긴다)가 거른다. 장면은 왼쪽 pane 에 A(11),
//! 오른쪽 pane 에 탭 두 개 B(12, 표시)·C(13)이고, A 범위 검색 바가 포커스를 가진다.

use crate::adapters::ui::popup::PopupScope;
use crate::adapters::ui::popup::focus::PopupNode;
use crate::adapters::ui::search_bar::SEARCH_BAR_POPUP_ID;
use crate::host_api::webview::keys::{WebViewKeyBridge, WebViewKeySink};
use crate::state::MainViewState;
use crate::state::focus_node::FocusNode;

const A: u32 = 11;
const B: u32 = 12;
const C: u32 = 13;
const RIGHT_PANE: u32 = 2;

fn scene() -> (
    MainViewState,
    crate::runtime::engine_session::EngineSession,
    PopupNode,
) {
    use tasty_core::{DomainEvent as E, SurfaceSpec};
    let mut events = vec![
        E::CategoryCreated {
            id: 0,
            name: "normal".into(),
            index: 0,
        },
        E::WorkspaceCreated {
            id: 1,
            name: "w".into(),
            category: 0,
            index: 0,
            pane: 1,
        },
        E::PaneSplit {
            target: 1,
            pane: RIGHT_PANE,
            split: tasty_core::SplitSpec {
                direction: crate::model::SplitDirection::Vertical,
                ratio: tasty_core::Ratio::from_f32(0.5),
                placement: tasty_core::Placement::After,
            },
        },
    ];
    for (id, pane, index) in [(A, 1, 0), (B, RIGHT_PANE, 0), (C, RIGHT_PANE, 1)] {
        events.push(E::TabCreated {
            id,
            pane,
            index,
            name: "page".into(),
            surface: SurfaceSpec {
                id,
                kind: "terminal".into(),
                data: None,
            },
        });
    }
    let (mut state, mut session) =
        crate::state::tests::test_state_from_model(crate::state::tests::test_model(events));
    let bar = {
        let engine = session.borrow_mut();
        state.focus_surface_by_id(engine.core, A);
        let bar = state
            .popups
            .open_request(
                &crate::intent::OpenRequest::new(SEARCH_BAR_POPUP_ID, PopupScope::Surface(A))
                    .focused(),
                true,
            )
            .expect("검색 바가 열린다");
        state.normalize_focus(engine.core);
        assert!(state.surface_displayed(engine.core, B), "B 가 표시된다");
        assert!(!state.surface_displayed(engine.core, C), "C 는 가려졌다");
        PopupNode::Host(bar)
    };
    (state, session, bar)
}

/// 오른쪽 pane 을 탭 C 로 바꾼다(앞 입력의 탭 전환).
fn show_c(state: &mut MainViewState, core: &crate::core::CoreState) {
    let ws = state.active_workspace(core);
    let pane = ws.pane_layout().find_pane(RIGHT_PANE).expect("오른쪽 pane");
    assert!(state.navigation.select_tab(pane, C));
}

/// Given 오른쪽 pane 이 B 를 보이고 A 범위 검색 바가 포커스일 때 B 의 누름·포커스 획득 알림이 대기열에 있다, When 앞
/// 입력이 오른쪽 pane 을 탭 C 로 바꾼 뒤 그 알림을 소비하면, Then 알림은 아무것도 바꾸지 않는다(검색 바 포커스·선택 A·
/// 회수 기록 그대로). 대조: 탭이 그대로면 B 누름이 검색 바 포커스를 풀고 B 를 고른다.
#[test]
fn a_press_on_a_tab_a_prior_entry_switched_away_changes_nothing() {
    for (kind, switched) in [
        (crate::webview::PressKind::Click, false),
        (crate::webview::PressKind::Click, true),
        (crate::webview::PressKind::Focus, true),
    ] {
        let (mut state, mut session, bar) = scene();
        let engine = session.borrow_mut();
        state.webview_key_released.insert(B);
        let bridge = WebViewKeyBridge::new();
        match kind {
            crate::webview::PressKind::Click => bridge.note_press(B),
            crate::webview::PressKind::Focus => bridge.note_focus(B),
        }
        if switched {
            show_c(&mut state, engine.core);
        }
        let changed = crate::app::webview_keys::consume_native_focus_requests(
            &mut state,
            engine.core,
            &bridge,
            &|_| true,
        );
        let node = state.focus_node(engine.core);
        if !switched {
            assert!(changed, "대조");
            assert_eq!(node, FocusNode::Surface(Some(B)), "대조");
            continue;
        }
        assert!(!changed, "{kind:?}: 가려진 B 의 알림이 선택을 바꿨다");
        assert_eq!(
            node,
            FocusNode::Popup(bar),
            "{kind:?}: 검색 바 포커스가 풀렸다"
        );
        assert_eq!(state.focused_surface_id(&engine.read()), Some(A));
        assert!(
            state.webview_key_released.contains(&B),
            "{kind:?}: 가려진 B 의 회수 기록이 지워졌다"
        );
    }
}

/// Given A 범위 검색 바가 포커스일 때 [B 누름, A 누름] 이 대기열에 있다, When 앞 입력이 B 의 WebView 를 교체해 B 의
/// binding 이 맞지 않은 채 소비하면, Then B 누름은 건너뛰고(검색 바 포커스는 A 누름이 정한다) 뒤 A 누름은 그대로
/// 적용된다. 대조: binding 이 맞으면 B 누름이 B 를 고른 뒤 A 누름이 A 로 돌린다. 둘 다 끝 노드는 A 본문이고, 차이는
/// B 의 회수 기록이다.
#[test]
fn a_press_for_a_webview_a_prior_entry_replaced_is_skipped() {
    for replaced in [false, true] {
        let (mut state, mut session, _) = scene();
        let engine = session.borrow_mut();
        state.webview_key_released.insert(B);
        let bridge = WebViewKeyBridge::new();
        bridge.note_press(B);
        bridge.note_press(A);
        let bound = |sid| !(replaced && sid == B);
        crate::app::webview_keys::consume_native_focus_requests(
            &mut state,
            engine.core,
            &bridge,
            &bound,
        );
        assert_eq!(state.focus_node(engine.core), FocusNode::Surface(Some(A)));
        assert!(!bridge.has_focus_requests(), "두 알림 모두 소비됐다");
        assert_eq!(
            state.webview_key_released.contains(&B),
            replaced,
            "replaced={replaced}: 옛 WebView 누름이 새 WebView 의 회수 기록을 지웠는지"
        );
    }
}

/// P3b: Given 유지한 HTTP 세션의 B 가 새 문서로 바뀌기 전 대기열이 [B 누름, B 키(1), A 키(2), B 포커스 획득] 이다,
/// When 동기화가 B 의 새 문서를 맞춰 옛 문서 항목을 지우면(`retire_page`), Then B 의 항목만 지워지고 끝낼 키는 B 의
/// 키(1)뿐이며 A 의 키는 남는다. 그 뒤 B 가 낸 새 누름은 지워지지 않고 소비된다(직렬 교체 → 같은 surface 의 새 입력).
#[test]
fn a_replaced_page_drops_only_its_own_entries_and_takes_new_ones() {
    use crate::webview::NativeInput;
    use winit::keyboard::{Key, ModifiersState, NativeKeyCode, PhysicalKey};
    let bridge = WebViewKeyBridge::new();
    bridge.set_policy(crate::webview::HostShortcutPolicy::new(
        crate::webview::ShortcutSources {
            host: vec!["ctrl+j".to_string()],
            ..Default::default()
        },
    ));
    bridge.set_key_holder(Some(B));
    let key = |sid| {
        bridge.forward_key(
            sid,
            Key::Character("j".into()),
            PhysicalKey::Unidentified(NativeKeyCode::Unidentified),
            ModifiersState::CONTROL,
        )
    };
    bridge.note_press(B);
    let old = key(B).expect("B 키가 대기열에 든다");
    bridge.set_key_holder(Some(A));
    let kept = key(A).expect("A 키가 대기열에 든다");
    bridge.note_focus(B);
    assert_eq!(
        bridge.retire_page(B),
        vec![old],
        "끝낼 키는 B 의 옛 키뿐이다"
    );
    bridge.note_press(B);
    let left: Vec<_> = bridge
        .take_inputs()
        .into_iter()
        .map(|input| match input {
            NativeInput::Press { surface_id, .. } => (surface_id, None),
            NativeInput::Forwarded(ev) => (ev.surface_id, Some(ev.id)),
        })
        .collect();
    assert_eq!(left, vec![(A, Some(kept)), (B, None)]);
}
