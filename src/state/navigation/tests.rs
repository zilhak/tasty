use super::*;
use crate::model::SplitDirection;
use tasty_core::{DomainEvent as E, Placement, Ratio, SplitSpec, SurfaceSpec};

fn state_fixture(
    ids: Vec<u32>,
    extra: Vec<E>,
) -> (
    crate::state::RequestContext,
    crate::runtime::engine_session::EngineSession,
) {
    let mut events = vec![E::CategoryCreated {
        id: 0,
        name: "normal".into(),
        index: 0,
    }];
    for (index, id) in ids.into_iter().enumerate() {
        events.push(E::WorkspaceCreated {
            id,
            name: format!("workspace {id}"),
            category: 0,
            index,
            pane: id * 10,
        });
        events.push(tab(id * 100, id * 10, 0, id * 1000));
    }
    events.extend(extra);
    crate::state::tests::test_state_from_model(crate::state::tests::test_model(events))
}
fn fixture(ids: Vec<u32>, extra: Vec<E>) -> crate::runtime::engine_session::EngineSession {
    state_fixture(ids, extra).1
}
fn tab(id: u32, pane: u32, index: usize, surface: u32) -> E {
    E::TabCreated {
        id,
        pane,
        index,
        name: "tab".into(),
        surface: SurfaceSpec {
            id: surface,
            kind: "empty".into(),
            data: None,
        },
    }
}
fn split(target: u32, id: u32) -> E {
    E::SurfaceSplit {
        target,
        surface: SurfaceSpec {
            id,
            kind: "empty".into(),
            data: None,
        },
        split: SplitSpec {
            direction: SplitDirection::Horizontal,
            ratio: Ratio::from_f32(0.5),
            placement: Placement::After,
        },
    }
}

#[test]
fn two_owners_keep_independent_live_ids_through_reorder_and_creation() {
    let before = fixture(vec![1, 2, 3], vec![]);
    let mut a = NavigationState::default();
    let mut b = NavigationState::default();
    a.reconcile(&before.core_state.workspaces());
    b.reconcile(&before.core_state.workspaces());
    b.select_workspace(&before.core_state.workspaces(), 3);
    let after = fixture(vec![4, 2, 3, 1], vec![]);
    a.reconcile(&after.core_state.workspaces());
    b.reconcile(&after.core_state.workspaces());
    assert_eq!(a.workspace_id(&after.core_state.workspaces()), Some(1));
    assert_eq!(b.workspace_id(&after.core_state.workspaces()), Some(3));
    assert_eq!(a.workspace_index(&after.core_state.workspaces()), 3);
    assert_eq!(b.workspace_index(&after.core_state.workspaces()), 2);
}

#[test]
fn deleting_an_earlier_tab_preserves_id_and_deleting_selection_uses_next_slot() {
    let extra = vec![
        tab(101, 10, 1, 1001),
        tab(102, 10, 2, 1002),
        tab(103, 10, 3, 1003),
    ];
    let before = fixture(vec![1], extra.clone());
    let mut nav = NavigationState::default();
    nav.select_tab(before.core_state.find_pane_by_id(10).unwrap(), 102);
    nav.reconcile(&before.core_state.workspaces());
    let mut changed = extra;
    changed.push(E::TabClosed { id: 100 });
    let after = fixture(vec![1], changed.clone());
    nav.reconcile(&after.core_state.workspaces());
    let pane = after.core_state.find_pane_by_id(10).unwrap();
    assert_eq!(nav.tab_id(pane), Some(102));
    assert_eq!(nav.tab_index(pane), 1);
    changed.push(E::TabClosed { id: 102 });
    let after = fixture(vec![1], changed);
    nav.reconcile(&after.core_state.workspaces());
    assert_eq!(
        nav.tab_id(after.core_state.find_pane_by_id(10).unwrap()),
        Some(103)
    );
}

#[test]
fn split_does_not_select_new_surface_and_closed_selection_falls_back() {
    let before = fixture(vec![1], vec![]);
    let mut nav = NavigationState::default();
    nav.reconcile(&before.core_state.workspaces());
    let split_view = fixture(vec![1], vec![split(1000, 1001)]);
    nav.reconcile(&split_view.core_state.workspaces());
    let tab = &split_view.core_state.find_pane_by_id(10).unwrap().tabs[0];
    assert_eq!(nav.surface_id(tab), Some(1000));
    assert!(nav.select_surface(tab, 1001));
    let after = fixture(
        vec![1],
        vec![split(1000, 1001), E::SurfaceClosed { id: 1001 }],
    );
    nav.reconcile(&after.core_state.workspaces());
    assert_eq!(
        nav.surface_id(&after.core_state.find_pane_by_id(10).unwrap().tabs[0]),
        Some(1000)
    );
}

#[test]
fn parked_navigation_reconciles_only_disappeared_selections() {
    let before = fixture(vec![1, 2, 3], vec![]);
    let mut nav = NavigationState::default();
    nav.select_workspace(&before.core_state.workspaces(), 2);
    nav.reconcile(&before.core_state.workspaces());
    for (ids, selected) in [(vec![2, 3], Some(2)), (vec![3], Some(3)), (vec![], None)] {
        let after = fixture(ids, vec![]);
        nav.reconcile(&after.core_state.workspaces());
        assert_eq!(nav.workspace_id(&after.core_state.workspaces()), selected);
    }
}

