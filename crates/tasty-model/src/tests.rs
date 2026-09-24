use super::*;
use std::sync::Arc;
use tasty_terminal::Waker;

fn noop_waker() -> Waker {
    Arc::new(|| {})
}

fn px(v: f32) -> PhysicalPx {
    PhysicalPx(v)
}

fn lp(v: f32) -> LogicalPx {
    LogicalPx(v)
}

// ---- PhysicalRect tests ----

#[test]
fn rect_contains_inside() {
    let r = PhysicalRect {
        x: px(10.0),
        y: px(20.0),
        width: px(100.0),
        height: px(50.0),
    };
    assert!(r.contains(px(50.0), px(40.0)));
}

#[test]
fn rect_contains_at_origin() {
    let r = PhysicalRect {
        x: px(10.0),
        y: px(20.0),
        width: px(100.0),
        height: px(50.0),
    };
    assert!(r.contains(px(10.0), px(20.0)));
}

#[test]
fn rect_contains_outside_left() {
    let r = PhysicalRect {
        x: px(10.0),
        y: px(20.0),
        width: px(100.0),
        height: px(50.0),
    };
    assert!(!r.contains(px(5.0), px(40.0)));
}

#[test]
fn rect_contains_outside_bottom() {
    let r = PhysicalRect {
        x: px(10.0),
        y: px(20.0),
        width: px(100.0),
        height: px(50.0),
    };
    assert!(!r.contains(px(50.0), px(80.0)));
}

#[test]
fn rect_contains_at_boundary_exclusive() {
    let r = PhysicalRect {
        x: px(0.0),
        y: px(0.0),
        width: px(100.0),
        height: px(100.0),
    };
    // Right edge is exclusive
    assert!(!r.contains(px(100.0), px(50.0)));
    // Bottom edge is exclusive
    assert!(!r.contains(px(50.0), px(100.0)));
}

#[test]
fn rect_split_vertical() {
    let r = PhysicalRect {
        x: px(0.0),
        y: px(0.0),
        width: px(200.0),
        height: px(100.0),
    };
    let (r1, r2) = r.split_with_gap(
        SplitDirection::Vertical,
        0.5,
        PANE_BORDER_WIDTH.to_physical(1.0),
    );
    let gap = PANE_BORDER_WIDTH.to_physical(1.0);
    let usable = px(200.0) - gap;
    assert_eq!(r1.x, px(0.0));
    assert_eq!(r1.width, (usable * 0.5).floor());
    assert_eq!(r2.x, r1.width + gap);
    assert_eq!(r2.width, usable - r1.width);
    assert_eq!(r1.height, px(100.0));
    assert_eq!(r2.height, px(100.0));
}

#[test]
fn rect_split_horizontal() {
    let r = PhysicalRect {
        x: px(0.0),
        y: px(0.0),
        width: px(200.0),
        height: px(100.0),
    };
    let (r1, r2) = r.split_with_gap(
        SplitDirection::Horizontal,
        0.5,
        PANE_BORDER_WIDTH.to_physical(1.0),
    );
    let gap = PANE_BORDER_WIDTH.to_physical(1.0);
    let usable = px(100.0) - gap;
    assert_eq!(r1.y, px(0.0));
    assert_eq!(r1.height, (usable * 0.5).floor());
    assert_eq!(r2.y, r1.height + gap);
    assert_eq!(r2.height, usable - r1.height);
    assert_eq!(r1.width, px(200.0));
    assert_eq!(r2.width, px(200.0));
}

#[test]
fn rect_split_unequal_ratio() {
    let r = PhysicalRect {
        x: px(0.0),
        y: px(0.0),
        width: px(300.0),
        height: px(100.0),
    };
    let (r1, r2) = r.split_with_gap(
        SplitDirection::Vertical,
        0.3,
        PANE_BORDER_WIDTH.to_physical(1.0),
    );
    let gap = PANE_BORDER_WIDTH.to_physical(1.0);
    let usable = px(300.0) - gap;
    assert_eq!(r1.width, (usable * 0.3).floor());
    assert_eq!(r2.width, usable - r1.width);
    assert_eq!(r2.x, r1.width + gap);
}

#[test]
fn rect_approx_eq() {
    let r1 = PhysicalRect {
        x: px(10.0),
        y: px(20.0),
        width: px(100.0),
        height: px(50.0),
    };
    let r2 = PhysicalRect {
        x: px(10.5),
        y: px(20.3),
        width: px(100.2),
        height: px(50.1),
    };
    assert!(r1.approx_eq(&r2));
}

#[test]
fn rect_not_approx_eq() {
    let r1 = PhysicalRect {
        x: px(10.0),
        y: px(20.0),
        width: px(100.0),
        height: px(50.0),
    };
    let r2 = PhysicalRect {
        x: px(12.0),
        y: px(20.0),
        width: px(100.0),
        height: px(50.0),
    };
    assert!(!r1.approx_eq(&r2));
}

// ---- PaneNode tests ----

