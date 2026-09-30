use super::*;
use crate::model::{Pane, PaneNode, PhysicalPx, Workspace};

fn split_workspace(id: u32, mirror: bool) -> Workspace {
    let mut workspace = Workspace::new_with_terminal_marker(id, "split".into(), 10, 100, 1000);
    *workspace.pane_layout_mut() = PaneNode::Split {
        direction: SplitDirection::Vertical,
        ratio: 0.5,
        first: Box::new(PaneNode::Leaf(Pane::new_with_terminal_marker(
            10, 100, 1000,
        ))),
        second: Box::new(PaneNode::Leaf(Pane::new_with_terminal_marker(
            20, 200, 2000,
        ))),
    };
    workspace.mirror = mirror;
    workspace
}
fn rect() -> PhysicalRect {
    PhysicalRect {
        x: PhysicalPx(0.0),
        y: PhysicalPx(0.0),
        width: PhysicalPx(1000.0),
        height: PhysicalPx(600.0),
    }
}
fn info() -> DividerInfo {
    DividerInfo {
        direction: SplitDirection::Vertical,
        split_rect: rect(),
    }
}
fn start(previews: &mut LayoutPreviews, core: &CoreState, workspace: u32) -> u64 {
    let ws = core
        .workspaces()
        .iter()
        .find(|candidate| candidate.id == workspace)
        .unwrap();
    previews
        .begin(
            core,
            LayoutTarget::Workspace(workspace),
            ws.pane_layout(),
            info(),
            rect(),
            1.0,
        )
        .unwrap()
}
fn width(previews: &LayoutPreviews, core: &CoreState, workspace: u32) -> f32 {
    let ws = core
        .workspaces()
        .iter()
        .find(|candidate| candidate.id == workspace)
        .unwrap();
    previews
        .geometry(
            core,
            LayoutTarget::Workspace(workspace),
            ws.pane_layout(),
            rect(),
            1.0,
        )
        .leaves[0]
        .1
        .width
        .value()
}

#[test]
fn preview_geometry_does_not_mutate_the_committed_tree_and_cancel_restores_geometry() {
    let (mut state, mut session) = crate::state::tests::test_state();
    let core = &mut session.core_state;
    core.set_workspace_fixture(vec![split_workspace(1, false)]);
    core.committed_structure_revision = Some(12);
    state.reconcile_presentation(core);
    let original = width(&state.layout_previews, core, 1);
    let sequence = start(&mut state.layout_previews, core, 1);
    assert!(state.layout_previews.update(core, sequence, 0.7));
    assert!(width(&state.layout_previews, core, 1) > original);
    assert_eq!(
        core.local_workspaces[0]
            .pane_layout()
            .split_parts()
            .unwrap()
            .1,
        0.5
    );
    let regions = state.surface_regions(core, rect(), 1.0);
    let pane_rects = state.pane_rects(core, &core.local_workspaces[0], rect(), 1.0);
    assert_eq!(regions[0].1.width, pane_rects[0].1.width);
    assert_eq!(regions[0].2[0].rect.width, pane_rects[0].1.width);
    let divider = state.pane_dividers(core, &core.local_workspaces[0], rect(), 1.0)[0];
    assert!(
        state
            .find_pane_divider_at(core, divider.x.value(), 10.0, rect(), 1.0)
            .is_some()
    );
    let commit = state.layout_previews.finish(core, sequence).unwrap();
    assert_eq!(commit.revision, 12);
    assert_eq!(commit.leaves, [10, 20]);
    assert_eq!(commit.ratio, 0.7);
    let newer = start(&mut state.layout_previews, core, 1);
    state.layout_previews.update(core, newer, 0.8);
    state.layout_previews.cancel(sequence);
    assert!(
        width(&state.layout_previews, core, 1) > original,
        "late reply only removes its own preview sequence"
    );
    state.layout_previews.cancel(newer);
    assert_eq!(width(&state.layout_previews, core, 1), original);
}

#[test]
fn a_new_projection_invalidates_the_drag_even_when_leaf_ids_are_identical() {
    let (_, mut session) = crate::state::tests::test_state();
    let core = &mut session.core_state;
    core.set_workspace_fixture(vec![split_workspace(1, false)]);
    core.committed_structure_revision = Some(5);
    let mut previews = LayoutPreviews::default();
    let sequence = start(&mut previews, core, 1);
    previews.update(core, sequence, 0.8);
    core.replace_local_workspaces(vec![split_workspace(1, false)]);
    core.committed_structure_revision = Some(7);
    assert!(!previews.update(core, sequence, 0.9));
    assert!(previews.finish(core, sequence).is_none());
    assert!(previews.entries.is_empty());
}

