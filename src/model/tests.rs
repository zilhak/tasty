use super::*;
use std::sync::Arc;
use tasty_terminal::Waker;

fn noop_waker() -> Waker {
    Arc::new(|| {})
}

// ---- Rect tests ----

#[test]
fn rect_contains_inside() {
    let r = Rect { x: 10.0, y: 20.0, width: 100.0, height: 50.0 };
    assert!(r.contains(50.0, 40.0));
}

#[test]
fn rect_contains_at_origin() {
    let r = Rect { x: 10.0, y: 20.0, width: 100.0, height: 50.0 };
    assert!(r.contains(10.0, 20.0));
}

#[test]
fn rect_contains_outside_left() {
    let r = Rect { x: 10.0, y: 20.0, width: 100.0, height: 50.0 };
    assert!(!r.contains(5.0, 40.0));
}

#[test]
fn rect_contains_outside_bottom() {
    let r = Rect { x: 10.0, y: 20.0, width: 100.0, height: 50.0 };
    assert!(!r.contains(50.0, 80.0));
}

#[test]
fn rect_contains_at_boundary_exclusive() {
    let r = Rect { x: 0.0, y: 0.0, width: 100.0, height: 100.0 };
    // Right edge is exclusive
    assert!(!r.contains(100.0, 50.0));
    // Bottom edge is exclusive
    assert!(!r.contains(50.0, 100.0));
}

#[test]
fn rect_split_vertical() {
    let r = Rect { x: 0.0, y: 0.0, width: 200.0, height: 100.0 };
    let (r1, r2) = r.split(SplitDirection::Vertical, 0.5);
    let gap = PANE_BORDER_WIDTH;
    let usable = 200.0 - gap;
    assert_eq!(r1.x, 0.0);
    assert_eq!(r1.width, (usable * 0.5).floor());
    assert_eq!(r2.x, r1.width + gap);
    assert_eq!(r2.width, usable - r1.width);
    assert_eq!(r1.height, 100.0);
    assert_eq!(r2.height, 100.0);
}

#[test]
fn rect_split_horizontal() {
    let r = Rect { x: 0.0, y: 0.0, width: 200.0, height: 100.0 };
    let (r1, r2) = r.split(SplitDirection::Horizontal, 0.5);
    let gap = PANE_BORDER_WIDTH;
    let usable = 100.0 - gap;
    assert_eq!(r1.y, 0.0);
    assert_eq!(r1.height, (usable * 0.5).floor());
    assert_eq!(r2.y, r1.height + gap);
    assert_eq!(r2.height, usable - r1.height);
    assert_eq!(r1.width, 200.0);
    assert_eq!(r2.width, 200.0);
}

#[test]
fn rect_split_unequal_ratio() {
    let r = Rect { x: 0.0, y: 0.0, width: 300.0, height: 100.0 };
    let (r1, r2) = r.split(SplitDirection::Vertical, 0.3);
    let gap = PANE_BORDER_WIDTH;
    let usable = 300.0 - gap;
    assert_eq!(r1.width, (usable * 0.3).floor());
    assert_eq!(r2.width, usable - r1.width);
    assert_eq!(r2.x, r1.width + gap);
}

#[test]
fn rect_approx_eq() {
    let r1 = Rect { x: 10.0, y: 20.0, width: 100.0, height: 50.0 };
    let r2 = Rect { x: 10.5, y: 20.3, width: 100.2, height: 50.1 };
    assert!(r1.approx_eq(&r2));
}

#[test]
fn rect_not_approx_eq() {
    let r1 = Rect { x: 10.0, y: 20.0, width: 100.0, height: 50.0 };
    let r2 = Rect { x: 12.0, y: 20.0, width: 100.0, height: 50.0 };
    assert!(!r1.approx_eq(&r2));
}

// ---- PaneNode tests ----

