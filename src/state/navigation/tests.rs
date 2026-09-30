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

#[test]
fn tab_navigation_reports_no_change_and_missing_targets_without_mutating_structure() {
    let mut pane = Pane::new_with_terminal_marker(1, 10, 100);
    pane.add_terminal_marker_tab(11, 101);
    pane.add_terminal_marker_tab(12, 102);
    let mut nav = NavigationState::default();
    assert_eq!(
        nav.goto_tab(&pane, 0),
        crate::model::TabSwitch::AlreadyActive
    );
    assert_eq!(nav.goto_tab(&pane, 2), crate::model::TabSwitch::Switched);
    assert_eq!(
        nav.goto_tab(&pane, 2),
        crate::model::TabSwitch::AlreadyActive
    );
    assert_eq!(
        nav.goto_tab(&pane, 9),
        crate::model::TabSwitch::OutOfRange { tabs: 3 }
    );
    assert_eq!(nav.tab_id(&pane), Some(12));
    assert_eq!(
        pane.tabs.iter().map(|t| t.id).collect::<Vec<_>>(),
        [10, 11, 12]
    );
}

#[test]
fn all_tab_removal_positions_preserve_the_legacy_neighbour_policy_by_id() {
    for (selected, removed, expected) in [(11, 0, 11), (11, 2, 11), (11, 1, 12), (12, 2, 11)] {
        let mut workspaces = vec![workspace(1)];
        let pane = workspaces[0].pane_layout_mut().find_pane_mut(10).unwrap();
        pane.tabs.clear();
        for tab in [10, 11, 12] {
            pane.add_terminal_marker_tab(tab, tab * 10);
        }
        let mut nav = NavigationState::default();
        nav.select_tab(pane, selected);
        nav.reconcile(&workspaces);
        workspaces[0]
            .pane_layout_mut()
            .find_pane_mut(10)
            .unwrap()
            .remove_tab(removed);
        nav.reconcile(&workspaces);
        assert_eq!(
            nav.tab_id(workspaces[0].pane_layout().first_pane().unwrap()),
            Some(expected)
        );
    }
}

#[test]
fn moving_a_surface_repairs_the_source_and_replaces_the_selected_destination() {
    let mut workspaces = vec![workspace(1), workspace(2)];
    for (ws, first, second) in [(&mut workspaces[0], 1000, 1001)] {
        ws.pane_layout_mut().find_pane_mut(10).unwrap().tabs[0].split_surface_by_id_generic(
            first,
            SplitDirection::Horizontal,
            Box::new(EmptySurface::new(second)),
        );
    }
    workspaces[1]
        .pane_layout_mut()
        .find_pane_mut(20)
        .unwrap()
        .tabs[0]
        .split_surface_by_id_generic(
            2000,
            SplitDirection::Horizontal,
            Box::new(EmptySurface::new(2001)),
        );
    let mut nav = NavigationState::default();
    nav.reconcile(&workspaces);
    nav.select_surface(
        &workspaces[1].pane_layout().first_pane().unwrap().tabs[0],
        2001,
    );
    let source = &mut workspaces[0]
        .pane_layout_mut()
        .find_pane_mut(10)
        .unwrap()
        .tabs[0];
    let (layout, moved) = source.take_layout().extract_surface(1000);
    source.put_layout(layout);
    workspaces[1]
        .pane_layout_mut()
        .find_pane_mut(20)
        .unwrap()
        .tabs[0]
        .layout_mut()
        .replace_surface(2001, moved.unwrap());
    nav.remap_surface_selection(2001, 1000);
    nav.reconcile(&workspaces);
    assert_eq!(
        nav.surface_id(&workspaces[0].pane_layout().first_pane().unwrap().tabs[0]),
        Some(1001)
    );
    assert_eq!(
        nav.surface_id(&workspaces[1].pane_layout().first_pane().unwrap().tabs[0]),
        Some(1000)
    );
}