#[test]
fn pane_node_compute_rects_single() {
    let pane = Pane {
        id: 1,
        tabs: vec![],
        active_tab: 0,
        tab_scroll_offset: 0.0,
    };
    let node = PaneNode::Leaf(pane);
    let rect = PhysicalRect {
        x: px(0.0),
        y: px(0.0),
        width: px(800.0),
        height: px(600.0),
    };
    let rects = node.compute_rects(rect, 1.0);
    assert_eq!(rects.len(), 1);
    assert_eq!(rects[0].0, 1);
    assert_eq!(rects[0].1.width, px(800.0));
}

#[test]
fn pane_node_compute_rects_split() {
    let p1 = Pane {
        id: 1,
        tabs: vec![],
        active_tab: 0,
        tab_scroll_offset: 0.0,
    };
    let p2 = Pane {
        id: 2,
        tabs: vec![],
        active_tab: 0,
        tab_scroll_offset: 0.0,
    };
    let node = PaneNode::Split {
        direction: SplitDirection::Vertical,
        ratio: 0.5,
        first: Box::new(PaneNode::Leaf(p1)),
        second: Box::new(PaneNode::Leaf(p2)),
    };
    let rect = PhysicalRect {
        x: px(0.0),
        y: px(0.0),
        width: px(800.0),
        height: px(600.0),
    };
    let rects = node.compute_rects(rect, 1.0);
    assert_eq!(rects.len(), 2);
    assert_eq!(rects[0].0, 1);
    assert_eq!(rects[1].0, 2);
    let gap = PANE_BORDER_WIDTH.to_physical(1.0);
    let usable = px(800.0) - gap;
    assert_eq!(rects[0].1.width, (usable * 0.5).floor());
    assert_eq!(rects[1].1.width, usable - rects[0].1.width);
}

#[test]
fn pane_node_find_pane() {
    let p1 = Pane {
        id: 1,
        tabs: vec![],
        active_tab: 0,
        tab_scroll_offset: 0.0,
    };
    let p2 = Pane {
        id: 2,
        tabs: vec![],
        active_tab: 0,
        tab_scroll_offset: 0.0,
    };
    let node = PaneNode::Split {
        direction: SplitDirection::Vertical,
        ratio: 0.5,
        first: Box::new(PaneNode::Leaf(p1)),
        second: Box::new(PaneNode::Leaf(p2)),
    };
    assert!(node.find_pane(1).is_some());
    assert!(node.find_pane(2).is_some());
    assert!(node.find_pane(99).is_none());
}

#[test]
fn pane_node_all_pane_ids() {
    let p1 = Pane {
        id: 1,
        tabs: vec![],
        active_tab: 0,
        tab_scroll_offset: 0.0,
    };
    let p2 = Pane {
        id: 2,
        tabs: vec![],
        active_tab: 0,
        tab_scroll_offset: 0.0,
    };
    let p3 = Pane {
        id: 3,
        tabs: vec![],
        active_tab: 0,
        tab_scroll_offset: 0.0,
    };
    let node = PaneNode::Split {
        direction: SplitDirection::Vertical,
        ratio: 0.5,
        first: Box::new(PaneNode::Leaf(p1)),
        second: Box::new(PaneNode::Split {
            direction: SplitDirection::Horizontal,
            ratio: 0.5,
            first: Box::new(PaneNode::Leaf(p2)),
            second: Box::new(PaneNode::Leaf(p3)),
        }),
    };
    assert_eq!(node.all_pane_ids(), vec![1, 2, 3]);
}

#[test]
fn pane_node_next_prev_pane_id() {
    let p1 = Pane {
        id: 1,
        tabs: vec![],
        active_tab: 0,
        tab_scroll_offset: 0.0,
    };
    let p2 = Pane {
        id: 2,
        tabs: vec![],
        active_tab: 0,
        tab_scroll_offset: 0.0,
    };
    let p3 = Pane {
        id: 3,
        tabs: vec![],
        active_tab: 0,
        tab_scroll_offset: 0.0,
    };
    let node = PaneNode::Split {
        direction: SplitDirection::Vertical,
        ratio: 0.5,
        first: Box::new(PaneNode::Leaf(p1)),
        second: Box::new(PaneNode::Split {
            direction: SplitDirection::Horizontal,
            ratio: 0.5,
            first: Box::new(PaneNode::Leaf(p2)),
            second: Box::new(PaneNode::Leaf(p3)),
        }),
    };
    assert_eq!(node.next_pane_id(1), 2);
    assert_eq!(node.next_pane_id(2), 3);
    assert_eq!(node.next_pane_id(3), 1); // wraps
    assert_eq!(node.prev_pane_id(1), 3); // wraps
    assert_eq!(node.prev_pane_id(2), 1);
}

