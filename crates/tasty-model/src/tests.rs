use super::*;

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
    };
    let p2 = Pane {
        id: 2,
        tabs: vec![],
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
    };
    let p2 = Pane {
        id: 2,
        tabs: vec![],
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
    };
    let p2 = Pane {
        id: 2,
        tabs: vec![],
    };
    let p3 = Pane {
        id: 3,
        tabs: vec![],
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
    };
    let p2 = Pane {
        id: 2,
        tabs: vec![],
    };
    let p3 = Pane {
        id: 3,
        tabs: vec![],
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
    };
    let p2 = Pane {
        id: 2,
        tabs: vec![],
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
    };
    let mut node = PaneNode::Leaf(p1);

    let new_pane = Pane {
        id: 2,
        tabs: vec![],
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
    };
    let mut node = PaneNode::Leaf(p1);

    let new_pane = Pane {
        id: 2,
        tabs: vec![],
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
    assert!(pane.close_tab(0));
    assert_eq!(pane.tabs.len(), 1);
}

#[test]
fn pane_add_variants_append_structure_without_owning_a_selection() {
    let mut pane = Pane::new_with_terminal_marker(1, 10, 100);
    pane.add_surface_tab_background(11, "bg".into(), None, SurfaceDescriptor::new(101, "empty"));
    assert_eq!(pane.tabs.len(), 2);
    assert_eq!(pane.tabs[0].id, 10);
    pane.add_surface_tab(12, "fg".into(), None, SurfaceDescriptor::new(102, "empty"));
    assert_eq!(pane.tabs.len(), 3);
    assert_eq!(pane.tabs[2].id, 12);
}

#[test]
fn pane_close_tab_last_tab_fails() {
    let pane = Pane::new_with_terminal_marker(1, 10, 100);
    assert_eq!(pane.tabs.len(), 1);
    let mut pane = pane;
    assert!(!pane.close_tab(0));
    assert_eq!(pane.tabs.len(), 1);
}

// ---- Close pane tests ----

#[test]
fn pane_node_close_pane_single_leaf_fails() {
    let p1 = Pane {
        id: 1,
        tabs: vec![],
    };
    let mut node = PaneNode::Leaf(p1);
    assert!(!node.close_pane(1));
}

