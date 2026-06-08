use super::*;
use std::sync::Arc;
use tasty_terminal::Waker;

fn noop_waker() -> Waker {
    Arc::new(|| {})
}

/// Helper to create a PhysicalPx value concisely in tests.
fn px(v: f32) -> PhysicalPx {
    PhysicalPx(v)
}

/// Helper to create a LogicalPx value concisely in tests.
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
    let (r1, r2) = r.split_with_gap(SplitDirection::Vertical, 0.5, PANE_BORDER_WIDTH);
    let gap = PANE_BORDER_WIDTH;
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
    let (r1, r2) = r.split_with_gap(SplitDirection::Horizontal, 0.5, PANE_BORDER_WIDTH);
    let gap = PANE_BORDER_WIDTH;
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
    let (r1, r2) = r.split_with_gap(SplitDirection::Vertical, 0.3, PANE_BORDER_WIDTH);
    let gap = PANE_BORDER_WIDTH;
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
    let rects = node.compute_rects(rect);
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
    let rects = node.compute_rects(rect);
    assert_eq!(rects.len(), 2);
    assert_eq!(rects[0].0, 1);
    assert_eq!(rects[1].0, 2);
    let gap = PANE_BORDER_WIDTH;
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
    let result = node.find_divider_at(401.0, 300.0, rect, 5.0);
    assert!(result.is_some());
    assert_eq!(result.unwrap().direction, SplitDirection::Vertical);

    // Far from divider
    let result = node.find_divider_at(200.0, 300.0, rect, 5.0);
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
    // Cannot easily test with real terminals, but we can test the layout structure
    // This test validates the basic PhysicalRect-based lookup
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

// ---- workspace attach classification (단계 6) ----

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
    let r = super::compute_terminal_rect(px(1920.0), px(1080.0), lp(200.0), 1.0);
    assert_eq!(r.x, px(200.0));
    assert_eq!(r.y, px(0.0));
    assert_eq!(r.width, px(1720.0));
    assert_eq!(r.height, px(1080.0));
}

#[test]
fn compute_terminal_rect_with_scale() {
    let r = super::compute_terminal_rect(px(1920.0), px(1080.0), lp(100.0), 2.0);
    assert_eq!(r.x, px(200.0));
    assert_eq!(r.y, px(0.0));
    assert_eq!(r.width, px(1720.0));
    assert_eq!(r.height, px(1080.0));
}

#[test]
fn compute_terminal_rect_sidebar_clamped() {
    // Sidebar wider than surface should be clamped
    let r = super::compute_terminal_rect(px(100.0), px(100.0), lp(200.0), 1.0);
    assert_eq!(r.x, px(99.0));
    assert_eq!(r.width, px(1.0));
}

#[test]
fn compute_terminal_rect_zero_sidebar() {
    let r = super::compute_terminal_rect(px(800.0), px(600.0), lp(0.0), 1.5);
    assert_eq!(r.x, px(0.0));
    assert_eq!(r.width, px(800.0));
}

// ---- Surface::source_cwd ----

#[test]
fn source_cwd_markdown_returns_parent_dir() {
    #[cfg(windows)]
    let (file, parent) = ("C:\\docs\\readme.md", "C:\\docs");
    #[cfg(not(windows))]
    let (file, parent) = ("/docs/readme.md", "/docs");
    let md = MarkdownPanel::new(1, file.to_string());
    assert_eq!(md.source_cwd(), Some(std::path::PathBuf::from(parent)));
}

#[test]
fn source_cwd_image_is_none() {
    let img = ImagePanel::new_blank(1);
    assert_eq!(img.source_cwd(), None);
}

#[test]
fn source_cwd_empty_surface_is_none() {
    let e = EmptySurface::new(1);
    assert_eq!(e.source_cwd(), None);
}

// ---- AttachedSurface tests (attach/detach 단계 3) ----

#[test]
fn attached_surface_placeholder_kind_and_holder() {
    let s = AttachedSurface::placeholder(7, 42);
    assert_eq!(s.kind(), "attached");
    assert_eq!(s.surface_id(), Some(7));
    assert!(s.is_placeholder());
    assert!(!s.is_mirror());
    assert_eq!(s.holder(), Some(42));
    assert_eq!(s.source_cwd(), None);
}

#[test]
fn attached_surface_mirror_role() {
    let s = AttachedSurface::mirror(3, 99, 1);
    assert_eq!(s.kind(), "attached");
    assert!(s.is_mirror());
    assert!(!s.is_placeholder());
    assert_eq!(s.holder(), None);
}

#[test]
fn attached_surface_tree_json_role() {
    let p = AttachedSurface::placeholder(5, 8);
    let pj = p.to_tree_json();
    assert_eq!(pj["kind"], "attached");
    assert_eq!(pj["id"], 5);
    assert_eq!(pj["role"]["placeholder"]["holder"], 8);

    let m = AttachedSurface::mirror(5, 12, 2);
    let mj = m.to_tree_json();
    assert_eq!(mj["role"]["mirror"]["remote_surface_id"], 12);
    assert_eq!(mj["role"]["mirror"]["session"], 2);
}
