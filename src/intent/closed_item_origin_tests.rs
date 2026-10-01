//! 복원 핸들러가 요청의 origin을 후속 처리에 그대로 전달하는지 검사한다.
//! app/dispatch_domain_restore_tests.rs의 직접 호출과 달리 실제 핸들러를 거친다.

use super::handle;
use crate::intent::Intent;
use crate::state::WorkspaceCloseOrigin;

fn active_after_restoring_a_closed_workspace(intent: crate::intent::DispatchedIntent) -> usize {
    let (mut state, mut engine_session) = crate::state::tests::test_state();
    let mut engine = engine_session.borrow_mut();
    let event = crate::app::services::apply_create_workspace_inner(
        &mut engine,
        crate::app::services::WorkspaceCreationParams::terminal(),
    )
    .expect("second workspace");
    let crate::app::command::CoreEvent::WorkspaceCreated { index, .. } = event else {
        panic!("apply_create_workspace_inner가 WorkspaceCreated를 반환해야 한다");
    };
    assert_eq!(index, 1);
    let second = engine.runtime.counters.next_surface();
    let added_tab = engine.runtime.counters.next_tab();
    let added_surface = engine.runtime.counters.next_surface();
    let pid = engine
        .workspace_at(1)
        .expect("workspace index is valid")
        .pane_layout()
        .first_pane()
        .unwrap()
        .id;
    let pane = engine
        .workspace_at_mut(1)
        .expect("workspace index is valid")
        .pane_layout_mut()
        .find_pane_mut(pid)
        .unwrap();
    let first = pane.tabs[0].first_surface_id().unwrap();
    pane.tabs[0].split_surface_by_id_generic(
        first,
        crate::model::SplitDirection::Horizontal,
        Box::new(crate::model::EmptySurface::new(second)),
    );
    pane.add_surface_tab_background(
        added_tab,
        "restore-selected".into(),
        None,
        Box::new(crate::model::EmptySurface::new(added_surface)),
    );
    state.navigation.select_tab(pane, added_tab);
    assert!(state.close_workspace_at(&mut engine, 1, WorkspaceCloseOrigin::User));
    state.set_active_workspace_index(&engine, 0);
    assert_eq!(engine.workspaces().len(), 1);
    assert_eq!(
        engine.closed_items.len(),
        1,
        "사용자가 닫은 워크스페이스는 복원 스택에 기록한다"
    );

    let mut core = crate::adapters::ipc::handler::cli_entry_tests::test_core();
    handle(&mut core, &mut state, &mut engine, &intent);
    assert_eq!(
        engine.workspaces().len(),
        2,
        "복원이 워크스페이스를 되살려야 한다"
    );
    assert_eq!(engine.closed_items.len(), 0);
    let tab = &engine
        .workspace_at(1)
        .expect("workspace index is valid")
        .pane_layout()
        .first_pane()
        .unwrap()
        .tabs[0];
    let crate::model::SurfaceLayout::Split { node_id, .. } = tab.layout() else {
        panic!("undo must rebuild the split");
    };
    assert_eq!(state.navigation.split_hints.get(node_id), Some(&false));
    assert_eq!(
        engine.remote.presentation.split_hints.get(node_id),
        Some(&false)
    );
    state.active_workspace_index(&engine)
}

#[test]
fn a_shortcut_restore_moves_the_active_workspace_to_the_restored_one() {
    let intent = Intent::RestoreClosedItem.from_user_shortcut("restore_closed");
    assert_eq!(active_after_restoring_a_closed_workspace(intent), 1);
}

#[test]
fn an_agent_restore_leaves_the_active_workspace_alone() {
    let intent = Intent::RestoreClosedItem.from_agent_ipc();
    assert_eq!(active_after_restoring_a_closed_workspace(intent), 0);
}