#[test]
fn legacy_split_hints_follow_node_identity_through_extract_and_resplit() {
    use crate::model::{StructurePresentation, SurfaceLayout};
    let mut workspaces = vec![workspace(1)];
    let tab = &mut workspaces[0]
        .pane_layout_mut()
        .find_pane_mut(10)
        .unwrap()
        .tabs[0];
    tab.split_surface_by_id_generic(
        1000,
        SplitDirection::Horizontal,
        Box::new(EmptySurface::new(1001)),
    );
    let root = match tab.layout() {
        SurfaceLayout::Split { node_id, .. } => *node_id,
        _ => panic!("split"),
    };
    tab.split_surface_by_id_generic(
        1001,
        SplitDirection::Vertical,
        Box::new(EmptySurface::new(1002)),
    );
    let mut nav = NavigationState::default();
    nav.split_hints.insert(root, false);
    let mut before = Vec::new();
    tab.layout().split_node_ids(&mut before);
    let child = *before.iter().find(|id| **id != root).unwrap();
    nav.split_hints.insert(child, false);
    let (layout, moved) = tab.take_layout().extract_surface(1001);
    tab.put_layout(layout);
    tab.split_surface_by_id_generic(1000, SplitDirection::Vertical, moved.unwrap());
    let mut after = Vec::new();
    tab.layout().split_node_ids(&mut after);
    assert!(after.contains(&root));
    assert!(!after.contains(&child));
    let replacement = *after.iter().find(|id| **id != root).unwrap();
    assert_ne!(replacement, child);
    assert!(!nav.split_focus_second(root));
    assert!(
        nav.split_focus_second(replacement),
        "new split retains the legacy true hint"
    );
    nav.reconcile(&workspaces);
    assert!(
        !nav.split_hints.contains_key(&child),
        "retired split metadata is reclaimed"
    );
    let tree = workspaces[0].pane_layout().first_pane().unwrap().tabs[0]
        .layout()
        .to_tree_json_full(&nav);
    assert_eq!(tree["focus_second"], false);
    assert_eq!(tree["first"]["focus_second"], true);
}

#[cfg(feature = "gui")]
#[test]
fn category_user_intents_preserve_dirty_scheduling_and_agent_isolation() {
    use crate::intent::UiIntent;
    let (mut state, mut engine) = crate::state::tests::test_state();
    let category = engine.create_category("work").unwrap();
    let mut apply = |intent: UiIntent| {
        engine.layout_dirty.clear();
        crate::intent::popup::handle(&mut state, &mut engine, &intent.from_user_menu("test"));
        assert!(engine.layout_dirty.is_dirty());
    };
    apply(UiIntent::SetCategoryCollapsed {
        id: category,
        collapsed: true,
    });
    assert!(state.navigation.collapsed_categories.contains(&category));
    crate::intent::popup::handle(
        &mut state,
        &mut engine,
        &UiIntent::ToggleCategoryCollapsed { id: category }.from_user_menu("test"),
    );
    assert!(!state.navigation.collapsed_categories.contains(&category));
    crate::intent::popup::handle(
        &mut state,
        &mut engine,
        &UiIntent::ToggleAllCategoriesCollapsed.from_user_menu("test"),
    );
    assert!(
        engine
            .categories()
            .iter()
            .all(|c| state.navigation.collapsed_categories.contains(&c.id))
    );
    crate::intent::popup::handle(
        &mut state,
        &mut engine,
        &UiIntent::ToggleAllCategoriesCollapsed.from_agent_cli(),
    );
    assert!(
        engine
            .categories()
            .iter()
            .all(|c| state.navigation.collapsed_categories.contains(&c.id))
    );
    crate::intent::popup::handle(
        &mut state,
        &mut engine,
        &UiIntent::ToggleAllCategoriesCollapsed.from_user_menu("test"),
    );
    assert!(state.navigation.collapsed_categories.is_empty());
    crate::intent::popup::handle(
        &mut state,
        &mut engine,
        &UiIntent::SetCategoryCollapsed {
            id: 0,
            collapsed: true,
        }
        .from_user_menu("test"),
    );
    assert!(state.navigation.collapsed_categories.contains(&0));
    crate::intent::popup::handle(
        &mut state,
        &mut engine,
        &UiIntent::SetCategoryCollapsed {
            id: u32::MAX,
            collapsed: true,
        }
        .from_user_menu("test"),
    );
    assert!(!state.navigation.collapsed_categories.contains(&u32::MAX));
}

#[cfg(feature = "gui")]
#[test]
fn category_only_changes_round_trip_and_removed_presentation_is_reclaimed() {
    let (mut state, mut engine) = crate::state::tests::test_state();
    let category = engine.create_category("services").unwrap();
    state.navigation.collapsed_categories.insert(category);
    let saved =
        crate::core::layout_persistence::SavedLayout::capture(&mut engine, 0, &state.navigation);
    let (mut restored_state, mut restored_engine) = crate::state::tests::test_state();
    let restored = saved.restore(&mut restored_engine).expect("restore");
    restored_state
        .navigation
        .restore(&restored_engine.workspaces, &restored);
    assert!(
        restored_state
            .navigation
            .collapsed_categories
            .contains(&category)
    );
    restored_state
        .tab_bar_scroll
        .insert(u32::MAX, crate::model::LogicalPx(12.0));
    restored_engine.delete_category(category).unwrap();
    restored_state.reconcile_presentation(&restored_engine);
    assert!(
        !restored_state
            .navigation
            .collapsed_categories
            .contains(&category)
    );
    assert!(!restored_state.tab_bar_scroll.contains_key(&u32::MAX));
}