#[test]
fn pane_node_compute_rects_single() {
    let pane = Pane {
        id: 1,
        tabs: vec![],
        active_tab: 0,
    };
    let node = PaneNode::Leaf(pane);
    let rect = Rect { x: 0.0, y: 0.0, width: 800.0, height: 600.0 };
    let rects = node.compute_rects(rect);
    assert_eq!(rects.len(), 1);
    assert_eq!(rects[0].0, 1);
    assert_eq!(rects[0].1.width, 800.0);
}

#[test]
fn pane_node_compute_rects_split() {
    let p1 = Pane { id: 1, tabs: vec![], active_tab: 0, tab_scroll_offset: 0.0 };
    let p2 = Pane { id: 2, tabs: vec![], active_tab: 0, tab_scroll_offset: 0.0 };
    let node = PaneNode::Split {
        direction: SplitDirection::Vertical,
        ratio: 0.5,
        first: Box::new(PaneNode::Leaf(p1)),
        second: Box::new(PaneNode::Leaf(p2)),
    };
    let rect = Rect { x: 0.0, y: 0.0, width: 800.0, height: 600.0 };
    let rects = node.compute_rects(rect);
    assert_eq!(rects.len(), 2);
    assert_eq!(rects[0].0, 1);
    assert_eq!(rects[1].0, 2);
    let gap = PANE_BORDER_WIDTH;
    let usable = 800.0 - gap;
    assert_eq!(rects[0].1.width, (usable * 0.5).floor());
    assert_eq!(rects[1].1.width, usable - rects[0].1.width);
}

#[test]
fn pane_node_find_pane() {
    let p1 = Pane { id: 1, tabs: vec![], active_tab: 0, tab_scroll_offset: 0.0 };
    let p2 = Pane { id: 2, tabs: vec![], active_tab: 0, tab_scroll_offset: 0.0 };
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
    let p1 = Pane { id: 1, tabs: vec![], active_tab: 0, tab_scroll_offset: 0.0 };
    let p2 = Pane { id: 2, tabs: vec![], active_tab: 0, tab_scroll_offset: 0.0 };
    let p3 = Pane { id: 3, tabs: vec![], active_tab: 0, tab_scroll_offset: 0.0 };
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
    let p1 = Pane { id: 1, tabs: vec![], active_tab: 0, tab_scroll_offset: 0.0 };
    let p2 = Pane { id: 2, tabs: vec![], active_tab: 0, tab_scroll_offset: 0.0 };
    let p3 = Pane { id: 3, tabs: vec![], active_tab: 0, tab_scroll_offset: 0.0 };
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
    let p1 = Pane { id: 1, tabs: vec![], active_tab: 0, tab_scroll_offset: 0.0 };
    let p2 = Pane { id: 2, tabs: vec![], active_tab: 0, tab_scroll_offset: 0.0 };
    let node = PaneNode::Split {
        direction: SplitDirection::Vertical,
        ratio: 0.5,
        first: Box::new(PaneNode::Leaf(p1)),
        second: Box::new(PaneNode::Leaf(p2)),
    };
    let rect = Rect { x: 0.0, y: 0.0, width: 800.0, height: 600.0 };
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
    let p1 = Pane { id: 1, tabs: vec![], active_tab: 0, tab_scroll_offset: 0.0 };
    let mut node = PaneNode::Leaf(p1);

    let new_pane = Pane { id: 2, tabs: vec![], active_tab: 0, tab_scroll_offset: 0.0 };
    let result = node.split_pane_in_place(1, SplitDirection::Vertical, new_pane);
    assert!(result.is_none()); // success

    let ids = node.all_pane_ids();
    assert_eq!(ids, vec![1, 2]);
}

#[test]
fn pane_node_split_pane_in_place_not_found() {
    let p1 = Pane { id: 1, tabs: vec![], active_tab: 0, tab_scroll_offset: 0.0 };
    let mut node = PaneNode::Leaf(p1);

    let new_pane = Pane { id: 2, tabs: vec![], active_tab: 0, tab_scroll_offset: 0.0 };
    let result = node.split_pane_in_place(99, SplitDirection::Vertical, new_pane);
    assert!(result.is_some()); // not found, pane returned

    let ids = node.all_pane_ids();
    assert_eq!(ids, vec![1]); // unchanged
}

