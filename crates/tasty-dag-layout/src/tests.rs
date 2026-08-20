//! [`layout_dag`] 의 계약 검증.
//!
//! 검증 대상은 좌표의 "예쁨" 이 아니라 렌더가 의존하는 **불변식**이다:
//! 레이어 단조성, 카드 비겹침, dummy 미누출, 입력 순서 보존, 엣지 폴리라인의
//! 꺾임점 개수와 회피, 방향 토글의 순서 보존, 그리고 어떤 입력에도 패닉하지 않음.

use std::collections::{HashMap, HashSet};

use tasty_type_geometry::length::LogicalPx;

use crate::{GraphLayout, LayoutConfig, NodePosition, Orientation, layout_dag};

// ── 헬퍼 ──

fn ids(n: usize) -> Vec<String> {
    (0..n).map(|i| format!("t{i}")).collect()
}

fn node<'a>(out: &'a GraphLayout, id: &str) -> &'a NodePosition {
    out.nodes
        .iter()
        .find(|n| n.id == id)
        .unwrap_or_else(|| panic!("node {id} missing"))
}

/// 노드 사각형 두 개가 실제로 겹치는지(변끼리 닿는 것은 겹침이 아니다).
fn overlaps(a: &NodePosition, b: &NodePosition, cfg: &LayoutConfig) -> bool {
    let (w, h) = (cfg.node_size.0.value(), cfg.node_size.1.value());
    let x_overlap = a.x.value() < b.x.value() + w && b.x.value() < a.x.value() + w;
    let y_overlap = a.y.value() < b.y.value() + h && b.y.value() < a.y.value() + h;
    x_overlap && y_overlap
}

/// 점이 노드 사각형 **안쪽**에 있는지(변 위는 아니다).
fn strictly_inside(point: (LogicalPx, LogicalPx), n: &NodePosition, cfg: &LayoutConfig) -> bool {
    let (w, h) = (cfg.node_size.0.value(), cfg.node_size.1.value());
    let (px, py) = (point.0.value(), point.1.value());
    px > n.x.value() && px < n.x.value() + w && py > n.y.value() && py < n.y.value() + h
}

/// 형제 축(레이어와 직교하는 축) 좌표.
fn cross_of(n: &NodePosition, orientation: Orientation) -> f32 {
    match orientation {
        Orientation::LeftRight => n.y.value(),
        Orientation::TopDown => n.x.value(),
    }
}

/// 레이어 축 좌표.
fn along_of(n: &NodePosition, orientation: Orientation) -> f32 {
    match orientation {
        Orientation::LeftRight => n.x.value(),
        Orientation::TopDown => n.y.value(),
    }
}

/// 레이어별 노드 id 를 형제 축 순서로 나열한다.
fn sibling_order(out: &GraphLayout, orientation: Orientation) -> HashMap<u32, Vec<String>> {
    let mut by_layer: HashMap<u32, Vec<&NodePosition>> = HashMap::new();
    for n in &out.nodes {
        by_layer.entry(n.layer).or_default().push(n);
    }
    by_layer
        .into_iter()
        .map(|(layer, mut nodes)| {
            nodes.sort_by(|a, b| cross_of(a, orientation).total_cmp(&cross_of(b, orientation)));
            (layer, nodes.into_iter().map(|n| n.id.clone()).collect())
        })
        .collect()
}

/// 다이아몬드 20 개를 이어붙인 중간 규모 그래프.
fn graph_20() -> (Vec<String>, Vec<(usize, usize)>) {
    let mut edges = Vec::new();
    // 4 노드 다이아몬드 5 벌을 사슬로 연결한다.
    for block in 0..5 {
        let base = block * 4;
        edges.extend_from_slice(&[
            (base, base + 1),
            (base, base + 2),
            (base + 1, base + 3),
            (base + 2, base + 3),
        ]);
        if block > 0 {
            edges.push((base - 1, base));
        }
    }
    (ids(20), edges)
}

/// 시안 mock 규모(55 노드)의 넓고 깊은 그래프.
fn graph_55() -> (Vec<String>, Vec<(usize, usize)>) {
    let mut edges = Vec::new();
    for i in 1..55 {
        edges.push((i / 3, i)); // 3-ary 트리
        if i >= 6 {
            edges.push((i - 6, i)); // 레이어를 건너뛰는 교차 엣지
        }
    }
    (ids(55), edges)
}

