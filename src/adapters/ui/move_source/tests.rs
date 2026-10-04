use super::*;
use crate::model::SplitDirection;

fn test_engine() -> crate::runtime::engine_session::EngineSession {
    use tasty_core::{DomainEvent as E, Placement, Ratio, SplitSpec, SurfaceSpec};
    let tab = |id, pane, index| E::TabCreated {
        id,
        pane,
        index,
        name: "tab".into(),
        surface: SurfaceSpec {
            id,
            kind: "terminal".into(),
            data: None,
        },
    };
    crate::state::tests::test_state_from_model(crate::state::tests::test_model(vec![
        E::CategoryCreated {
            id: 0,
            name: "normal".into(),
            index: 0,
        },
        E::WorkspaceCreated {
            id: 1,
            name: "first".into(),
            category: 0,
            index: 0,
            pane: 1,
        },
        tab(1, 1, 0),
        tab(2, 1, 1),
        E::PaneSplit {
            target: 1,
            pane: 2,
            split: SplitSpec {
                direction: SplitDirection::Horizontal,
                ratio: Ratio::from_f32(0.5),
                placement: Placement::After,
            },
        },
        tab(3, 2, 0),
        E::WorkspaceCreated {
            id: 2,
            name: "other".into(),
            category: 0,
            index: 1,
            pane: 3,
        },
        tab(4, 3, 0),
    ]))
    .1
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

#[test]
fn empty_slot_emits_nothing() {
    let mut engine_session = test_engine();
    let engine = engine_session.borrow_mut();
    let pending_move = None;
    let navigation = crate::state::navigation::NavigationState::default();
    let (pane, _, _) = first_pane(&engine);
    let panes = [(pane, rect(0.0, 0.0, 400.0, 300.0))];
    assert!(same(
        resolve(
            pending_move,
            &navigation,
            &engine,
            0,
            &panes,
            BAR,
            None,
            1.0
        ),
        None
    ));
}

#[test]
fn pending_surface_region_is_emitted_only_for_the_pending_id() {
    let mut engine_session = test_engine();
    let engine = engine_session.borrow_mut();
    let mut pending_move;
    let navigation = crate::state::navigation::NavigationState::default();
    let (left, _, a) = first_pane(&engine);
    let (right, b) = (2, 3);
    let panes = [
        (left, rect(0.0, 0.0, 200.0, 300.0)),
        (right, rect(200.0, 0.0, 200.0, 300.0)),
    ];
    pending_move = Some(PendingMove::Surface(b));
    // 탭 바 아래 콘텐츠 영역의 b 자리만 나온다. a나 포커스와 무관하다.
    assert!(same(
        resolve(
            pending_move,
            &navigation,
            &engine,
            0,
            &panes,
            BAR,
            None,
            1.0
        ),
        Some(MoveSourceMark::Ring {
            pane_id: right,
            rect: rect(200.0, 24.0, 200.0, 276.0),
        })
    ));
    pending_move = Some(PendingMove::Surface(a));
    assert!(same(
        resolve(
            pending_move,
            &navigation,
            &engine,
            0,
            &panes,
            BAR,
            None,
            1.0
        ),
        Some(MoveSourceMark::Ring {
            pane_id: left,
            rect: rect(0.0, 24.0, 200.0, 276.0),
        })
    ));
}

#[test]
fn surface_in_an_inactive_tab_puts_the_glyph_on_that_tab() {
    let mut engine_session = test_engine();
    let engine = engine_session.borrow_mut();
    let mut pending_move;
    let mut navigation = crate::state::navigation::NavigationState::default();
    let (pane, _, a) = first_pane(&engine);
    let b = 2;
    navigation.goto_tab(engine.find_pane_by_id(pane).unwrap(), 1);
    pending_move = Some(PendingMove::Surface(a));
    let panes = [(pane, rect(0.0, 0.0, 400.0, 300.0))];
    assert!(same(
        resolve(
            pending_move,
            &navigation,
            &engine,
            0,
            &panes,
            BAR,
            None,
            1.0
        ),
        Some(MoveSourceMark::TabGlyph {
            pane_id: pane,
            tab_index: 0,
        })
    ));
    // 대상이 활성 탭에 있으면 글리프 대신 링이다.
    pending_move = Some(PendingMove::Surface(b));
    assert!(matches!(
        resolve(
            pending_move,
            &navigation,
            &engine,
            0,
            &panes,
            BAR,
            None,
            1.0
        ),
        Some(MoveSourceMark::Ring { .. })
    ));
}

#[test]
fn pending_tab_rings_its_cell_whether_active_or_not() {
    let mut engine_session = test_engine();
    let engine = engine_session.borrow_mut();
    let mut pending_move;
    let navigation = crate::state::navigation::NavigationState::default();
    let (pane, tab_a, _) = first_pane(&engine);
    let tab_b = 2;
    let panes = [(pane, rect(0.0, 0.0, 400.0, 300.0))];
    pending_move = Some(PendingMove::Tab(tab_b));
    assert!(same(
        resolve(
            pending_move,
            &navigation,
            &engine,
            0,
            &panes,
            BAR,
            None,
            1.0
        ),
        Some(MoveSourceMark::TabRing {
            pane_id: pane,
            tab_index: 1,
        })
    ));
    pending_move = Some(PendingMove::Tab(tab_a));
    assert!(same(
        resolve(
            pending_move,
            &navigation,
            &engine,
            0,
            &panes,
            BAR,
            None,
            1.0
        ),
        Some(MoveSourceMark::TabRing {
            pane_id: pane,
            tab_index: 0,
        })
    ));
}

#[test]
fn pending_pane_rings_the_whole_pane_rect() {
    let mut engine_session = test_engine();
    let engine = engine_session.borrow_mut();
    let pending_move;
    let navigation = crate::state::navigation::NavigationState::default();
    let (left, _, _) = first_pane(&engine);
    let right = 2;
    let right_rect = rect(200.0, 0.0, 200.0, 300.0);
    let panes = [(left, rect(0.0, 0.0, 200.0, 300.0)), (right, right_rect)];
    pending_move = Some(PendingMove::Pane(right));
    assert!(same(
        resolve(
            pending_move,
            &navigation,
            &engine,
            0,
            &panes,
            BAR,
            None,
            1.0
        ),
        Some(MoveSourceMark::Ring {
            pane_id: right,
            rect: right_rect,
        })
    ));
}

#[test]
fn target_in_another_workspace_marks_that_workspace_for_every_kind() {
    let mut engine_session = test_engine();
    let engine = engine_session.borrow_mut();
    let mut pending_move;
    let navigation = crate::state::navigation::NavigationState::default();
    let (pane, _, _) = first_pane(&engine);
    let (other_pane, other_tab, other_sid) = (3, 4, 4);
    let panes = [(pane, rect(0.0, 0.0, 400.0, 300.0))];
    for pending in [
        PendingMove::Surface(other_sid),
        PendingMove::Tab(other_tab),
        PendingMove::Pane(other_pane),
    ] {
        pending_move = Some(pending);
        assert!(
            same(
                resolve(
                    pending_move,
                    &navigation,
                    &engine,
                    0,
                    &panes,
                    BAR,
                    None,
                    1.0
                ),
                None
            ),
            "{pending:?}"
        );
        assert_eq!(
            workspace_cue(&engine, 0, pending_move),
            Some(1),
            "{pending:?}"
        );
    }
    // 활성 워크스페이스가 바뀌면 같은 슬롯이 사이드바 단서 대신 링으로 보인다.
    pending_move = Some(PendingMove::Pane(other_pane));
    let other_panes = [(other_pane, rect(0.0, 0.0, 400.0, 300.0))];
    assert!(matches!(
        resolve(
            pending_move,
            &navigation,
            &engine,
            1,
            &other_panes,
            BAR,
            None,
            1.0
        ),
        Some(MoveSourceMark::Ring { .. })
    ));
    assert_eq!(workspace_cue(&engine, 1, pending_move), None);
}

#[test]
fn hidden_pane_in_the_active_workspace_emits_nothing() {
    let mut engine_session = test_engine();
    let engine = engine_session.borrow_mut();
    let pending_move;
    let navigation = crate::state::navigation::NavigationState::default();
    let (left, _, _) = first_pane(&engine);
    let right = 2;
    pending_move = Some(PendingMove::Pane(right));
    // right가 pane_rects에 없는 프레임. 지금은 생기지 않는 방어 경로다.
    let panes = [(left, rect(0.0, 0.0, 400.0, 300.0))];
    assert!(same(
        resolve(
            pending_move,
            &navigation,
            &engine,
            0,
            &panes,
            BAR,
            None,
            1.0
        ),
        None
    ));
}

#[test]
fn closed_pending_target_emits_nothing() {
    let mut engine_session = test_engine();
    let engine = engine_session.borrow_mut();
    let mut pending_move;
    let navigation = crate::state::navigation::NavigationState::default();
    let (pane, _, _) = first_pane(&engine);
    let panes = [(pane, rect(0.0, 0.0, 400.0, 300.0))];
    for pending in [
        PendingMove::Surface(999_999),
        PendingMove::Tab(999_999),
        PendingMove::Pane(999_999),
    ] {
        pending_move = Some(pending);
        assert!(
            same(
                resolve(
                    pending_move,
                    &navigation,
                    &engine,
                    0,
                    &panes,
                    BAR,
                    None,
                    1.0
                ),
                None
            ),
            "{pending:?}"
        );
        assert!(
            clear_if_target_closed(&engine, &mut pending_move),
            "{pending:?}"
        );
        assert!(pending_move.is_none());
    }
}

#[test]
fn live_target_keeps_the_slot() {
    let mut engine_session = test_engine();
    let engine = engine_session.borrow_mut();
    let mut pending_move;
    let (pane, tab, a) = first_pane(&engine);
    for pending in [
        PendingMove::Surface(a),
        PendingMove::Tab(tab),
        PendingMove::Pane(pane),
    ] {
        pending_move = Some(pending);
        assert!(!clear_if_target_closed(&engine, &mut pending_move));
        assert_eq!(pending_move, Some(pending));
    }
    pending_move = None;
    assert!(!clear_if_target_closed(&engine, &mut pending_move));
    assert_eq!(workspace_cue(&engine, 0, pending_move), None);
}