#[test]
fn pane_node_close_pane_promotes_sibling() {
    let p1 = Pane {
        id: 1,
        tabs: vec![],
    };
    let p2 = Pane {
        id: 2,
        tabs: vec![],
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
    };
    let p2 = Pane {
        id: 2,
        tabs: vec![],
    };
    let p3 = Pane {
        id: 3,
        tabs: vec![],
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
    };
    let p2 = Pane {
        id: 2,
        tabs: vec![],
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

fn empty_pane(id: u32) -> Pane {
    Pane { id, tabs: vec![] }
}

#[test]
fn detach_leaf_returns_pane_and_promotes_sibling() {
    let mut node = PaneNode::Split {
        direction: SplitDirection::Vertical,
        ratio: 0.5,
        first: Box::new(PaneNode::Leaf(empty_pane(1))),
        second: Box::new(PaneNode::Leaf(empty_pane(2))),
    };
    let detached = node.detach_pane(1).expect("pane 1 detached");
    assert_eq!(detached.id, 1);
    assert!(matches!(&node, PaneNode::Leaf(p) if p.id == 2));
}

#[test]
fn detach_nested_leaf_keeps_outer_split() {
    let mut node = PaneNode::Split {
        direction: SplitDirection::Vertical,
        ratio: 0.3,
        first: Box::new(PaneNode::Leaf(empty_pane(1))),
        second: Box::new(PaneNode::Split {
            direction: SplitDirection::Horizontal,
            ratio: 0.5,
            first: Box::new(PaneNode::Leaf(empty_pane(2))),
            second: Box::new(PaneNode::Leaf(empty_pane(3))),
        }),
    };
    assert_eq!(node.detach_pane(3).map(|p| p.id), Some(3));
    assert_eq!(node.all_pane_ids(), vec![1, 2]);
    assert!(matches!(node, PaneNode::Split { ratio, .. } if ratio == 0.3));
}

#[test]
fn detach_root_leaf_is_refused() {
    let mut node = PaneNode::Leaf(empty_pane(1));
    assert!(node.detach_pane(1).is_none());
    assert!(matches!(&node, PaneNode::Leaf(p) if p.id == 1));
}

#[test]
fn detach_missing_pane_leaves_tree_unchanged() {
    let mut node = PaneNode::Split {
        direction: SplitDirection::Vertical,
        ratio: 0.5,
        first: Box::new(PaneNode::Leaf(empty_pane(1))),
        second: Box::new(PaneNode::Leaf(empty_pane(2))),
    };
    assert!(node.detach_pane(99).is_none());
    assert_eq!(node.all_pane_ids(), vec![1, 2]);
}

#[test]
fn replace_leaf_keeps_split_ratio() {
    let mut node = PaneNode::Split {
        direction: SplitDirection::Vertical,
        ratio: 0.3,
        first: Box::new(PaneNode::Leaf(empty_pane(1))),
        second: Box::new(PaneNode::Leaf(empty_pane(2))),
    };
    let Ok(replaced) = node.replace_pane(2, empty_pane(9)) else {
        panic!("pane 2 must be replaced");
    };
    assert_eq!(replaced.id, 2);
    assert_eq!(node.all_pane_ids(), vec![1, 9]);
    assert!(matches!(node, PaneNode::Split { ratio, .. } if ratio == 0.3));
    let missing = node.replace_pane(42, empty_pane(10));
    assert!(matches!(missing, Err(p) if p.id == 10));
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

fn test_surface_node(id: SurfaceId) -> SurfaceDescriptor {
    SurfaceDescriptor::new(id, "terminal")
}

#[test]
fn surface_layout_all_surface_ids_single() {
    let node = test_surface_node(10);
    let layout = SurfaceLayout::Leaf(node);
    assert_eq!(layout.all_surface_ids(), vec![10]);
}

#[test]
fn surface_layout_all_surface_ids_split() {
    let node1 = test_surface_node(10);
    let node2 = test_surface_node(20);
    let layout = SurfaceLayout::Leaf(node1);
    let (layout, leftover) = layout.split_with_surface(10, SplitDirection::Vertical, node2);
    assert!(leftover.is_none(), "split should succeed");
    let ids = layout.all_surface_ids();
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&10));
    assert!(ids.contains(&20));
}

#[test]
fn surface_layout_split_with_surface_success() {
    let node1 = test_surface_node(10);
    let node2 = test_surface_node(20);
    let layout = SurfaceLayout::Leaf(node1);
    let (new_layout, leftover) = layout.split_with_surface(10, SplitDirection::Vertical, node2);
    assert!(leftover.is_none(), "node should be consumed on success");
    assert_eq!(new_layout.all_surface_ids().len(), 2);
}

#[test]
fn surface_layout_split_nonexistent_target() {
    let node1 = test_surface_node(10);
    let node2 = test_surface_node(20);
    let layout = SurfaceLayout::Leaf(node1);
    // Target 999 doesn't exist — new_node is returned back
    let (new_layout, leftover) = layout.split_with_surface(999, SplitDirection::Vertical, node2);
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
    let layout = SurfaceLayout::Leaf(node1);
    let (layout, _) = layout.split_with_surface(10, SplitDirection::Vertical, node2);
    let (new_layout, removed) = layout.close_surface(10);
    assert!(removed, "surface 10 should be removed");
    assert_eq!(new_layout.all_surface_ids(), vec![20]);
}

#[test]
fn surface_layout_close_surface_split_second() {
    let node1 = test_surface_node(10);
    let node2 = test_surface_node(20);
    let layout = SurfaceLayout::Leaf(node1);
    let (layout, _) = layout.split_with_surface(10, SplitDirection::Vertical, node2);
    let (new_layout, removed) = layout.close_surface(20);
    assert!(removed, "surface 20 should be removed");
    assert_eq!(new_layout.all_surface_ids(), vec![10]);
}

#[test]
fn surface_layout_close_single_surface_fails() {
    let node = test_surface_node(10);
    let layout = SurfaceLayout::Leaf(node);
    let (new_layout, removed) = layout.close_surface(10);
    assert!(!removed, "cannot close the only surface");
    assert_eq!(new_layout.all_surface_ids(), vec![10]);
}

#[test]
fn surface_layout_close_nonexistent_surface() {
    let node = test_surface_node(10);
    let layout = SurfaceLayout::Leaf(node);
    let (new_layout, removed) = layout.close_surface(999);
    assert!(!removed, "999 does not exist");
    assert_eq!(new_layout.all_surface_ids(), vec![10]);
}

#[test]
fn surface_layout_find_terminal() {
    let node = test_surface_node(10);
    let layout = SurfaceLayout::Leaf(node);
    assert!(layout.find_surface(10).is_some());
    assert!(layout.find_surface(999).is_none());
}

#[test]
fn surface_layout_find_terminal_in_split() {
    let node1 = test_surface_node(10);
    let node2 = test_surface_node(20);
    let layout = SurfaceLayout::Leaf(node1);
    let (layout, _) = layout.split_with_surface(10, SplitDirection::Vertical, node2);
    assert!(layout.find_surface(10).is_some());
    assert!(layout.find_surface(20).is_some());
    assert!(layout.find_surface(99).is_none());
}

#[test]
fn tab_close_surface_in_split() {
    let node1 = test_surface_node(10);
    let node2 = test_surface_node(20);
    let layout = SurfaceLayout::Leaf(node1);
    let (split_layout, _) = layout.split_with_surface(10, SplitDirection::Vertical, node2);
    let mut tab = Tab {
        id: 1,
        name: "Test".to_string(),
        explicit_name: None,
        layout_opt: Some(split_layout),
    };
    let closed = tab.close_surface(10);
    assert!(closed);
    assert_eq!(tab.layout().all_surface_ids(), vec![20]);
    assert!(tab.surface(10).is_none());
    assert_eq!(tab.surface(20).unwrap().surface_id(), Some(20));
}

#[test]
fn surface_layout_all_surface_ids_three_way() {
    let n1 = test_surface_node(1);
    let n2 = test_surface_node(2);
    let n3 = test_surface_node(3);
    let layout = SurfaceLayout::Leaf(n1);
    let (layout, _) = layout.split_with_surface(1, SplitDirection::Vertical, n2);
    let (layout, _) = layout.split_with_surface(2, SplitDirection::Horizontal, n3);
    let ids = layout.all_surface_ids();
    assert_eq!(ids.len(), 3);
    assert!(ids.contains(&1));
    assert!(ids.contains(&2));
    assert!(ids.contains(&3));
}

#[test]
fn surface_layout_to_tree_json_full_preserves_split_ratio() {
    let node1 = test_surface_node(10);
    let node2 = test_surface_node(20);
    let layout = SurfaceLayout::Leaf(node1);
    let (layout, _) = layout.split_with_surface(10, SplitDirection::Horizontal, node2);
    let json = layout.to_tree_json_full(&crate::StructurePresentationSnapshot::default());
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
        })),
        second: Box::new(PaneNode::Leaf(Pane {
            id: 2,
            tabs: vec![],
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