// ---- Close tab tests ----

#[test]
fn pane_close_tab_removes_tab() {
    let waker = noop_waker();
    let mut pane = Pane::new_with_shell(1, 10, 100, 80, 24, None, &[], waker.clone(), None).expect("pane creation");
    pane.add_tab_with_shell(11, 101, 80, 24, None, &[], waker, None).expect("add tab");
    assert_eq!(pane.tabs.len(), 2);
    assert!(pane.close_active_tab());
    assert_eq!(pane.tabs.len(), 1);
}

#[test]
fn pane_close_tab_last_tab_fails() {
    let waker = noop_waker();
    let mut pane = Pane::new_with_shell(1, 10, 100, 80, 24, None, &[], waker, None).expect("pane creation");
    assert_eq!(pane.tabs.len(), 1);
    assert!(!pane.close_active_tab());
    assert_eq!(pane.tabs.len(), 1);
}

// ---- Close pane tests ----

#[test]
fn pane_node_close_pane_single_leaf_fails() {
    let p1 = Pane { id: 1, tabs: vec![], active_tab: 0, tab_scroll_offset: 0.0 };
    let mut node = PaneNode::Leaf(p1);
    assert!(!node.close_pane(1));
}

#[test]
fn pane_node_close_pane_promotes_sibling() {
    let p1 = Pane { id: 1, tabs: vec![], active_tab: 0, tab_scroll_offset: 0.0 };
    let p2 = Pane { id: 2, tabs: vec![], active_tab: 0, tab_scroll_offset: 0.0 };
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
    let p1 = Pane { id: 1, tabs: vec![], active_tab: 0, tab_scroll_offset: 0.0 };
    let p2 = Pane { id: 2, tabs: vec![], active_tab: 0, tab_scroll_offset: 0.0 };
    let p3 = Pane { id: 3, tabs: vec![], active_tab: 0, tab_scroll_offset: 0.0 };
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
    let p1 = Pane { id: 1, tabs: vec![], active_tab: 0, tab_scroll_offset: 0.0 };
    let p2 = Pane { id: 2, tabs: vec![], active_tab: 0, tab_scroll_offset: 0.0 };
    let mut node = PaneNode::Split {
        direction: SplitDirection::Vertical,
        ratio: 0.5,
        first: Box::new(PaneNode::Leaf(p1)),
        second: Box::new(PaneNode::Leaf(p2)),
    };
    assert!(!node.close_pane(99));
    assert_eq!(node.all_pane_ids(), vec![1, 2]);
}

// ---- SurfaceGroupLayout tests ----

#[test]
fn surface_group_layout_find_surface_at() {
    // Cannot easily test with real terminals, but we can test the layout structure
    // This test validates the basic Rect-based lookup
    let rect = Rect { x: 0.0, y: 0.0, width: 100.0, height: 100.0 };
    assert!(rect.contains(50.0, 50.0));
    assert!(!rect.contains(150.0, 50.0));
}

// ---- Visitor pattern tests ----

#[test]
fn for_each_terminal_visits_single_pane() {
    let waker = noop_waker();
    let pane = Pane::new_with_shell(1, 1, 100, 80, 24, None, &[], waker, None).unwrap();
    let mut node = PaneNode::Leaf(pane);
    let mut visited = Vec::new();
    node.for_each_terminal_mut(&mut |sid, _terminal| {
        visited.push(sid);
    });
    assert_eq!(visited, vec![100]);
}

