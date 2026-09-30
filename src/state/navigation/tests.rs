use super::*;
use crate::model::{EmptySurface, SplitDirection};

fn workspace(id: u32) -> Workspace {
    Workspace::new_with_pane(
        id,
        format!("workspace {id}"),
        Pane::new_with_surface(
            id * 10,
            id * 100,
            "first".into(),
            Box::new(EmptySurface::new(id * 1000)),
        ),
    )
}

#[test]
fn two_owners_keep_independent_live_ids_through_reorder_and_creation() {
    let mut workspaces = vec![workspace(1), workspace(2), workspace(3)];
    let mut a = NavigationState::default();
    let mut b = NavigationState::default();
    a.reconcile(&workspaces);
    b.reconcile(&workspaces);
    b.select_workspace(&workspaces, 3);
    workspaces.rotate_left(1);
    workspaces.insert(0, workspace(4));
    a.reconcile(&workspaces);
    b.reconcile(&workspaces);
    assert_eq!(a.workspace_id(&workspaces), Some(1));
    assert_eq!(b.workspace_id(&workspaces), Some(3));
    assert_eq!(a.workspace_index(&workspaces), 3);
    assert_eq!(b.workspace_index(&workspaces), 2);
}

#[test]
fn deleting_an_earlier_tab_preserves_id_and_deleting_selection_uses_next_slot() {
    let mut workspaces = vec![workspace(1)];
    let pane = workspaces[0].pane_layout_mut().find_pane_mut(10).unwrap();
    pane.add_surface_tab_background(
        101,
        "second".into(),
        None,
        Box::new(EmptySurface::new(1001)),
    );
    pane.add_surface_tab_background(102, "third".into(), None, Box::new(EmptySurface::new(1002)));
    pane.add_surface_tab_background(
        103,
        "fourth".into(),
        None,
        Box::new(EmptySurface::new(1003)),
    );
    let mut nav = NavigationState::default();
    nav.select_tab(pane, 102);
    nav.reconcile(&workspaces);
    workspaces[0]
        .pane_layout_mut()
        .find_pane_mut(10)
        .unwrap()
        .tabs
        .remove(0);
    nav.reconcile(&workspaces);
    let pane = workspaces[0].pane_layout_mut().find_pane_mut(10).unwrap();
    assert_eq!(nav.tab_id(pane), Some(102));
    assert_eq!(nav.tab_index(pane), 1);
    pane.tabs.remove(1);
    nav.reconcile(&workspaces);
    assert_eq!(
        nav.tab_id(workspaces[0].pane_layout().first_pane().unwrap()),
        Some(103)
    );
}

#[test]
fn split_does_not_select_new_surface_and_closed_selection_falls_back() {
    let mut workspaces = vec![workspace(1)];
    let mut nav = NavigationState::default();
    nav.reconcile(&workspaces);
    let pane = workspaces[0].pane_layout_mut().find_pane_mut(10).unwrap();
    pane.tabs[0].split_surface_by_id_generic(
        1000,
        SplitDirection::Horizontal,
        Box::new(EmptySurface::new(1001)),
    );
    nav.reconcile(&workspaces);
    let tab = &mut workspaces[0]
        .pane_layout_mut()
        .find_pane_mut(10)
        .unwrap()
        .tabs[0];
    assert_eq!(nav.surface_id(tab), Some(1000));
    assert!(nav.select_surface(tab, 1001));
    tab.close_surface(1001);
    nav.reconcile(&workspaces);
    assert_eq!(
        nav.surface_id(&workspaces[0].pane_layout().first_pane().unwrap().tabs[0]),
        Some(1000)
    );
}

#[test]
fn parked_navigation_reconciles_only_disappeared_selections() {
    let mut workspaces = vec![workspace(1), workspace(2), workspace(3)];
    let mut nav = NavigationState::default();
    nav.select_workspace(&workspaces, 2);
    nav.reconcile(&workspaces);
    workspaces.remove(0);
    nav.reconcile(&workspaces);
    assert_eq!(nav.workspace_id(&workspaces), Some(2));
    workspaces.remove(0);
    nav.reconcile(&workspaces);
    assert_eq!(nav.workspace_id(&workspaces), Some(3));
    workspaces.clear();
    nav.reconcile(&workspaces);
    assert_eq!(nav.workspace_id(&workspaces), None);
}

#[test]
fn pane_focus_is_independent_from_model_and_invalid_ids_are_rejected() {
    let mut ws = workspace(1);
    ws.pane_layout_mut().split_pane_in_place(
        10,
        SplitDirection::Horizontal,
        Pane::new_with_surface(11, 101, "second".into(), Box::new(EmptySurface::new(1001))),
    );
    let mut nav = NavigationState::default();
    assert_eq!(nav.pane_id(&ws), Some(10));
    assert!(nav.select_pane(&ws, 11));
    assert!(!nav.select_pane(&ws, 999));
    assert_eq!(nav.pane_id(&ws), Some(11));
    assert!(ws.pane_layout_mut().close_pane(11));
    assert_eq!(nav.pane_id(&ws), Some(10));
}

#[test]
fn snapshot_deleting_multiple_predecessors_uses_the_next_surviving_id() {
    let mut selection = Selection::default();
    selection.select(3, vec![1, 2, 3, 4, 5]);
    assert_eq!(selection.resolve(&[2, 4, 5]), Some(4));
    assert_eq!(selection.resolve(&[1, 2]), Some(2));
    assert_eq!(selection.resolve(&[6, 7]), Some(6));
}