#[test]
fn pane_node_find_divider_at_vertical() {
    let p1 = Pane {
        id: 1,
        tabs: vec![],
        active_tab: 0,
        tab_scroll_offset: 0.0,
    };
    let p2 = Pane {
        id: 2,
        tabs: vec![],
        active_tab: 0,
        tab_scroll_offset: 0.0,
    };
    let node = PaneNode::Split {
        direction: SplitDirection::Vertical,
        ratio: 0.5,
        first: Box::new(PaneNode::Leaf(p1)),
        second: Box::new(PaneNode::Leaf(p2)),
    };
    let rect = PhysicalRect {
        x: px(0.0),
        y: px(0.0),
        width: px(800.0),
        height: px(600.0),
    };
    // Divider should be at x=400
    let result = node.find_divider_at(401.0, 300.0, rect, 5.0, 1.0);
    assert!(result.is_some());
    assert_eq!(result.unwrap().direction, SplitDirection::Vertical);

    // Far from divider
    let result = node.find_divider_at(200.0, 300.0, rect, 5.0, 1.0);
    assert!(result.is_none());
}

#[test]
fn pane_node_split_pane_in_place() {
    let p1 = Pane {
        id: 1,
        tabs: vec![],
        active_tab: 0,
        tab_scroll_offset: 0.0,
    };
    let mut node = PaneNode::Leaf(p1);

    let new_pane = Pane {
        id: 2,
        tabs: vec![],
        active_tab: 0,
        tab_scroll_offset: 0.0,
    };
    let result = node.split_pane_in_place(1, SplitDirection::Vertical, new_pane);
    assert!(result.is_none()); // success

    let ids = node.all_pane_ids();
    assert_eq!(ids, vec![1, 2]);
}

#[test]
fn pane_node_split_pane_in_place_not_found() {
    let p1 = Pane {
        id: 1,
        tabs: vec![],
        active_tab: 0,
        tab_scroll_offset: 0.0,
    };
    let mut node = PaneNode::Leaf(p1);

    let new_pane = Pane {
        id: 2,
        tabs: vec![],
        active_tab: 0,
        tab_scroll_offset: 0.0,
    };
    let result = node.split_pane_in_place(99, SplitDirection::Vertical, new_pane);
    assert!(result.is_some()); // not found, pane returned

    let ids = node.all_pane_ids();
    assert_eq!(ids, vec![1]); // unchanged
}

// ---- Close tab tests ----

#[test]
fn pane_close_tab_removes_tab() {
    let mut pane = Pane::new_with_terminal_marker(1, 10, 100);
    pane.add_terminal_marker_tab_background(11, 101, None);
    assert_eq!(pane.tabs.len(), 2);
    assert!(pane.close_active_tab());
    assert_eq!(pane.tabs.len(), 1);
}

#[test]
fn pane_add_surface_tab_selects_it_and_the_background_variant_does_not() {
    use super::EmptySurface;
    let mut pane = Pane::new_with_terminal_marker(1, 10, 100);
    pane.add_surface_tab_background(11, "bg".into(), None, Box::new(EmptySurface::new(101)));
    assert_eq!(pane.tabs.len(), 2);
    assert_eq!(pane.active_tab, 0, "background tab keeps the selection");
    pane.add_surface_tab(12, "fg".into(), None, Box::new(EmptySurface::new(102)));
    assert_eq!(pane.tabs.len(), 3);
    assert_eq!(pane.active_tab, 2, "the ordinary variant selects its tab");
}

#[test]
fn pane_close_tab_last_tab_fails() {
    let pane = Pane::new_with_terminal_marker(1, 10, 100);
    assert_eq!(pane.tabs.len(), 1);
    let mut pane = pane;
    assert!(!pane.close_active_tab());
    assert_eq!(pane.tabs.len(), 1);
}

// ---- Close pane tests ----

#[test]
fn pane_node_close_pane_single_leaf_fails() {
    let p1 = Pane {
        id: 1,
        tabs: vec![],
        active_tab: 0,
        tab_scroll_offset: 0.0,
    };
    let mut node = PaneNode::Leaf(p1);
    assert!(!node.close_pane(1));
}

#[test]
fn pane_node_close_pane_promotes_sibling() {
    let p1 = Pane {
        id: 1,
        tabs: vec![],
        active_tab: 0,
        tab_scroll_offset: 0.0,
    };
    let p2 = Pane {
        id: 2,
        tabs: vec![],
        active_tab: 0,
        tab_scroll_offset: 0.0,
    };
    let mut node = PaneNode::Split {
        direction: SplitDirection::Vertical,
        ratio: 0.5,
        first: Box::new(PaneNode::Leaf(p1)),
        second: Box::new(PaneNode::Leaf(p2)),
    };

    // Close pane 1 -- pane 2 should be promoted
    assert!(node.close_pane(1));
    assert_eq!(node.all_pane_ids(), vec![2]);
}

#[test]
fn pane_node_close_pane_nested() {
    let p1 = Pane {
        id: 1,
        tabs: vec![],
        active_tab: 0,
        tab_scroll_offset: 0.0,
    };
    let p2 = Pane {
        id: 2,
        tabs: vec![],
        active_tab: 0,
        tab_scroll_offset: 0.0,
    };
    let p3 = Pane {
        id: 3,
        tabs: vec![],
        active_tab: 0,
        tab_scroll_offset: 0.0,
    };
    let mut node = PaneNode::Split {
        direction: SplitDirection::Vertical,
        ratio: 0.5,
        first: Box::new(PaneNode::Leaf(p1)),
        second: Box::new(PaneNode::Split {
            direction: SplitDirection::Horizontal,
            ratio: 0.5,
            first: Box::new(PaneNode::Leaf(p2)),
            second: Box::new(PaneNode::Leaf(p3)),
        }),
    };

    // Close pane 2 -- should promote pane 3 in the nested split
    assert!(node.close_pane(2));
    assert_eq!(node.all_pane_ids(), vec![1, 3]);
}

