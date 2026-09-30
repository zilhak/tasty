use super::*;
use crate::model::SplitDirection;

fn test_engine() -> crate::runtime::engine_session::EngineSession {
    let waker: tasty_terminal::Waker = std::sync::Arc::new(|| {});
    crate::runtime::engine_session::EngineSession::new(80, 24, waker).expect("engine")
}

fn rect(x: f32, y: f32, w: f32, h: f32) -> PhysicalRect {
    PhysicalRect {
        x: PhysicalPx(x),
        y: PhysicalPx(y),
        width: PhysicalPx(w),
        height: PhysicalPx(h),
    }
}

const BAR: PhysicalPx = PhysicalPx(24.0);

/// PhysicalRect에 PartialEq가 없어 Debug 표현으로 비교한다.
fn same(a: Option<MoveSourceMark>, b: Option<MoveSourceMark>) -> bool {
    format!("{a:?}") == format!("{b:?}")
}

/// 첫 workspace의 첫 pane, 그 첫 탭과 surface.
fn first_pane(engine: &CoreState) -> (u32, u32, u32) {
    let a = engine
        .workspace_at(0)
        .expect("workspace index is valid")
        .all_surface_ids()[0];
    let (_, pane_id) = engine.find_workspace_index_for_surface(a).unwrap();
    let tab_id = engine.find_pane_by_id(pane_id).unwrap().tabs[0].id;
    (pane_id, tab_id, a)
}

fn add_tab(engine: &mut CoreState, pane_id: u32) -> (u32, u32) {
    let tab_id = engine.next_ids.next_tab();
    let sid = engine.next_ids.next_surface();
    engine
        .find_pane_by_id_mut(pane_id)
        .unwrap()
        .add_terminal_marker_tab(tab_id, sid);
    (tab_id, sid)
}

fn split_new_pane(engine: &mut CoreState, pane_id: u32) -> (u32, u32) {
    let new_pane_id = engine.next_ids.next_pane();
    let tab_id = engine.next_ids.next_tab();
    let sid = engine.next_ids.next_surface();
    let pane = crate::model::Pane::new_with_terminal_marker(new_pane_id, tab_id, sid);
    let ws_idx = engine.find_workspace_index_for_pane(pane_id).unwrap();
    assert!(
        engine
            .workspace_at_mut(ws_idx)
            .expect("workspace index is valid")
            .pane_layout_mut()
            .split_pane_in_place(pane_id, SplitDirection::Horizontal, pane)
            .is_none()
    );
    (new_pane_id, sid)
}

fn push_workspace(engine: &mut CoreState) -> (u32, u32, u32) {
    let ws_id = engine.next_ids.next_workspace();
    let pane_id = engine.next_ids.next_pane();
    let tab_id = engine.next_ids.next_tab();
    let sid = engine.next_ids.next_surface();
    engine.push_local_workspace(crate::model::Workspace::new_with_terminal_marker(
        ws_id,
        "ws1".to_string(),
        pane_id,
        tab_id,
        sid,
    ));
    (pane_id, tab_id, sid)
}

#[test]
fn empty_slot_emits_nothing() {
    let mut engine_session = test_engine();
    let engine = engine_session.borrow_mut();
    let navigation = crate::state::navigation::NavigationState::default();
    let (pane, _, _) = first_pane(&engine);
    let panes = [(pane, rect(0.0, 0.0, 400.0, 300.0))];
    assert!(same(
        resolve(&navigation, &engine, 0, &panes, BAR, None, 1.0),
        None
    ));
}

#[test]
fn pending_surface_region_is_emitted_only_for_the_pending_id() {
    let mut engine_session = test_engine();
    let mut engine = engine_session.borrow_mut();
    let navigation = crate::state::navigation::NavigationState::default();
    let (left, _, a) = first_pane(&engine);
    let (right, b) = split_new_pane(&mut engine, left);
    let panes = [
        (left, rect(0.0, 0.0, 200.0, 300.0)),
        (right, rect(200.0, 0.0, 200.0, 300.0)),
    ];
    engine.pending_move = Some(PendingMove::Surface(b));
    // 탭 바 아래 콘텐츠 영역의 b 자리만 나온다. a나 포커스와 무관하다.
    assert!(same(
        resolve(&navigation, &engine, 0, &panes, BAR, None, 1.0),
        Some(MoveSourceMark::Ring {
            pane_id: right,
            rect: rect(200.0, 24.0, 200.0, 276.0),
        })
    ));
    engine.pending_move = Some(PendingMove::Surface(a));
    assert!(same(
        resolve(&navigation, &engine, 0, &panes, BAR, None, 1.0),
        Some(MoveSourceMark::Ring {
            pane_id: left,
            rect: rect(0.0, 24.0, 200.0, 276.0),
        })
    ));
}

#[test]
fn surface_in_an_inactive_tab_puts_the_glyph_on_that_tab() {
    let mut engine_session = test_engine();
    let mut engine = engine_session.borrow_mut();
    let mut navigation = crate::state::navigation::NavigationState::default();
    let (pane, _, a) = first_pane(&engine);
    let (_, b) = add_tab(&mut engine, pane);
    navigation.goto_tab(engine.find_pane_by_id(pane).unwrap(), 1);
    engine.pending_move = Some(PendingMove::Surface(a));
    let panes = [(pane, rect(0.0, 0.0, 400.0, 300.0))];
    assert!(same(
        resolve(&navigation, &engine, 0, &panes, BAR, None, 1.0),
        Some(MoveSourceMark::TabGlyph {
            pane_id: pane,
            tab_index: 0,
        })
    ));
    // 대상이 활성 탭에 있으면 글리프 대신 링이다.
    engine.pending_move = Some(PendingMove::Surface(b));
    assert!(matches!(
        resolve(&navigation, &engine, 0, &panes, BAR, None, 1.0),
        Some(MoveSourceMark::Ring { .. })
    ));
}