// ── 스펙 "확인 절차" 대응 ──

#[test]
fn linear_chain_assigns_increasing_layers() {
    let out = layout_dag(&ids(3), &[(0, 1), (1, 2)], &LayoutConfig::default());
    let layers: Vec<u32> = out.nodes.iter().map(|n| n.layer).collect();
    assert_eq!(layers, vec![0, 1, 2]);

    // 레이어가 커지면 레이어 축 좌표도 커진다.
    let along: Vec<f32> = out
        .nodes
        .iter()
        .map(|n| along_of(n, Orientation::LeftRight))
        .collect();
    assert!(along[0] < along[1] && along[1] < along[2], "{along:?}");
    assert!(!out.has_cycle);
}

#[test]
fn diamond_places_siblings_on_same_layer_at_different_positions() {
    let cfg = LayoutConfig::default();
    let out = layout_dag(&ids(4), &[(0, 1), (0, 2), (1, 3), (2, 3)], &cfg);
    let (b, c) = (node(&out, "t1"), node(&out, "t2"));
    assert_eq!(b.layer, c.layer);
    assert_ne!(b.y, c.y); // LeftRight 기본이므로 형제는 세로로 갈린다
    assert!(!overlaps(b, c, &cfg));
    assert_eq!(node(&out, "t0").layer, 0);
    assert_eq!(node(&out, "t3").layer, 2);
}

#[test]
fn single_node_without_edges_does_not_panic() {
    let out = layout_dag(&["solo".to_string()], &[], &LayoutConfig::default());
    assert_eq!(out.nodes.len(), 1);
    assert_eq!(out.nodes[0].id, "solo");
    assert_eq!(out.nodes[0].layer, 0);
    assert_eq!(out.width, LogicalPx(168.0));
    assert_eq!(out.height, LogicalPx(48.0));
}

#[test]
fn empty_graph_returns_empty_layout() {
    let out = layout_dag(&[], &[(0, 1)], &LayoutConfig::default());
    assert!(out.nodes.is_empty());
    assert!(out.edges.is_empty());
    assert!(!out.has_cycle);
}

#[test]
fn cyclic_graph_lays_out_without_panicking() {
    let out = layout_dag(&ids(2), &[(0, 1), (1, 0)], &LayoutConfig::default());
    assert_eq!(out.nodes.len(), 2);
    assert!(out.has_cycle);
    // 격자 폴백이 아니라 정상 레이어 배치가 나온다 — 두 노드가 다른 레이어에 선다.
    assert_ne!(out.nodes[0].layer, out.nodes[1].layer);
    // 되돌아가는 엣지가 표시된다.
    assert_eq!(out.edges.iter().filter(|e| e.back).count(), 1);
}

#[test]
fn longer_cycle_still_produces_layered_layout() {
    let cfg = LayoutConfig::default();
    let out = layout_dag(&ids(5), &[(0, 1), (1, 2), (2, 3), (3, 4), (4, 1)], &cfg);
    assert_eq!(out.nodes.len(), 5);
    assert!(out.has_cycle);
    assert!(out.nodes.iter().map(|n| n.layer).max().unwrap() >= 3);
    assert_no_overlap(&out, &cfg);
}

#[test]
fn self_loop_is_reported_as_cycle_and_dropped_from_edges() {
    let out = layout_dag(&ids(2), &[(0, 0), (0, 1)], &LayoutConfig::default());
    assert!(out.has_cycle);
    assert_eq!(out.edges.len(), 1);
    assert_eq!((out.edges[0].from, out.edges[0].to), (0, 1));
}

#[test]
fn no_two_nodes_share_identical_coordinates() {
    let (ids, edges) = graph_20();
    let out = layout_dag(&ids, &edges, &LayoutConfig::default());
    let mut seen = HashSet::new();
    for n in &out.nodes {
        assert!(
            seen.insert((n.x.value().to_bits(), n.y.value().to_bits())),
            "duplicate coordinate at {}",
            n.id
        );
    }
}