#[test]
fn for_each_terminal_visits_split_panes() {
    let waker = noop_waker();
    let p1 = Pane::new_with_shell(1, 1, 101, 80, 24, None, &[], waker.clone(), None).unwrap();
    let p2 = Pane::new_with_shell(2, 2, 102, 80, 24, None, &[], waker, None).unwrap();
    let mut node = PaneNode::Split {
        direction: SplitDirection::Vertical,
        ratio: 0.5,
        first: Box::new(PaneNode::Leaf(p1)),
        second: Box::new(PaneNode::Leaf(p2)),
    };
    let mut visited = Vec::new();
    node.for_each_terminal_mut(&mut |sid, _terminal| {
        visited.push(sid);
    });
    assert_eq!(visited, vec![101, 102]);
}

#[test]
fn for_each_terminal_mut_can_modify() {
    let waker = noop_waker();
    let pane = Pane::new_with_shell(1, 1, 200, 80, 24, None, &[], waker, None).unwrap();
    let mut node = PaneNode::Leaf(pane);
    let mut count = 0u32;
    node.for_each_terminal_mut(&mut |_sid, terminal| {
        terminal.set_mark();
        count += 1;
    });
    assert_eq!(count, 1);
}

// ---- SurfaceGroupLayout tests ----

fn test_surface_node(id: SurfaceId) -> SurfaceNode {
    let waker: tasty_terminal::Waker = std::sync::Arc::new(|| {});
    let terminal = tasty_terminal::Terminal::new(80, 24, id, waker).unwrap();
    SurfaceNode { id, terminal, deferred_spawn: None }
}

#[test]
fn surface_group_all_surface_ids_single() {
    let node = test_surface_node(10);
    let layout = SurfaceGroupLayout::Single(node);
    assert_eq!(layout.all_surface_ids(), vec![10]);
}

#[test]
fn surface_group_all_surface_ids_split() {
    let node1 = test_surface_node(10);
    let node2 = test_surface_node(20);
    let layout = SurfaceGroupLayout::Single(node1);
    let (layout, leftover) = layout.split_with_node(10, SplitDirection::Vertical, node2);
    assert!(leftover.is_none(), "split should succeed");
    let ids = layout.all_surface_ids();
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&10));
    assert!(ids.contains(&20));
}

#[test]
fn surface_group_split_with_node_success() {
    let node1 = test_surface_node(10);
    let node2 = test_surface_node(20);
    let layout = SurfaceGroupLayout::Single(node1);
    let (new_layout, leftover) = layout.split_with_node(10, SplitDirection::Vertical, node2);
    assert!(leftover.is_none(), "node should be consumed on success");
    assert_eq!(new_layout.all_surface_ids().len(), 2);
}

#[test]
fn surface_group_split_nonexistent_target() {
    let node1 = test_surface_node(10);
    let node2 = test_surface_node(20);
    let layout = SurfaceGroupLayout::Single(node1);
    // Target 999 doesn't exist — new_node is returned back
    let (new_layout, leftover) = layout.split_with_node(999, SplitDirection::Vertical, node2);
    assert!(leftover.is_some(), "node should be returned when target not found");
    assert_eq!(new_layout.all_surface_ids(), vec![10]);
}

#[test]
fn surface_group_close_surface_split_first() {
    let node1 = test_surface_node(10);
    let node2 = test_surface_node(20);
    let layout = SurfaceGroupLayout::Single(node1);
    let (layout, _) = layout.split_with_node(10, SplitDirection::Vertical, node2);
    let (new_layout, removed) = layout.close_surface(10);
    assert!(removed, "surface 10 should be removed");
    assert_eq!(new_layout.all_surface_ids(), vec![20]);
}

#[test]
fn surface_group_close_surface_split_second() {
    let node1 = test_surface_node(10);
    let node2 = test_surface_node(20);
    let layout = SurfaceGroupLayout::Single(node1);
    let (layout, _) = layout.split_with_node(10, SplitDirection::Vertical, node2);
    let (new_layout, removed) = layout.close_surface(20);
    assert!(removed, "surface 20 should be removed");
    assert_eq!(new_layout.all_surface_ids(), vec![10]);
}