#[test]
fn pane_node_close_pane_not_found() {
    let p1 = Pane {
        id: 1,
        tabs: vec![],
        active_tab: 0,
        tab_scroll_offset: 0.0,
    };
    let p2 = Pane {
        id: 2,
        tabs: vec![],
        active_tab: 0,
        tab_scroll_offset: 0.0,
    };
    let mut node = PaneNode::Split {
        direction: SplitDirection::Vertical,
        ratio: 0.5,
        first: Box::new(PaneNode::Leaf(p1)),
        second: Box::new(PaneNode::Leaf(p2)),
    };
    assert!(!node.close_pane(99));
    assert_eq!(node.all_pane_ids(), vec![1, 2]);
}

// ---- SurfaceLayout tests ----

#[test]
fn surface_layout_find_surface_at() {
    let rect = PhysicalRect {
        x: px(0.0),
        y: px(0.0),
        width: px(100.0),
        height: px(100.0),
    };
    assert!(rect.contains(px(50.0), px(50.0)));
    assert!(!rect.contains(px(150.0), px(50.0)));
}

// ---- Visitor pattern tests ----

#[test]
fn pane_node_visits_single_pane() {
    let pane = Pane::new_with_terminal_marker(1, 1, 100);
    let node = PaneNode::Leaf(pane);
    let mut ids = Vec::new();
    for pid in node.all_pane_ids() {
        if let Some(p) = node.find_pane(pid) {
            ids.extend(p.all_surface_ids());
        }
    }
    assert_eq!(ids, vec![100]);
}

#[test]
fn pane_node_visits_split_panes() {
    let p1 = Pane::new_with_terminal_marker(1, 1, 101);
    let p2 = Pane::new_with_terminal_marker(2, 2, 102);
    let node = PaneNode::Split {
        direction: SplitDirection::Vertical,
        ratio: 0.5,
        first: Box::new(PaneNode::Leaf(p1)),
        second: Box::new(PaneNode::Leaf(p2)),
    };
    let mut ids = Vec::new();
    for pid in node.all_pane_ids() {
        if let Some(p) = node.find_pane(pid) {
            ids.extend(p.all_surface_ids());
        }
    }
    assert_eq!(ids, vec![101, 102]);
}

// ---- SurfaceLayout tests ----

fn test_surface_node(id: SurfaceId) -> TerminalSurface {
    TerminalSurface { id }
}

#[test]
fn surface_layout_all_surface_ids_single() {
    let node = test_surface_node(10);
    let layout = SurfaceLayout::Leaf(Box::new(node));
    assert_eq!(layout.all_surface_ids(), vec![10]);
}

#[test]
fn surface_layout_all_surface_ids_split() {
    let node1 = test_surface_node(10);
    let node2 = test_surface_node(20);
    let layout = SurfaceLayout::Leaf(Box::new(node1));
    let (layout, leftover) = layout.split_with_node(10, SplitDirection::Vertical, node2);
    assert!(leftover.is_none(), "split should succeed");
    let ids = layout.all_surface_ids();
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&10));
    assert!(ids.contains(&20));
}

#[test]
fn surface_layout_split_with_node_success() {
    let node1 = test_surface_node(10);
    let node2 = test_surface_node(20);
    let layout = SurfaceLayout::Leaf(Box::new(node1));
    let (new_layout, leftover) = layout.split_with_node(10, SplitDirection::Vertical, node2);
    assert!(leftover.is_none(), "node should be consumed on success");
    assert_eq!(new_layout.all_surface_ids().len(), 2);
}

#[test]
fn surface_layout_split_nonexistent_target() {
    let node1 = test_surface_node(10);
    let node2 = test_surface_node(20);
    let layout = SurfaceLayout::Leaf(Box::new(node1));
    // Target 999 doesn't exist — new_node is returned back
    let (new_layout, leftover) = layout.split_with_node(999, SplitDirection::Vertical, node2);
    assert!(
        leftover.is_some(),
        "node should be returned when target not found"
    );
    assert_eq!(new_layout.all_surface_ids(), vec![10]);
}

#[test]
fn surface_layout_close_surface_split_first() {
    let node1 = test_surface_node(10);
    let node2 = test_surface_node(20);
    let layout = SurfaceLayout::Leaf(Box::new(node1));
    let (layout, _) = layout.split_with_node(10, SplitDirection::Vertical, node2);
    let (new_layout, removed) = layout.close_surface(10);
    assert!(removed, "surface 10 should be removed");
    assert_eq!(new_layout.all_surface_ids(), vec![20]);
}