// ── 불변식 ──

fn assert_no_overlap(out: &GraphLayout, cfg: &LayoutConfig) {
    for (i, a) in out.nodes.iter().enumerate() {
        for b in &out.nodes[i + 1..] {
            assert!(
                !overlaps(a, b, cfg),
                "{} {:?} overlaps {} {:?}",
                a.id,
                (a.x, a.y),
                b.id,
                (b.x, b.y)
            );
        }
    }
}

#[test]
fn node_cards_never_overlap() {
    let cfg = LayoutConfig::default();
    for (ids, edges) in [graph_20(), graph_55()] {
        assert_no_overlap(&layout_dag(&ids, &edges, &cfg), &cfg);
    }
    let td = LayoutConfig {
        orientation: Orientation::TopDown,
        ..cfg
    };
    let (ids, edges) = graph_55();
    assert_no_overlap(&layout_dag(&ids, &edges, &td), &td);
}

#[test]
fn nodes_are_returned_in_input_order_without_dummy_leak() {
    let (ids, edges) = graph_55();
    let out = layout_dag(&ids, &edges, &LayoutConfig::default());
    assert_eq!(out.nodes.len(), ids.len());
    for (n, id) in out.nodes.iter().zip(ids.iter()) {
        assert_eq!(&n.id, id);
    }
}

#[test]
fn isolated_nodes_get_their_own_column_on_the_first_layer() {
    let cfg = LayoutConfig::default();
    // t0→t1 만 이어지고 t2, t3 은 고립.
    let out = layout_dag(&ids(4), &[(0, 1)], &cfg);
    assert_eq!(out.nodes.len(), 4);
    assert_eq!(node(&out, "t2").layer, 0);
    assert_eq!(node(&out, "t3").layer, 0);
    assert_no_overlap(&out, &cfg);
    // 조각은 형제 축 방향으로 나란히 붙는다.
    let mut crosses: Vec<f32> = ["t0", "t2", "t3"]
        .iter()
        .map(|id| cross_of(node(&out, id), cfg.orientation))
        .collect();
    crosses.sort_by(f32::total_cmp);
    for pair in crosses.windows(2) {
        assert!(pair[1] - pair[0] >= cfg.node_size.1.value(), "{crosses:?}");
    }
}

#[test]
fn out_of_range_and_duplicate_edges_are_dropped() {
    let out = layout_dag(
        &ids(2),
        &[(0, 1), (0, 1), (0, 9), (7, 0)],
        &LayoutConfig::default(),
    );
    assert_eq!(out.edges.len(), 1);
    assert!(!out.has_cycle);
}

#[test]
fn layer_skipping_edge_gets_bend_points_that_avoid_cards() {
    let cfg = LayoutConfig::default();
    // a→b→c 와 함께 a→c 가 있다. a→c 는 레이어 1 을 건너뛴다.
    let out = layout_dag(&ids(3), &[(0, 1), (1, 2), (0, 2)], &cfg);
    let skipping = out
        .edges
        .iter()
        .find(|e| (e.from, e.to) == (0, 2))
        .expect("skipping edge missing");
    assert!(
        skipping.points.len() >= 3,
        "expected a bend point, got {:?}",
        skipping.points
    );
    for edge in &out.edges {
        for &point in &edge.points {
            for n in &out.nodes {
                assert!(
                    !strictly_inside(point, n, &cfg),
                    "point {point:?} sits inside card {}",
                    n.id
                );
            }
        }
    }
}

#[test]
fn every_edge_polyline_avoids_every_card_in_a_large_graph() {
    let cfg = LayoutConfig::default();
    let (ids, edges) = graph_55();
    let out = layout_dag(&ids, &edges, &cfg);
    let mut with_bends = 0;
    for edge in &out.edges {
        assert!(edge.points.len() >= 2);
        if edge.points.len() > 2 {
            with_bends += 1;
        }
        for &point in &edge.points {
            for n in &out.nodes {
                assert!(
                    !strictly_inside(point, n, &cfg),
                    "point {point:?} sits inside card {}",
                    n.id
                );
            }
        }
    }
    assert!(with_bends > 0, "the 55-node mock should skip layers");
}

