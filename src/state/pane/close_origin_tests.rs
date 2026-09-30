//! 창 경로(사용자)와 IPC 경로(에이전트)가 같은 Core 닫기를 타고,
//! 사용자 닫기만 복원 기록을 남기는지 검사한다.

use crate::core::CoreState;
use crate::core::intent::{CascadeLevel, CoreEvent, DomainIntent};
use crate::model::{ClosedItem, SplitDirection};
use crate::state::RequestContext;
use crate::state::tests::test_state;
use tasty_terminal::Terminal;

/// 닫을 surface와 기대하는 닫기 계층을 만든다.
#[derive(Clone, Copy, Debug)]
enum Case {
    /// split 탭의 surface 하나.
    Surface,
    /// 탭이 둘인 pane의 탭 하나.
    Tab,
    /// pane이 둘인 workspace의 pane 하나.
    Pane,
    /// workspace가 둘일 때 surface가 하나뿐인 workspace.
    Workspace,
}

const CASES: [Case; 4] = [Case::Surface, Case::Tab, Case::Pane, Case::Workspace];

fn insert_detached(engine: &mut CoreState, sid: u32) {
    engine
        .runtime
        .terminals
        .insert(sid, Terminal::new_detached(80, 24));
}

/// 닫을 surface를 돌려준다. 시나리오는 활성 workspace 안에서 만든다.
fn arrange(case: Case) -> (RequestContext, CoreState, u32) {
    let (mut state, mut engine) = test_state();
    let sid_a = state.focused_surface_id(&engine).unwrap();
    let ws_idx = state.active_workspace_index(&engine);
    let pane_id = state.active_workspace(&engine).focused_pane;
    let target = match case {
        Case::Surface => {
            let sid_b = engine.next_ids.next_surface();
            engine.workspaces[ws_idx]
                .pane_layout_mut()
                .find_pane_mut(pane_id)
                .unwrap()
                .split_surface_by_id_marker(sid_a, SplitDirection::Horizontal, sid_b)
                .unwrap();
            insert_detached(&mut engine, sid_b);
            sid_a
        }
        Case::Tab => {
            state.add_tab(&mut engine).unwrap();
            state.focused_surface_id(&engine).unwrap()
        }
        Case::Pane => {
            let new_pane_id = engine.next_ids.next_pane();
            let new_tab_id = engine.next_ids.next_tab();
            let sid_b = engine.next_ids.next_surface();
            insert_detached(&mut engine, sid_b);
            let pane = crate::model::Pane::new_with_terminal_marker(new_pane_id, new_tab_id, sid_b);
            engine.workspaces[ws_idx]
                .pane_layout_mut()
                .split_pane_in_place(pane_id, SplitDirection::Vertical, pane);
            engine.workspaces[ws_idx].focused_pane = new_pane_id;
            sid_b
        }
        Case::Workspace => {
            let event = crate::core::apply_create_workspace_inner(
                &mut engine,
                crate::core::WorkspaceCreationParams::terminal(),
            )
            .unwrap();
            let CoreEvent::WorkspaceCreated { index, .. } = event else {
                panic!("WorkspaceCreated expected");
            };
            state.set_active_workspace_index(&engine, index);
            engine.workspaces[index].all_surface_ids()[0]
        }
    };
    (state, engine, target)
}

fn expected_level(case: Case) -> CascadeLevel {
    match case {
        Case::Surface => CascadeLevel::Surface,
        Case::Tab => CascadeLevel::Tab,
        Case::Pane => CascadeLevel::Pane,
        Case::Workspace => CascadeLevel::Workspace,
    }
}

fn record_matches(case: Case, item: &ClosedItem) -> bool {
    matches!(
        (case, item),
        (Case::Surface, ClosedItem::Surface { .. })
            | (Case::Tab, ClosedItem::Tab(_))
            | (Case::Pane, ClosedItem::Pane { .. })
            | (Case::Workspace, ClosedItem::Workspace { .. })
    )
}

#[test]
fn a_user_surface_close_records_one_item_of_its_level() {
    for case in CASES {
        let (mut state, mut engine, sid) = arrange(case);
        let before = engine.closed_items.len();

        assert!(
            state.close_surface_by_id(&mut engine, sid, true),
            "{case:?}"
        );

        assert_eq!(engine.closed_items.len(), before + 1, "{case:?}");
        let newest = engine.closed_items.list().next().unwrap();
        assert!(record_matches(case, newest), "{case:?}");
    }
}

#[test]
fn an_agent_surface_close_records_nothing() {
    for case in CASES {
        let (mut state, mut engine, sid) = arrange(case);
        let mut core = crate::ipc::handler::cli_entry_tests::test_core();
        let before = engine.closed_items.len();

        let closed = crate::app::structural_exec::close_surface(
            &mut core,
            &mut state,
            &mut engine,
            sid,
            false,
            &crate::core::origin::IPC_AGENT,
        )
        .expect("close_surface");

        assert!(closed.closed, "{case:?}");
        assert_eq!(engine.closed_items.len(), before, "{case:?}");
    }
}