#[test]
fn surface_layout_close_surface_split_second() {
    let node1 = test_surface_node(10);
    let node2 = test_surface_node(20);
    let layout = SurfaceLayout::Leaf(Box::new(node1));
    let (layout, _) = layout.split_with_node(10, SplitDirection::Vertical, node2);
    let (new_layout, removed) = layout.close_surface(20);
    assert!(removed, "surface 20 should be removed");
    assert_eq!(new_layout.all_surface_ids(), vec![10]);
}

#[test]
fn surface_layout_close_single_surface_fails() {
    let node = test_surface_node(10);
    let layout = SurfaceLayout::Leaf(Box::new(node));
    let (new_layout, removed) = layout.close_surface(10);
    assert!(!removed, "cannot close the only surface");
    assert_eq!(new_layout.all_surface_ids(), vec![10]);
}

#[test]
fn surface_layout_close_nonexistent_surface() {
    let node = test_surface_node(10);
    let layout = SurfaceLayout::Leaf(Box::new(node));
    let (new_layout, removed) = layout.close_surface(999);
    assert!(!removed, "999 does not exist");
    assert_eq!(new_layout.all_surface_ids(), vec![10]);
}

#[test]
fn surface_layout_find_terminal() {
    let node = test_surface_node(10);
    let layout = SurfaceLayout::Leaf(Box::new(node));
    assert!(layout.find_surface(10).is_some());
    assert!(layout.find_surface(999).is_none());
}

#[test]
fn surface_layout_find_terminal_in_split() {
    let node1 = test_surface_node(10);
    let node2 = test_surface_node(20);
    let layout = SurfaceLayout::Leaf(Box::new(node1));
    let (layout, _) = layout.split_with_node(10, SplitDirection::Vertical, node2);
    assert!(layout.find_surface(10).is_some());
    assert!(layout.find_surface(20).is_some());
    assert!(layout.find_surface(99).is_none());
}

#[test]
fn tab_close_surface_in_split() {
    let node1 = test_surface_node(10);
    let node2 = test_surface_node(20);
    let layout = SurfaceLayout::Leaf(Box::new(node1));
    let (split_layout, _) = layout.split_with_node(10, SplitDirection::Vertical, node2);
    let mut tab = Tab {
        id: 1,
        name: "Test".to_string(),
        explicit_name: None,
        layout_opt: Some(split_layout),
        focused_surface: 10,
        osc_title: None,
        cached_display_name: None,
    };
    let closed = tab.close_surface(10);
    assert!(closed);
    assert_eq!(tab.layout().all_surface_ids(), vec![20]);
    // focused_surface should have been reset to the remaining surface
    assert_eq!(tab.focused_surface, 20);
}

#[test]
fn surface_layout_all_surface_ids_three_way() {
    let n1 = test_surface_node(1);
    let n2 = test_surface_node(2);
    let n3 = test_surface_node(3);
    let layout = SurfaceLayout::Leaf(Box::new(n1));
    let (layout, _) = layout.split_with_node(1, SplitDirection::Vertical, n2);
    let (layout, _) = layout.split_with_node(2, SplitDirection::Horizontal, n3);
    let ids = layout.all_surface_ids();
    assert_eq!(ids.len(), 3);
    assert!(ids.contains(&1));
    assert!(ids.contains(&2));
    assert!(ids.contains(&3));
}

// ---- Deferred placeholder tests ----

fn test_deferred_placeholder(id: SurfaceId) -> super::EmptySurface {
    let waker: tasty_terminal::Waker = std::sync::Arc::new(|| {});
    let spawn = super::terminal_surface::DeferredSpawn {
        shell: None,
        shell_args: Vec::new(),
        extra_env: Vec::new(),
        cols: 80,
        rows: 24,
        waker,
        working_dir: None,
        restore_command: None,
        scrollback_persist_id: None,
    };
    super::EmptySurface::new_deferred(id, spawn)
}

#[test]
fn tab_is_deferred_detects_placeholder_leaf() {
    let placeholder = test_deferred_placeholder(42);
    let tab = Tab {
        id: 1,
        name: "Shell".to_string(),
        explicit_name: None,
        layout_opt: Some(SurfaceLayout::Leaf(Box::new(placeholder))),
        focused_surface: 42,
        osc_title: None,
        cached_display_name: None,
    };
    assert!(tab.is_deferred());
    assert_eq!(tab.deferred_surface_ids(), vec![42]);
    assert!(tab.is_surface_deferred(42));
    assert!(!tab.is_surface_deferred(99));
}

#[test]
fn tab_is_deferred_walks_split_layout() {
    // Layout: Split(EmptySurface(deferred=Some, id=10), EmptySurface(deferred=Some, id=20))
    let p1 = test_deferred_placeholder(10);
    let p2 = test_deferred_placeholder(20);
    let layout = SurfaceLayout::Split {
        direction: SplitDirection::Vertical,
        ratio: 0.5,
        first: Box::new(SurfaceLayout::Leaf(Box::new(p1))),
        second: Box::new(SurfaceLayout::Leaf(Box::new(p2))),
        focus_second: false,
    };
    let tab = Tab {
        id: 1,
        name: "Shell".to_string(),
        explicit_name: None,
        layout_opt: Some(layout),
        focused_surface: 10,
        osc_title: None,
        cached_display_name: None,
    };
    assert!(tab.is_deferred());
    let ids = tab.deferred_surface_ids();
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&10));
    assert!(ids.contains(&20));
}

