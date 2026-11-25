//! plugin `surface.unfocused` 감지. 이동 재구성(`key_focus_moves`)과, 창이 도는 감지 단계(`observe_window_focus`)가
//! 실제 plugin surface 판정으로 내는 이벤트를 본다.

use super::{KeyFocusMove, key_focus_moves};
use crate::adapters::ui::popup::PopupScope;
use crate::runtime::engine_session::EngineSession;
use crate::state::{PendingHostEvent, RequestContext};

const fn left(surface_id: u32, next: Option<u32>) -> KeyFocusMove {
    KeyFocusMove::Left { surface_id, next }
}
const fn entered(surface_id: u32, prev: Option<u32>) -> KeyFocusMove {
    KeyFocusMove::Entered { surface_id, prev }
}

/// 진입 큐와 낸 값으로 이동을 다시 만든다: 다른 surface로 가면 떠남이 들어옴보다 먼저이고, 진입 사이나 마지막 진입
/// 뒤의 surface 없는 값(키를 가진 팝업)은 떠남 하나가 된다. 바뀐 것이 없으면 이동도 없다.
#[test]
fn moves_are_rebuilt_in_order_from_the_entries_and_the_shown_value() {
    assert_eq!(key_focus_moves(None, &[], None), (vec![], None));
    assert_eq!(key_focus_moves(Some(1), &[], Some(1)), (vec![], Some(1)));
    assert_eq!(
        key_focus_moves(None, &[(1, None)], Some(1)),
        (vec![entered(1, None)], Some(1))
    );
    assert_eq!(
        key_focus_moves(Some(1), &[], None),
        (vec![left(1, None)], None),
        "keys went to a popup"
    );
    assert_eq!(
        key_focus_moves(Some(1), &[(2, Some(1))], Some(2)),
        (vec![left(1, Some(2)), entered(2, Some(1))], Some(2)),
        "to another surface: left first"
    );
    assert_eq!(
        key_focus_moves(Some(1), &[(1, None)], Some(1)),
        (vec![left(1, None), entered(1, None)], Some(1)),
        "to a popup and back between two polls"
    );
    assert_eq!(
        key_focus_moves(Some(1), &[(1, None)], None),
        (vec![left(1, None), entered(1, None), left(1, None)], None),
        "back and away again between two polls"
    );
}

/// 선택이 201인 두 pane 장면. 201은 egui-mesh plugin surface로 바꾸고 101은 터미널로 둔다.
fn scene() -> (RequestContext, EngineSession) {
    crate::i18n::init("en");
    let (state, mut session) = crate::state::tests::two_pane_fixture();
    session.runtime.surfaces.insert(
        201,
        Box::new(crate::runtime::egui_mesh_surface::EguiMeshSurface::new(
            201,
            "mesh-demo",
            "com.example.demo".into(),
            "Mesh".into(),
            None,
        )),
    );
    assert_eq!(
        state.key_focus_surface(&session.borrow_mut().read()),
        Some(201)
    );
    (state, session)
}

/// 창이 프레임 하나를 화면에 낸 것처럼 마친다(`render_if_dirty`의 두 호출).
fn show(state: &mut RequestContext, session: &mut EngineSession) {
    state.show_key_focus_frame(&session.borrow_mut().read());
    state.note_shown_key_focus(true);
}

#[derive(Debug, PartialEq, Eq)]
enum Ev {
    Focused(u32, Option<u32>),
    Unfocused(u32, Option<u32>),
}

/// App이 창마다 도는 감지 단계를 돌리고 쌓인 포커스 이벤트를 순서대로 꺼낸다.
fn poll(state: &mut RequestContext, session: &mut EngineSession) -> Vec<Ev> {
    let mut engine = session.borrow_mut();
    crate::app::dispatch::host_events::observe_window_focus(state, &mut engine);
    crate::app::dispatch::host_events::take_current_host_events(&mut engine)
        .into_iter()
        .filter_map(|e| match e {
            PendingHostEvent::SurfaceFocused {
                surface_id,
                prev_surface_id,
            } => Some(Ev::Focused(surface_id, prev_surface_id)),
            PendingHostEvent::SurfaceUnfocused {
                surface_id,
                next_surface_id,
            } => Some(Ev::Unfocused(surface_id, next_surface_id)),
            _ => None,
        })
        .collect()
}

fn open_focused_popup(state: &mut RequestContext) {
    state
        .popups
        .open_centered_focused("notifications", PopupScope::Window)
        .expect("opens");
}