#[test]
fn user_pane_and_tab_closes_record_but_agent_ones_do_not() {
    let (mut state, mut engine, _) = arrange(Case::Pane);
    assert!(state.close_active_pane(&mut engine));
    assert_eq!(engine.closed_items.len(), 1);
    assert!(record_matches(
        Case::Pane,
        engine.closed_items.list().next().unwrap()
    ));

    let (mut state, mut engine, _) = arrange(Case::Tab);
    assert!(state.close_active_tab(&mut engine));
    assert_eq!(engine.closed_items.len(), 1);
    assert!(record_matches(
        Case::Tab,
        engine.closed_items.list().next().unwrap()
    ));

    let mut core = crate::ipc::handler::cli_entry_tests::test_core();
    let (mut state, mut engine, _) = arrange(Case::Pane);
    let pane_id = state.active_workspace(&engine).focused_pane;
    let closed = crate::app::structural_exec::close_pane(
        &mut core,
        &mut state,
        &mut engine,
        pane_id,
        &crate::core::origin::IPC_AGENT,
    )
    .expect("close_pane");
    assert!(closed.closed);
    assert_eq!(engine.closed_items.len(), 0, "agent pane close");

    let (mut state, mut engine, sid) = arrange(Case::Tab);
    let tab_id = engine
        .find_tab_for_surface(sid)
        .expect("tab of the focused surface");
    let closed = crate::app::structural_exec::close_tab(
        &mut core,
        &mut state,
        &mut engine,
        tab_id,
        &crate::core::origin::IPC_AGENT,
    )
    .expect("close_tab");
    assert!(closed.closed);
    assert_eq!(engine.closed_items.len(), 0, "agent tab close");
}

/// 닫지 못한 탭은 복원 목록에 들어가지 않는다.
#[test]
fn closing_the_only_tab_fails_without_a_record() {
    let (mut state, mut engine) = test_state();
    assert!(!state.close_active_tab(&mut engine));
    assert_eq!(engine.closed_items.len(), 0);
}

/// 같은 시나리오를 창 경로와 Core::apply(IPC와 같은 입구)로 닫아 결과 이벤트를 비교한다.
#[test]
fn the_window_path_and_core_apply_return_the_same_surface_closed_event() {
    for case in CASES {
        let (mut state, mut engine, sid) = arrange(case);
        let window_event = state
            .close_surface_by_id_inner(&mut engine, sid, false, true)
            .unwrap_or_else(|| panic!("{case:?}: window path did not close"));

        let (_, mut twin, twin_sid) = arrange(case);
        assert_eq!(sid, twin_sid, "{case:?}: twin scenario ids differ");
        let mut core = crate::ipc::handler::cli_entry_tests::test_core();
        let core_events = core
            .apply(
                &mut twin,
                DomainIntent::CloseSurface {
                    surface_id: sid,
                    save_snapshot: false,
                },
            )
            .expect("Core::apply CloseSurface");

        assert_eq!(core_events.len(), 1, "{case:?}");
        assert_eq!(
            format!("{window_event:?}"),
            format!("{:?}", core_events[0]),
            "{case:?}"
        );
        let CoreEvent::SurfaceClosed { cascade_level, .. } = window_event else {
            panic!("{case:?}: SurfaceClosed expected");
        };
        assert_eq!(cascade_level, expected_level(case), "{case:?}");
    }
}

/// 사용자 창 경로의 mirror 닫기는 user_triggered로 전달한다.
#[test]
fn a_mirror_close_active_surface_from_the_window_forwards_as_user_triggered() {
    let (mut state, mut engine, sid) = arrange(Case::Surface);
    state.active_workspace_mut(&mut engine).mirror = true;

    assert!(state.close_active_surface(&mut engine));

    assert!(engine.find_terminal_by_id(sid).is_some());
    assert_eq!(engine.closed_items.len(), 0);
    assert_eq!(engine.pending_structural_forward.len(), 1);
    assert!(engine.pending_structural_forward[0].user_triggered);
}

/// 에이전트의 mirror 닫기는 Core 입구에서 에이전트 요청으로 전달한다.
#[cfg(feature = "gui")] // headless에는 전달 큐를 보내는 루프가 없어 거절한다
#[test]
fn a_mirror_close_from_an_agent_forwards_as_not_user_triggered() {
    let (mut state, mut engine, sid) = arrange(Case::Surface);
    state.active_workspace_mut(&mut engine).mirror = true;
    let mut core = crate::ipc::handler::cli_entry_tests::test_core();

    let result = crate::app::structural_exec::close_surface(
        &mut core,
        &mut state,
        &mut engine,
        sid,
        false,
        &crate::core::origin::IPC_AGENT,
    );

    assert!(
        result.is_err(),
        "mirror 구조 변경은 로컬에서 실행하지 않는다"
    );
    assert!(engine.find_terminal_by_id(sid).is_some());
    assert_eq!(engine.pending_structural_forward.len(), 1);
    assert!(!engine.pending_structural_forward[0].user_triggered);
}