#[test]
fn tab_is_not_deferred_with_real_terminal() {
    let node = test_surface_node(7);
    let tab = Tab {
        id: 1,
        name: "Shell".to_string(),
        explicit_name: None,
        layout_opt: Some(SurfaceLayout::Leaf(Box::new(node))),
        focused_surface: 7,
        osc_title: None,
        cached_display_name: None,
    };
    assert!(!tab.is_deferred());
    assert_eq!(tab.deferred_surface_ids(), Vec::<SurfaceId>::new());
    assert!(!tab.is_surface_deferred(7));
}

#[test]
fn tab_ensure_initialized_replaces_placeholder_in_split() {
    let p1 = test_deferred_placeholder(11);
    let p2 = test_deferred_placeholder(12);
    let layout = SurfaceLayout::Split {
        direction: SplitDirection::Horizontal,
        ratio: 0.5,
        first: Box::new(SurfaceLayout::Leaf(Box::new(p1))),
        second: Box::new(SurfaceLayout::Leaf(Box::new(p2))),
        focus_second: false,
    };
    let mut tab = Tab {
        id: 1,
        name: "Shell".to_string(),
        explicit_name: None,
        layout_opt: Some(layout),
        focused_surface: 11,
        osc_title: None,
        cached_display_name: None,
    };
    // Only wake id=11. id=12 must remain deferred.
    let spawned = tab.ensure_initialized(11);
    assert!(spawned.is_some());
    assert!(!tab.is_surface_deferred(11));
    assert!(tab.is_surface_deferred(12));
    assert_eq!(tab.deferred_surface_ids(), vec![12]);
}

#[test]
fn workspace_classify_attach_surfaces_separates_terminal_and_non_terminal() {
    use super::{EmptySurface, Pane, Surface, TerminalSurface, Workspace};
    // 터미널(100) + split 으로 비-터미널 EmptySurface(200, 비-deferred) + deferred(300).
    let mut pane = Pane::new_with_surface(1, 1, "t".into(), Box::new(TerminalSurface { id: 100 }));
    pane.split_surface_by_id_with_surface(
        100,
        SplitDirection::Vertical,
        Box::new(EmptySurface::new(200)),
    )
    .unwrap();
    // deferred 터미널 자리(EmptySurface deferred) → 터미널로 분류돼야 한다.
    pane.split_surface_by_id_with_surface(
        200,
        SplitDirection::Horizontal,
        Box::new(test_deferred_placeholder(300)) as Box<dyn Surface>,
    )
    .unwrap();
    let ws = Workspace::new_with_pane(1, "w".into(), pane);
    let class = ws.classify_attach_surfaces();
    let mut terms = class.terminals.clone();
    terms.sort_unstable();
    assert_eq!(terms, vec![100, 300]); // 실 터미널 + deferred
    assert_eq!(class.non_terminals, vec![200]); // 비-deferred empty
    assert!(class.explorers.is_empty());
}

// Deferred::Plugin은 터미널 tap 대상이 아니라 placeholder로 분류한다.
#[test]
fn workspace_classify_attach_surfaces_puts_plugin_deferred_in_non_terminals() {
    use super::terminal_surface::DeferredPlugin;
    use super::{EmptySurface, Pane, Surface, TerminalSurface, Workspace};
    let mut pane = Pane::new_with_surface(1, 1, "t".into(), Box::new(TerminalSurface { id: 100 }));
    let plugin_ph = EmptySurface::new_deferred_plugin(
        300,
        DeferredPlugin {
            kind: "myplugin".into(),
            snapshot: serde_json::json!({}),
        },
    );
    pane.split_surface_by_id_with_surface(
        100,
        SplitDirection::Vertical,
        Box::new(plugin_ph) as Box<dyn Surface>,
    )
    .unwrap();
    let ws = Workspace::new_with_pane(1, "w".into(), pane);
    let class = ws.classify_attach_surfaces();
    assert_eq!(
        class.terminals,
        vec![100],
        "실 터미널만 terminal 이어야 한다"
    );
    assert_eq!(
        class.non_terminals,
        vec![300],
        "plugin deferred 는 non_terminal placeholder 여야 한다 (terminal 오분류 금지)"
    );
}

/// explorer는 별도 후보로 분류하고 고정 cwd가 아닌 활성 탭의 현재 경로를 전달한다.
#[test]
fn workspace_classify_attach_surfaces_puts_explorer_in_dedicated_bucket_with_active_root() {
    use super::{ExplorerPanel, Pane, Workspace};
    use std::path::PathBuf;

    let mut panel = ExplorerPanel::new(200, PathBuf::from("/proj"));
    panel
        .active_tab_mut()
        .navigate_to(PathBuf::from("/proj/sub"));
    let pane = Pane::new_with_surface(1, 1, "Explorer".into(), Box::new(panel));
    let ws = Workspace::new_with_pane(1, "w".into(), pane);
    let class = ws.classify_attach_surfaces();
    assert!(class.non_terminals.is_empty());
    assert_eq!(class.explorers, vec![(200, PathBuf::from("/proj/sub"))]);
}