#[test]
fn cancelled_second_mirror_drag_preserves_the_first_until_remote_projection_replacement() {
    let (_, mut session) = crate::state::tests::test_state();
    let core = &mut session.core_state;
    core.set_workspace_fixture(vec![split_workspace(9, true)]);
    let mut previews = LayoutPreviews::default();
    let original = width(&previews, core, 9);
    let first = start(&mut previews, core, 9);
    assert!(previews.update(core, first, 0.6));
    assert!(
        previews.finish(core, first).is_none(),
        "mirror has no local journal ratio fact"
    );
    let completed = width(&previews, core, 9);
    assert!(completed > original);
    let second = start(&mut previews, core, 9);
    assert!(previews.update(core, second, 0.8));
    assert!(width(&previews, core, 9) > completed);
    previews.cancel(second);
    assert_eq!(width(&previews, core, 9), completed);
    assert_eq!(
        core.mirror_workspaces[0]
            .pane_layout()
            .split_parts()
            .unwrap()
            .1,
        0.5
    );
    assert!(
        core.replace_mirror_workspace(split_workspace(9, true))
            .is_ok()
    );
    previews.retain(core);
    assert_eq!(width(&previews, core, 9), original);
    assert!(previews.entries.is_empty());
}

#[test]
fn another_view_keeps_its_geometry_and_removed_target_releases_the_preview() {
    let (_, mut session) = crate::state::tests::test_state();
    let core = &mut session.core_state;
    core.set_workspace_fixture(vec![split_workspace(1, false)]);
    core.committed_structure_revision = Some(3);
    let mut first_view = LayoutPreviews::default();
    let second_view = LayoutPreviews::default();
    let original = width(&second_view, core, 1);
    let sequence = start(&mut first_view, core, 1);
    first_view.update(core, sequence, 0.8);
    assert!(width(&first_view, core, 1) > original);
    assert_eq!(width(&second_view, core, 1), original);
    drop(core.remove_workspace_at(0));
    assert!(first_view.finish(core, sequence).is_none());
    assert!(first_view.entries.is_empty());
}

#[test]
fn surface_preview_is_shared_by_hit_testing_dividers_and_move_source_mark() {
    use crate::model::{SurfaceLayout, TerminalSurface};
    let (mut state, mut session) = crate::state::tests::test_state();
    let core = &mut session.core_state;
    core.set_workspace_fixture(vec![split_workspace(1, false)]);
    core.committed_structure_revision = Some(8);
    core.find_pane_by_id_mut(10).unwrap().tabs[0].put_layout(SurfaceLayout::Split {
        direction: SplitDirection::Vertical,
        ratio: 0.5,
        first: Box::new(SurfaceLayout::Leaf(Box::new(TerminalSurface { id: 1000 }))),
        second: Box::new(SurfaceLayout::Leaf(Box::new(TerminalSurface { id: 1001 }))),
        node_id: crate::model::SplitNodeId::allocate(),
    });
    state.reconcile_presentation(core);
    let panes = state.pane_rects(core, &core.local_workspaces[0], rect(), 1.5);
    let content = PhysicalRect {
        y: panes[0].1.y + state.tab_bar_height,
        height: (panes[0].1.height - state.tab_bar_height).max(PhysicalPx(1.0)),
        ..panes[0].1
    };
    let tab = &core.find_pane_by_id(10).unwrap().tabs[0];
    let before = state.tab_surface_regions(core, tab, content, 1.5)[0].rect;
    let sequence = state
        .layout_previews
        .begin(
            core,
            LayoutTarget::Tab(tab.id),
            tab.layout(),
            DividerInfo {
                direction: SplitDirection::Vertical,
                split_rect: content,
            },
            content,
            1.5,
        )
        .unwrap();
    state.layout_previews.update(core, sequence, 0.8);
    let after = state.tab_surface_regions(core, tab, content, 1.5)[0].rect;
    assert!(after.width > before.width);
    let divider = state.surface_dividers(core, tab, content, 1.5)[0];
    assert_eq!(divider.x, after.x + after.width);
    assert!(
        state
            .find_surface_divider_at(
                core,
                divider.x.value(),
                content.y.value() + 10.0,
                rect(),
                1.5
            )
            .is_some()
    );
    core.pending_move = Some(crate::core::state::PendingMove::Surface(1000));
    let mark = crate::adapters::ui::move_source::resolve(
        &state.navigation,
        core,
        0,
        &panes,
        state.tab_bar_height,
        Some(&state.layout_previews),
        1.5,
    )
    .unwrap();
    assert!(
        matches!(mark, crate::adapters::ui::move_source::MoveSourceMark::Ring { rect: marked, .. } if marked.approx_eq(&after))
    );
    let x = before.x.value() + (before.width.value() + after.width.value()) / 2.0;
    state.focus_surface_at_position(core, x, content.y.value() + 10.0, rect(), 1.5);
    let tab = &core.find_pane_by_id(10).unwrap().tabs[0];
    assert_eq!(state.navigation.surface_id(tab), Some(1000));
    assert_eq!(tab.layout().split_parts().unwrap().1, 0.5);
}