#[test]
fn pending_tab_rings_its_cell_whether_active_or_not() {
    let mut engine_session = test_engine();
    let mut engine = engine_session.borrow_mut();
    let navigation = crate::state::navigation::NavigationState::default();
    let (pane, tab_a, _) = first_pane(&engine);
    let (tab_b, _) = add_tab(&mut engine, pane);
    let panes = [(pane, rect(0.0, 0.0, 400.0, 300.0))];
    engine.pending_move = Some(PendingMove::Tab(tab_b));
    assert!(same(
        resolve(&navigation, &engine, 0, &panes, BAR, None, 1.0),
        Some(MoveSourceMark::TabRing {
            pane_id: pane,
            tab_index: 1,
        })
    ));
    engine.pending_move = Some(PendingMove::Tab(tab_a));
    assert!(same(
        resolve(&navigation, &engine, 0, &panes, BAR, None, 1.0),
        Some(MoveSourceMark::TabRing {
            pane_id: pane,
            tab_index: 0,
        })
    ));
}

#[test]
fn pending_pane_rings_the_whole_pane_rect() {
    let mut engine_session = test_engine();
    let mut engine = engine_session.borrow_mut();
    let navigation = crate::state::navigation::NavigationState::default();
    let (left, _, _) = first_pane(&engine);
    let (right, _) = split_new_pane(&mut engine, left);
    let right_rect = rect(200.0, 0.0, 200.0, 300.0);
    let panes = [(left, rect(0.0, 0.0, 200.0, 300.0)), (right, right_rect)];
    engine.pending_move = Some(PendingMove::Pane(right));
    assert!(same(
        resolve(&navigation, &engine, 0, &panes, BAR, None, 1.0),
        Some(MoveSourceMark::Ring {
            pane_id: right,
            rect: right_rect,
        })
    ));
}

#[test]
fn target_in_another_workspace_marks_that_workspace_for_every_kind() {
    let mut engine_session = test_engine();
    let mut engine = engine_session.borrow_mut();
    let navigation = crate::state::navigation::NavigationState::default();
    let (pane, _, _) = first_pane(&engine);
    let (other_pane, other_tab, other_sid) = push_workspace(&mut engine);
    let panes = [(pane, rect(0.0, 0.0, 400.0, 300.0))];
    for pending in [
        PendingMove::Surface(other_sid),
        PendingMove::Tab(other_tab),
        PendingMove::Pane(other_pane),
    ] {
        engine.pending_move = Some(pending);
        assert!(
            same(
                resolve(&navigation, &engine, 0, &panes, BAR, None, 1.0),
                None
            ),
            "{pending:?}"
        );
        assert_eq!(workspace_cue(&engine, 0), Some(1), "{pending:?}");
    }
    // 활성 워크스페이스가 바뀌면 같은 슬롯이 사이드바 단서 대신 링으로 보인다.
    engine.pending_move = Some(PendingMove::Pane(other_pane));
    let other_panes = [(other_pane, rect(0.0, 0.0, 400.0, 300.0))];
    assert!(matches!(
        resolve(&navigation, &engine, 1, &other_panes, BAR, None, 1.0),
        Some(MoveSourceMark::Ring { .. })
    ));
    assert_eq!(workspace_cue(&engine, 1), None);
}

#[test]
fn hidden_pane_in_the_active_workspace_emits_nothing() {
    let mut engine_session = test_engine();
    let mut engine = engine_session.borrow_mut();
    let navigation = crate::state::navigation::NavigationState::default();
    let (left, _, _) = first_pane(&engine);
    let (right, _) = split_new_pane(&mut engine, left);
    engine.pending_move = Some(PendingMove::Pane(right));
    // right가 pane_rects에 없는 프레임. 지금은 생기지 않는 방어 경로다.
    let panes = [(left, rect(0.0, 0.0, 400.0, 300.0))];
    assert!(same(
        resolve(&navigation, &engine, 0, &panes, BAR, None, 1.0),
        None
    ));
}

#[test]
fn closed_pending_target_emits_nothing() {
    let mut engine_session = test_engine();
    let mut engine = engine_session.borrow_mut();
    let navigation = crate::state::navigation::NavigationState::default();
    let (pane, _, _) = first_pane(&engine);
    let panes = [(pane, rect(0.0, 0.0, 400.0, 300.0))];
    for pending in [
        PendingMove::Surface(999_999),
        PendingMove::Tab(999_999),
        PendingMove::Pane(999_999),
    ] {
        engine.pending_move = Some(pending);
        assert!(
            same(
                resolve(&navigation, &engine, 0, &panes, BAR, None, 1.0),
                None
            ),
            "{pending:?}"
        );
        assert!(clear_if_target_closed(&mut engine), "{pending:?}");
        assert!(engine.pending_move.is_none());
    }
}

#[test]
fn live_target_keeps_the_slot() {
    let mut engine_session = test_engine();
    let mut engine = engine_session.borrow_mut();
    let (pane, tab, a) = first_pane(&engine);
    for pending in [
        PendingMove::Surface(a),
        PendingMove::Tab(tab),
        PendingMove::Pane(pane),
    ] {
        engine.pending_move = Some(pending);
        assert!(!clear_if_target_closed(&mut engine));
        assert_eq!(engine.pending_move, Some(pending));
    }
    engine.pending_move = None;
    assert!(!clear_if_target_closed(&mut engine));
    assert_eq!(workspace_cue(&engine, 0), None);
}