/// 호스트 타입에 의존하지 않고 content mirror trait 계약만 재현하는 시험 surface.
struct ContentSurface {
    id: SurfaceId,
    kind: &'static str,
    plugin_id: String,
    file: Option<std::path::PathBuf>,
}

impl super::Surface for ContentSurface {
    crate::impl_surface_any!();

    fn kind(&self) -> &'static str {
        self.kind
    }

    fn type_name(&self) -> &'static str {
        "Remote"
    }

    fn surface_id(&self) -> Option<SurfaceId> {
        Some(self.id)
    }

    fn source_cwd(&self) -> Option<std::path::PathBuf> {
        None
    }

    fn attach_content_info(&self) -> Option<(&str, &str, Option<std::path::PathBuf>)> {
        Some((self.kind, self.plugin_id.as_str(), self.file.clone()))
    }
}

/// content 후보의 kind·plugin ID·경로를 보존한다. 최종 허용 목록은 호스트 책임이다.
#[test]
fn workspace_classify_attach_surfaces_puts_content_surface_in_dedicated_bucket() {
    use super::{Pane, Workspace};
    use std::path::PathBuf;

    let pane = Pane::new_with_surface(
        1,
        1,
        "README.md".into(),
        Box::new(ContentSurface {
            id: 400,
            kind: "markdown",
            plugin_id: "com.tasty.markdown".into(),
            file: Some(PathBuf::from("/proj/README.md")),
        }),
    );
    let ws = Workspace::new_with_pane(1, "w".into(), pane);
    let class = ws.classify_attach_surfaces();
    assert!(
        class.non_terminals.is_empty(),
        "content 후보가 placeholder 로 새면 안 된다"
    );
    assert_eq!(
        class.content_candidates,
        vec![(
            400,
            "markdown".to_string(),
            "com.tasty.markdown".to_string(),
            Some(PathBuf::from("/proj/README.md")),
        )]
    );
}

/// 두 신호가 모두 있으면 mesh 분류를 우선한다.
#[test]
fn workspace_classify_attach_surfaces_prefers_mesh_over_content() {
    use super::{Pane, Workspace};

    struct BothSurface(SurfaceId);
    impl super::Surface for BothSurface {
        crate::impl_surface_any!();
        fn kind(&self) -> &'static str {
            "image"
        }
        fn type_name(&self) -> &'static str {
            "EguiMesh"
        }
        fn surface_id(&self) -> Option<SurfaceId> {
            Some(self.0)
        }
        fn source_cwd(&self) -> Option<std::path::PathBuf> {
            None
        }
        fn attach_mesh_info(&self) -> Option<(&str, &str)> {
            Some(("image", "com.tasty.image"))
        }
        fn attach_content_info(&self) -> Option<(&str, &str, Option<std::path::PathBuf>)> {
            Some(("image", "com.tasty.image", None))
        }
    }

    let pane = Pane::new_with_surface(1, 1, "img".into(), Box::new(BothSurface(500)));
    let ws = Workspace::new_with_pane(1, "w".into(), pane);
    let class = ws.classify_attach_surfaces();
    assert_eq!(
        class.mesh_candidates,
        vec![(500, "image".to_string(), "com.tasty.image".to_string())]
    );
    assert!(
        class.content_candidates.is_empty(),
        "mesh 후보가 content 버킷으로 새면 mesh mirror 가 끊긴다"
    );
}

#[test]
fn surface_layout_to_tree_json_full_preserves_split_ratio() {
    let node1 = test_surface_node(10);
    let node2 = test_surface_node(20);
    let layout = SurfaceLayout::Leaf(Box::new(node1));
    let (layout, _) = layout.split_with_node(10, SplitDirection::Horizontal, node2);
    let json = layout.to_tree_json_full();
    assert_eq!(json["type"], "Split");
    assert_eq!(json["direction"], "horizontal");
    assert_eq!(json["ratio"], 0.5);
    assert_eq!(json["first"]["type"], "Leaf");
    assert_eq!(json["first"]["id"], 10);
    assert_eq!(json["second"]["id"], 20);
}

// ---- compute_terminal_rect tests ----

#[test]
fn compute_terminal_rect_basic() {
    let r = super::compute_terminal_rect(px(1920.0), px(1080.0), lp(200.0), px(0.0), px(0.0), 1.0);
    assert_eq!(r.x, px(200.0));
    assert_eq!(r.y, px(0.0));
    assert_eq!(r.width, px(1720.0));
    assert_eq!(r.height, px(1080.0));
}