#[test]
fn surface_group_close_single_surface_fails() {
    let node = test_surface_node(10);
    let layout = SurfaceGroupLayout::Single(node);
    let (new_layout, removed) = layout.close_surface(10);
    assert!(!removed, "cannot close the only surface");
    assert_eq!(new_layout.all_surface_ids(), vec![10]);
}

#[test]
fn surface_group_close_nonexistent_surface() {
    let node = test_surface_node(10);
    let layout = SurfaceGroupLayout::Single(node);
    let (new_layout, removed) = layout.close_surface(999);
    assert!(!removed, "999 does not exist");
    assert_eq!(new_layout.all_surface_ids(), vec![10]);
}

#[test]
fn surface_group_find_terminal() {
    let node = test_surface_node(10);
    let layout = SurfaceGroupLayout::Single(node);
    assert!(layout.find_terminal(10).is_some());
    assert!(layout.find_terminal(999).is_none());
}

#[test]
fn surface_group_find_terminal_in_split() {
    let node1 = test_surface_node(10);
    let node2 = test_surface_node(20);
    let layout = SurfaceGroupLayout::Single(node1);
    let (layout, _) = layout.split_with_node(10, SplitDirection::Vertical, node2);
    assert!(layout.find_terminal(10).is_some());
    assert!(layout.find_terminal(20).is_some());
    assert!(layout.find_terminal(99).is_none());
}

#[test]
fn surface_group_node_close_surface_via_wrapper() {
    let node1 = test_surface_node(10);
    let node2 = test_surface_node(20);
    let layout = SurfaceGroupLayout::Single(node1);
    let (split_layout, _) = layout.split_with_node(10, SplitDirection::Vertical, node2);
    let mut group_node = SurfaceGroupNode {
        layout_opt: Some(split_layout),
        focused_surface: 10,
        _first_surface: 10,
    };
    let closed = group_node.close_surface(10);
    assert!(closed);
    assert_eq!(group_node.layout().all_surface_ids(), vec![20]);
    // focused_surface should have been reset to the remaining surface
    assert_eq!(group_node.focused_surface, 20);
}

#[test]
fn surface_group_all_surface_ids_three_way() {
    let n1 = test_surface_node(1);
    let n2 = test_surface_node(2);
    let n3 = test_surface_node(3);
    let layout = SurfaceGroupLayout::Single(n1);
    let (layout, _) = layout.split_with_node(1, SplitDirection::Vertical, n2);
    let (layout, _) = layout.split_with_node(2, SplitDirection::Horizontal, n3);
    let ids = layout.all_surface_ids();
    assert_eq!(ids.len(), 3);
    assert!(ids.contains(&1));
    assert!(ids.contains(&2));
    assert!(ids.contains(&3));
}

// ---- compute_terminal_rect tests ----

#[test]
fn compute_terminal_rect_basic() {
    let r = super::compute_terminal_rect(1920.0, 1080.0, 200.0, 1.0);
    assert_eq!(r.x, 200.0);
    assert_eq!(r.y, 0.0);
    assert_eq!(r.width, 1720.0);
    assert_eq!(r.height, 1080.0);
}

#[test]
fn compute_terminal_rect_with_scale() {
    let r = super::compute_terminal_rect(1920.0, 1080.0, 100.0, 2.0);
    assert_eq!(r.x, 200.0);
    assert_eq!(r.y, 0.0);
    assert_eq!(r.width, 1720.0);
    assert_eq!(r.height, 1080.0);
}

#[test]
fn compute_terminal_rect_sidebar_clamped() {
    // Sidebar wider than surface should be clamped
    let r = super::compute_terminal_rect(100.0, 100.0, 200.0, 1.0);
    assert_eq!(r.x, 99.0);
    assert_eq!(r.width, 1.0);
}

#[test]
fn compute_terminal_rect_zero_sidebar() {
    let r = super::compute_terminal_rect(800.0, 600.0, 0.0, 1.5);
    assert_eq!(r.x, 0.0);
    assert_eq!(r.width, 800.0);
}
