//! Reorder requests retain admitted source identity across earlier queued moves.
use super::*;
use crate::model::{EmptySurface, Pane, Workspace};

fn workspace(id: u32) -> Workspace {
    Workspace::new_with_pane(
        id,
        format!("ws-{id}"),
        Pane::new_with_surface(
            id * 10,
            id * 100,
            "tab".into(),
            Box::new(EmptySurface::new(id * 1000)),
        ),
    )
}

#[test]
fn queued_workspace_moves_keep_the_admitted_source_id() {
    let (mut state, mut engine_session) = crate::state::tests::test_state();
    let mut engine = engine_session.borrow_mut();
    let mut core = crate::adapters::ipc::handler::cli_entry_tests::test_core();
    engine.set_workspace_fixture(vec![workspace(11), workspace(22), workspace(33)]);
    state.reconcile_presentation(&engine);
    state.navigation.select_workspace(&engine.workspaces(), 33);
    let queue = [0, 1].map(|index| DomainIntent::MoveWorkspace {
        workspace_id: engine
            .workspace_at(index)
            .expect("workspace index is valid")
            .id,
        to_index: 2,
    });
    for request in queue {
        execute(&mut core, &mut state, &mut engine, request).unwrap();
    }
    assert_eq!(
        engine
            .workspaces()
            .into_iter()
            .map(|w| w.id)
            .collect::<Vec<_>>(),
        [33, 11, 22]
    );
    assert_eq!(
        state.navigation.workspace_id(&engine.workspaces()),
        Some(33)
    );
    let events = execute(
        &mut core,
        &mut state,
        &mut engine,
        DomainIntent::MoveWorkspace {
            workspace_id: 99,
            to_index: 0,
        },
    )
    .unwrap();
    assert!(matches!(
        events.as_slice(),
        [CoreEvent::WorkspaceMoved { moved: false }]
    ));
}

#[test]
fn queued_tab_moves_keep_identity_and_missing_sources_remain_noops() {
    let (mut state, mut engine_session) = crate::state::tests::test_state();
    let mut engine = engine_session.borrow_mut();
    let mut core = crate::adapters::ipc::handler::cli_entry_tests::test_core();
    engine.set_workspace_fixture(vec![workspace(11)]);
    let pane = engine
        .workspace_at_mut(0)
        .expect("workspace index is valid")
        .pane_layout_mut()
        .find_pane_mut(110)
        .unwrap();
    pane.add_surface_tab_background(
        2200,
        "second".into(),
        None,
        Box::new(EmptySurface::new(22000)),
    );
    pane.add_surface_tab_background(
        3300,
        "third".into(),
        None,
        Box::new(EmptySurface::new(33000)),
    );
    let queue = [0, 1].map(|index| DomainIntent::MoveTab {
        pane_id: pane.id,
        tab_id: pane.tabs[index].id,
        to_index: 2,
    });
    state.navigation.select_tab(pane, 3300);
    state.reconcile_presentation(&engine);
    for request in queue {
        execute(&mut core, &mut state, &mut engine, request).unwrap();
    }
    let pane = engine.find_pane_by_id(110).unwrap();
    assert_eq!(
        pane.tabs.iter().map(|t| t.id).collect::<Vec<_>>(),
        [3300, 1100, 2200]
    );
    assert_eq!(state.navigation.tab_id(pane), Some(3300));
    let before = pane.tabs.iter().map(|t| t.id).collect::<Vec<_>>();
    let events = execute(
        &mut core,
        &mut state,
        &mut engine,
        DomainIntent::MoveTab {
            pane_id: 110,
            tab_id: 9999,
            to_index: 0,
        },
    )
    .unwrap();
    assert!(matches!(
        events.as_slice(),
        [CoreEvent::TabMoved { moved: false }]
    ));
    let after = engine
        .find_pane_by_id(110)
        .unwrap()
        .tabs
        .iter()
        .map(|t| t.id)
        .collect::<Vec<_>>();
    assert_eq!(before, after);
}