#[test]
fn compute_terminal_rect_with_scale() {
    let r = super::compute_terminal_rect(px(1920.0), px(1080.0), lp(100.0), px(0.0), px(0.0), 2.0);
    assert_eq!(r.x, px(200.0));
    assert_eq!(r.y, px(0.0));
    assert_eq!(r.width, px(1720.0));
    assert_eq!(r.height, px(1080.0));
}

#[test]
fn compute_terminal_rect_sidebar_clamped() {
    // Sidebar wider than surface should be clamped
    let r = super::compute_terminal_rect(px(100.0), px(100.0), lp(200.0), px(0.0), px(0.0), 1.0);
    assert_eq!(r.x, px(99.0));
    assert_eq!(r.width, px(1.0));
}

#[test]
fn compute_terminal_rect_zero_sidebar() {
    let r = super::compute_terminal_rect(px(800.0), px(600.0), lp(0.0), px(0.0), px(0.0), 1.5);
    assert_eq!(r.x, px(0.0));
    assert_eq!(r.width, px(800.0));
}

#[test]
fn compute_terminal_rect_with_top_inset() {
    // Titlebar inset shifts the terminal down and shrinks its height.
    let r = super::compute_terminal_rect(px(1920.0), px(1080.0), lp(200.0), px(36.0), px(0.0), 1.0);
    assert_eq!(r.x, px(200.0));
    assert_eq!(r.y, px(36.0));
    assert_eq!(r.width, px(1720.0));
    assert_eq!(r.height, px(1044.0));
}

#[test]
fn compute_terminal_rect_with_bottom_inset() {
    // StatusBar inset shrinks the work-column height from the bottom; y is unchanged.
    let r =
        super::compute_terminal_rect(px(1920.0), px(1080.0), lp(200.0), px(36.0), px(24.0), 1.0);
    assert_eq!(r.x, px(200.0));
    assert_eq!(r.y, px(36.0));
    assert_eq!(r.width, px(1720.0));
    assert_eq!(r.height, px(1020.0));
}

// ---- Surface::source_cwd ----

#[test]
fn source_cwd_empty_surface_is_none() {
    let e = EmptySurface::new(1);
    assert_eq!(e.source_cwd(), None);
}

// ---- Pane::spawn_terminal — 셸 spawn 실패는 패닉이 아니라 Err ----

#[test]
fn spawn_terminal_with_a_missing_shell_returns_err_not_panic() {
    // 잘못된 셸 경로는 복구 가능한 오류로 반환해야 한다.
    let bogus = "/nonexistent/definitely/not/a/real/shell-xyzzy";
    let result = Pane::spawn_terminal(
        1,
        ShellSpawnOpts {
            cols: 80,
            rows: 24,
            shell: Some(bogus),
            shell_args: &[],
            waker: noop_waker(),
            working_dir: None,
            extra_env: &[],
        },
    );
    assert!(
        result.is_err(),
        "a missing shell path must return Err, not Ok or panic"
    );
}

// ---- 보더 상수의 좌표계 (docs/concepts/typed-length.md#두-타입) ----

/// 배율 1에서는 좌표계 오류가 드러나지 않아 배율 2도 비교한다.
/// pane은 논리 두께에 배율을 적용하고 surface는 물리 두께를 유지한다.
#[test]
fn border_constants_diverge_only_at_scale_two() {
    assert_eq!(
        <PaneNode as BinaryTree>::border_width(1.0),
        px(2.0),
        "배율 1 의 pane 보더"
    );
    assert_eq!(
        <SurfaceLayout as BinaryTree>::border_width(1.0),
        px(1.0),
        "배율 1 의 surface 보더"
    );

    assert_eq!(
        <PaneNode as BinaryTree>::border_width(2.0),
        px(4.0),
        "pane 보더는 논리라 배율을 탄다"
    );
    assert_eq!(
        <SurfaceLayout as BinaryTree>::border_width(2.0),
        px(1.0),
        "surface 보더는 hairline 이라 배율을 안 탄다"
    );
}

/// 상수 선언뿐 아니라 실제 레이아웃이 계산한 간격도 확인한다.
#[test]
fn pane_gap_in_computed_rects_follows_scale() {
    let node = PaneNode::Split {
        direction: SplitDirection::Vertical,
        ratio: 0.5,
        first: Box::new(PaneNode::Leaf(Pane {
            id: 1,
            tabs: vec![],
            active_tab: 0,
            tab_scroll_offset: 0.0,
        })),
        second: Box::new(PaneNode::Leaf(Pane {
            id: 2,
            tabs: vec![],
            active_tab: 0,
            tab_scroll_offset: 0.0,
        })),
    };
    let rect = PhysicalRect {
        x: px(0.0),
        y: px(0.0),
        width: px(800.0),
        height: px(600.0),
    };

    for (sf, expected_gap) in [(1.0_f32, 2.0_f32), (2.0, 4.0)] {
        let rects = node.compute_rects(rect, sf);
        assert_eq!(rects.len(), 2);
        let gap = rects[1].1.x - (rects[0].1.x + rects[0].1.width);
        assert_eq!(gap, px(expected_gap), "배율 {sf} 에서 pane 간격");
    }
}