/// 키를 가져가는 팝업이 열리면 plugin surface는 포커스 잃음을 받고(간 곳 없음), 닫혀 돌아오면 앞 값 없는
/// `surface.focused`를 받는다. 화면에 낸 프레임이 없으면 아무것도 나지 않는다.
#[test]
fn a_popup_taking_the_keys_unfocuses_the_plugin_surface_and_closing_it_focuses_it_again() {
    let (mut state, mut session) = scene();
    show(&mut state, &mut session);
    assert_eq!(poll(&mut state, &mut session), vec![Ev::Focused(201, None)]);

    open_focused_popup(&mut state);
    assert_eq!(poll(&mut state, &mut session), vec![], "no frame shown yet");
    show(&mut state, &mut session);
    assert_eq!(
        poll(&mut state, &mut session),
        vec![Ev::Unfocused(201, None)]
    );
    show(&mut state, &mut session);
    assert_eq!(poll(&mut state, &mut session), vec![], "once");

    state.popups.close("notifications");
    show(&mut state, &mut session);
    assert_eq!(poll(&mut state, &mut session), vec![Ev::Focused(201, None)]);
}

/// 키 포커스가 plugin surface에서 다른 surface로 가면 같은 감지에서 잃음이 얻음보다 먼저 온다. 터미널(plugin이
/// 아닌 surface)은 키를 잃어도 잃음 이벤트가 없다.
#[test]
fn moving_to_another_surface_sends_unfocused_before_focused_and_not_for_a_terminal() {
    let (mut state, mut session) = scene();
    show(&mut state, &mut session);
    poll(&mut state, &mut session);

    assert!(state.focus_surface_by_id(&session.borrow_mut().read(), 101));
    show(&mut state, &mut session);
    assert_eq!(
        poll(&mut state, &mut session),
        vec![Ev::Unfocused(201, Some(101)), Ev::Focused(101, Some(201))]
    );

    open_focused_popup(&mut state);
    show(&mut state, &mut session);
    assert_eq!(poll(&mut state, &mut session), vec![], "terminal 101");
    state.popups.close("notifications");
    assert!(state.focus_surface_by_id(&session.borrow_mut().read(), 201));
    show(&mut state, &mut session);
    assert_eq!(
        poll(&mut state, &mut session),
        vec![Ev::Focused(201, None)],
        "from the popup to 201: no unfocused for 101"
    );
}

/// 키 포커스를 가져가지 않은 팝업이 키 하나를 받아 그 입력의 키 주인으로 묶여도(창의 키 처리가 묶는 결속) 포커스
/// 위치는 surface 그대로라 잃음이 나지 않는다.
#[test]
fn a_popup_bound_for_one_key_does_not_unfocus_the_surface() {
    let (mut state, mut session) = scene();
    show(&mut state, &mut session);
    poll(&mut state, &mut session);

    let iid = state
        .popups
        .open_request(
            &crate::intent::OpenRequest::new("notifications", PopupScope::Window),
            true,
        )
        .expect("opens passive");
    state.normalize_focus(&session.borrow_mut().read());
    state
        .popups
        .bind_input_key_owner(crate::adapters::ui::popup::focus::PopupNode::Host(iid));
    assert!(state.popups.bound_input_key_owner().is_some(), "bound");
    assert_eq!(
        state.key_focus_surface(&session.borrow_mut().read()),
        Some(201)
    );
    show(&mut state, &mut session);
    assert_eq!(poll(&mut state, &mut session), vec![]);
}

/// plugin surface는 egui-mesh·webview surface다. 터미널과 없는 surface는 아니다.
#[test]
fn plugin_surfaces_are_the_mesh_and_webview_surfaces() {
    let (_state, mut session) = scene();
    session.runtime.surfaces.insert(
        101,
        Box::new(crate::plugin_bridge::remote_surface::RemoteSurface::new(
            101,
            "html",
            "com.tasty.html".into(),
            "html".into(),
        )),
    );
    let (_terminal_state, mut terminal_session) = crate::state::tests::two_pane_fixture();
    let read = session.borrow_mut();
    let read = read.read();
    assert!(super::is_plugin_surface(&read, 201), "egui-mesh");
    assert!(super::is_plugin_surface(&read, 101), "webview");
    assert!(!super::is_plugin_surface(&read, 999), "missing");
    let terminal = terminal_session.borrow_mut();
    assert!(!super::is_plugin_surface(&terminal.read(), 101), "terminal");
}