#[test]
fn pane_focus_is_independent_from_model_and_invalid_ids_are_rejected() {
    let mut ws = Workspace::new_with_terminal_marker(1, "workspace".into(), 10, 100, 1000);
    ws.pane_layout_mut().split_pane_in_place(
        10,
        SplitDirection::Horizontal,
        Pane::new_with_terminal_marker(11, 101, 1001),
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
fn all_tab_removal_positions_preserve_the_legacy_neighbour_policy_by_id() {
    for (selected, removed, expected) in [
        (101, 100, 101),
        (101, 102, 101),
        (101, 101, 102),
        (102, 102, 101),
    ] {
        let extra = vec![tab(101, 10, 1, 1001), tab(102, 10, 2, 1002)];
        let before = fixture(vec![1], extra.clone());
        let mut nav = NavigationState::default();
        nav.select_tab(before.core_state.find_pane_by_id(10).unwrap(), selected);
        nav.reconcile(&before.core_state.workspaces());
        let mut changed = extra;
        changed.push(E::TabClosed { id: removed });
        let after = fixture(vec![1], changed);
        nav.reconcile(&after.core_state.workspaces());
        assert_eq!(
            nav.tab_id(after.core_state.find_pane_by_id(10).unwrap()),
            Some(expected)
        );
    }
}

#[test]
fn moving_a_surface_repairs_the_source_and_replaces_the_selected_destination() {
    let before = fixture(vec![1, 2], vec![split(1000, 1001), split(2000, 2001)]);
    let mut nav = NavigationState::default();
    nav.reconcile(&before.core_state.workspaces());
    nav.select_surface(
        &before.core_state.find_pane_by_id(20).unwrap().tabs[0],
        2001,
    );
    let after = fixture(
        vec![1, 2],
        vec![
            split(1000, 1001),
            split(2000, 2001),
            E::SurfaceMoved {
                id: 1000,
                target: 2001,
                split: SplitSpec {
                    direction: SplitDirection::Horizontal,
                    ratio: Ratio::from_f32(0.5),
                    placement: Placement::After,
                },
            },
            E::SurfaceClosed { id: 2001 },
        ],
    );
    nav.remap_surface_selection(2001, 1000);
    nav.reconcile(&after.core_state.workspaces());
    assert_eq!(
        nav.surface_id(&after.core_state.find_pane_by_id(10).unwrap().tabs[0]),
        Some(1001)
    );
    assert_eq!(
        nav.surface_id(&after.core_state.find_pane_by_id(20).unwrap().tabs[0]),
        Some(1000)
    );
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
fn legacy_split_hints_follow_node_identity_through_extract_and_resplit() {
    use crate::model::{StructurePresentation, SurfaceLayout};
    let mut workspaces = [Workspace::new_with_terminal_marker(
        1,
        "workspace".into(),
        10,
        100,
        1000,
    )];
    let tab = &mut workspaces[0]
        .pane_layout_mut()
        .find_pane_mut(10)
        .unwrap()
        .tabs[0];
    tab.split_surface_by_id_generic(
        1000,
        SplitDirection::Horizontal,
        crate::model::SurfaceDescriptor::new(1001, "empty"),
    );
    let root = match tab.layout() {
        SurfaceLayout::Split { node_id, .. } => *node_id,
        _ => panic!("split"),
    };
    tab.split_surface_by_id_generic(
        1001,
        SplitDirection::Vertical,
        crate::model::SurfaceDescriptor::new(1002, "empty"),
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
    let (mut state, mut engine_session) = state_fixture(
        vec![1],
        vec![E::CategoryCreated {
            id: 2,
            name: "work".into(),
            index: 1,
        }],
    );
    let mut engine = engine_session.borrow_mut();
    let category = 2;
    let mut apply = |intent: UiIntent| {
        engine.persistence.dirty.clear();
        crate::intent::popup::handle(&mut state, &mut engine, &intent.from_user_menu("test"));
        assert!(engine.persistence.dirty.is_dirty());
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

#[test]
fn retired_split_metadata_is_reclaimed_by_the_current_projection() {
    let before = fixture(vec![1], vec![split(1000, 1001)]);
    let mut ids = Vec::new();
    before.core_state.find_pane_by_id(10).unwrap().tabs[0]
        .layout()
        .split_node_ids(&mut ids);
    assert_eq!(ids.len(), 1);
    let mut nav = NavigationState::default();
    nav.split_hints.insert(ids[0], false);
    let after = fixture(vec![1], vec![]);
    nav.reconcile(&after.core_state.workspaces());
    assert!(nav.split_hints.is_empty());
}

#[cfg(feature = "gui")]
#[test]
fn removed_category_presentation_is_reclaimed() {
    let (mut state, before) = state_fixture(
        vec![1],
        vec![E::CategoryCreated {
            id: 2,
            name: "services".into(),
            index: 1,
        }],
    );
    state.navigation.collapsed_categories.insert(2);
    state
        .tab_bar_scroll
        .insert(u32::MAX, crate::model::LogicalPx(12.0));
    assert!(before.core_state.categories().iter().any(|c| c.id == 2));
    let after = fixture(vec![1], vec![]);
    state.reconcile_presentation(&after.core_state);
    assert!(!state.navigation.collapsed_categories.contains(&2));
    assert!(!state.tab_bar_scroll.contains_key(&u32::MAX));
}