#[test]
fn bounding_box_contains_every_node_and_bend_point() {
    let cfg = LayoutConfig::default();
    let (ids, edges) = graph_55();
    let out = layout_dag(&ids, &edges, &cfg);
    let (w, h) = (cfg.node_size.0.value(), cfg.node_size.1.value());
    for n in &out.nodes {
        assert!(n.x.value() >= -f32::EPSILON && n.y.value() >= -f32::EPSILON);
        assert!(n.x.value() + w <= out.width.value() + 1e-3);
        assert!(n.y.value() + h <= out.height.value() + 1e-3);
    }
    for edge in &out.edges {
        for &(x, y) in &edge.points {
            assert!(x.value() >= -1e-3 && x.value() <= out.width.value() + 1e-3);
            assert!(y.value() >= -1e-3 && y.value() <= out.height.value() + 1e-3);
        }
    }
}

// ── 방향 토글 ──

#[test]
fn orientation_swaps_axes_and_preserves_layer_and_sibling_order() {
    let (ids, edges) = graph_20();
    let lr = LayoutConfig::default();
    let td = LayoutConfig {
        orientation: Orientation::TopDown,
        ..lr
    };
    let a = layout_dag(&ids, &edges, &lr);
    let b = layout_dag(&ids, &edges, &td);

    // 레이어 번호는 방향과 무관하다.
    let layers_a: Vec<u32> = a.nodes.iter().map(|n| n.layer).collect();
    let layers_b: Vec<u32> = b.nodes.iter().map(|n| n.layer).collect();
    assert_eq!(layers_a, layers_b);

    // 레이어 축이 x → y 로 교환된다.
    for n in &a.nodes {
        let other = node(&b, &n.id);
        assert_eq!(
            along_of(n, Orientation::LeftRight) > 0.0,
            along_of(other, Orientation::TopDown) > 0.0
        );
    }

    // 형제 순서가 보존된다.
    assert_eq!(
        sibling_order(&a, Orientation::LeftRight),
        sibling_order(&b, Orientation::TopDown)
    );
}

#[test]
fn layer_axis_pitch_follows_config_gaps() {
    let cfg = LayoutConfig::default();
    let out = layout_dag(&ids(3), &[(0, 1), (1, 2)], &cfg);
    let pitch = cfg.node_size.0.value() + cfg.layer_gap.value();
    let xs: Vec<f32> = out.nodes.iter().map(|n| n.x.value()).collect();
    assert!((xs[1] - xs[0] - pitch).abs() < 1e-3, "{xs:?}");
    assert!((xs[2] - xs[1] - pitch).abs() < 1e-3, "{xs:?}");
}

// ── 결정성 / 성능 ──

#[test]
fn same_input_yields_identical_layout() {
    let (ids, edges) = graph_55();
    let cfg = LayoutConfig::default();
    assert_eq!(
        layout_dag(&ids, &edges, &cfg),
        layout_dag(&ids, &edges, &cfg)
    );
}

#[test]
fn large_graph_layout_stays_within_a_frame_budget() {
    let n = 200;
    let ids = ids(n);
    let mut edges = Vec::new();
    for i in 1..n {
        edges.push((i / 2, i));
        if i >= 4 {
            edges.push((i - 4, i));
        }
        if i >= 8 && i % 4 == 0 {
            edges.push((i - 3, i));
        }
    }
    assert!(edges.len() >= 400, "{} edges", edges.len());

    let started = std::time::Instant::now();
    let out = layout_dag(&ids, &edges, &LayoutConfig::default());
    let elapsed = started.elapsed();
    assert_eq!(out.nodes.len(), n);

    // debug 빌드는 최적화가 없어 한 프레임 예산으로 재면 의미가 없다. release 에서만
    // 16ms 를 강제하고, debug 는 "폭주하지 않는다" 수준만 확인한다.
    let budget = if cfg!(debug_assertions) {
        std::time::Duration::from_secs(5)
    } else {
        std::time::Duration::from_millis(16)
    };
    assert!(
        elapsed < budget,
        "{n} nodes / {} edges took {elapsed:?}, budget {budget:?}",
        edges.len()
    );
}
