//! `rust-sugiyama` 어댑터. **외부 레이아웃 라이브러리 호출은 이 모듈 안에서만**
//! 일어난다.
//!
//! 라이브러리를 자체 구현으로 갈아끼워야 할 때 손대는 범위가 이 파일 하나로
//! 갇히도록, 바깥에는 `crate::layout_dag` 의 공개 타입만 노출한다.
//!
//! # 축 이름
//!
//! 내부 계산은 화면 축이 아니라 **레이어 축(`along`)** 과 **형제 축(`cross`)** 으로
//! 한다. `rust-sugiyama` 는 언제나 위→아래(레이어 = y, 형제 = x)로 계산하므로 그
//! 출력을 그대로 이 두 축에 대응시키고, 화면 좌표로 옮기는 마지막 단계에서만
//! [`Orientation`] 에 따라 두 축을 x/y 에 붙인다.
//!
//! # 라이브러리에 대해 실측으로 확인한 사실 (0.4.0)
//!
//! - **사이클을 스스로 없앤다** — phase 0 이 greedy feedback arc set 으로 되돌아가는
//!   엣지를 뒤집는다. 그래서 사이클 그래프도 정상 레이어 배치가 나온다. 단
//!   self-loop 는 뒤집어도 사라지지 않아 내부 `assert!` 가 깨지므로, 어댑터가
//!   **호출 전에** 걷어내야 한다.
//! - **dummy vertex 는 출력에서 빠진다** — phase 3 이 `is_dummy` 를 필터한다. 즉
//!   유령 노드가 샐 위험은 없는 대신, 꺾임점은 이쪽에서 직접 만들어야 한다
//!   ([`crate::routing`]).
//! - **반환 좌표의 id 는 vertex 를 넣은 순서(petgraph 인덱스)** 다. 우리가 준 id 가
//!   아니다 — 그래서 `0..n` 을 순서대로 넣어 둘이 일치하게 만든다.
//! - **반환값의 `width`/`height` 는 픽셀이 아니라 레이어 개수·최대 레이어 폭**이다.
//!   쓰지 않고 우리가 경계 상자를 다시 잰다.
//! - y 좌표는 rank 별 offset 이라 같은 레이어면 정확히 같은 값이다. 라이브러리가
//!   레이어 번호를 돌려주지 않으므로 서로 다른 y 를 정렬한 순서로 되살린다.

use std::collections::{BTreeMap, HashSet};
use std::panic::AssertUnwindSafe;

use rust_sugiyama::configure::{Config, CrossingMinimization, RankingType};
use tasty_type_geometry::length::LogicalPx;

use crate::routing::{Metrics, along_center, route};
use crate::{EdgeRoute, GraphLayout, LayoutConfig, NodePosition, Orientation};

/// 교차 감소 transpose 패스를 켜 두는 노드 수 상한. 근거는 [`call_library`] 주석.
const TRANSPOSE_NODE_LIMIT: usize = 128;

/// 노드 하나의 그래프 공간 배치.
#[derive(Debug, Clone, Copy)]
struct Placed {
    layer: u32,
    /// 형제 축 중심.
    cross: f32,
    /// 소속 조각(약연결 컴포넌트) 번호.
    component: usize,
}

/// 레이아웃 파이프라인 전체.
pub(crate) fn run(
    node_ids: &[String],
    edges: &[(usize, usize)],
    cfg: &LayoutConfig,
) -> GraphLayout {
    let n = node_ids.len();
    let clean = sanitize(edges, n);
    let has_cycle = has_self_loop(edges, n) || is_cyclic(n, &clean);

    let m = metrics(cfg);
    let placed =
        solve(n, &clean, &m, cfg.component_gap.value()).unwrap_or_else(|| grid_fallback(n, &m));

    let mut layout = finish(node_ids, &clean, &placed, &m, cfg);
    layout.has_cycle = has_cycle;
    layout
}

/// 화면 축과 무관한 "레이어 축 / 형제 축" 치수로 config 를 번역한다.
fn metrics(cfg: &LayoutConfig) -> Metrics {
    let (node_w, node_h) = (cfg.node_size.0.value(), cfg.node_size.1.value());
    // 카드는 회전하지 않는다. 방향이 바뀌면 어느 변이 형제 축을 향하는지만 바뀐다.
    let (cross_extent, along_extent) = match cfg.orientation {
        Orientation::TopDown => (node_w, node_h),
        Orientation::LeftRight => (node_h, node_w),
    };
    Metrics {
        cross_extent: cross_extent.max(0.0),
        along_extent: along_extent.max(0.0),
        layer_gap: cfg.layer_gap.value().max(0.0),
        sibling_gap: cfg.sibling_gap.value().max(0.0),
    }
}

/// self-loop · 범위 밖 인덱스 · 중복을 걷어내고 입력 순서를 유지한다.
///
/// self-loop 를 남기면 라이브러리의 greedy feedback arc set 이 그것을 제거하지
/// 못해 "사이클이 사라졌다" 내부 assert 가 깨진다.
fn sanitize(edges: &[(usize, usize)], n: usize) -> Vec<(usize, usize)> {
    let mut seen: HashSet<(usize, usize)> = HashSet::with_capacity(edges.len());
    let mut out = Vec::with_capacity(edges.len());
    for &(a, b) in edges {
        if a == b || a >= n || b >= n || !seen.insert((a, b)) {
            continue;
        }
        out.push((a, b));
    }
    out
}

/// 자기 자신을 가리키는 엣지가 하나라도 있는지. 사이클 판정에만 쓴다.
fn has_self_loop(edges: &[(usize, usize)], n: usize) -> bool {
    edges.iter().any(|&(a, b)| a == b && a < n)
}

/// Kahn 위상정렬로 사이클 유무만 본다(어느 엣지인지는 필요 없다).
fn is_cyclic(n: usize, edges: &[(usize, usize)]) -> bool {
    let mut indegree = vec![0_usize; n];
    let mut adjacency: Vec<Vec<usize>> = vec![Vec::new(); n];
    for &(a, b) in edges {
        adjacency[a].push(b);
        indegree[b] += 1;
    }
    let mut ready: Vec<usize> = (0..n).filter(|&i| indegree[i] == 0).collect();
    let mut settled = 0_usize;
    while let Some(v) = ready.pop() {
        settled += 1;
        for &w in &adjacency[v] {
            indegree[w] -= 1;
            if indegree[w] == 0 {
                ready.push(w);
            }
        }
    }
    settled != n
}

/// `rust-sugiyama` 로 그래프 공간 좌표를 뽑는다.
///
/// 라이브러리가 패닉하거나 모든 노드를 돌려주지 않으면 [`None`] — 호출자가 격자
/// 폴백으로 떨어진다.
fn solve(
    n: usize,
    edges: &[(usize, usize)],
    m: &Metrics,
    component_gap: f32,
) -> Option<Vec<Placed>> {
    let layouts = call_library(n, edges, m)?;

    let mut components: Vec<Vec<(usize, u32, f32)>> = Vec::with_capacity(layouts.len());
    for (component, _, _) in layouts {
        if component.is_empty() {
            continue;
        }
        let mut nodes = to_component(&component, m)?;
        enforce_separation(&mut nodes, m);
        components.push(nodes);
    }

    // 조각 순서를 입력 인덱스 기준으로 고정한다 — 같은 입력이 항상 같은 그림이 되도록.
    components.sort_by_key(|c| c.iter().map(|&(id, _, _)| id).min().unwrap_or(usize::MAX));

    place_components(n, &components, m, component_gap)
}

/// 라이브러리 호출 한 지점. 패닉이 호출자(= GUI 프레임)로 새지 않게 가둔다.
#[allow(clippy::type_complexity)] // 라이브러리가 정한 반환 형태 그대로다
fn call_library(
    n: usize,
    edges: &[(usize, usize)],
    m: &Metrics,
) -> Option<Vec<(Vec<(usize, (f64, f64))>, f64, f64)>> {
    let mut vertices: Vec<(u32, (f64, f64))> = Vec::with_capacity(n);
    for i in 0..n {
        // 레이어 축 크기는 1.0 고정 — 최종 레이어 축 좌표는 layer 번호로 다시
        // 계산하므로 값 자체는 쓰이지 않는다. 다만 0 을 주면 rank 별 offset 이
        // 겹쳐 레이어 구분이 사라지므로 양수여야 한다.
        vertices.push((u32::try_from(i).ok()?, (f64::from(m.cross_extent), 1.0)));
    }
    let mut lib_edges: Vec<(u32, u32)> = Vec::with_capacity(edges.len());
    for &(a, b) in edges {
        lib_edges.push((u32::try_from(a).ok()?, u32::try_from(b).ok()?));
    }

    let config = Config {
        minimum_length: 1,
        // 라이브러리는 이 값을 각 vertex 크기에 더해 "패딩" 으로 쓴다. 결과적으로
        // 같은 레이어 이웃 카드의 중심 간 거리가 cross_extent + sibling_gap 이 된다.
        vertex_spacing: f64::from(m.sibling_gap),
        dummy_vertices: true,
        // dummy 에는 vertex_spacing 이 더해지지 않는다(실측). 레이어를 건너뛰는
        // 엣지가 지나갈 통로를 확보하려면 여기에 형제 간격을 직접 준다.
        dummy_size: f64::from(m.sibling_gap.max(1.0)),
        // 시안 mock 이 longest-path layering 이다 — 소스를 전부 레이어 0 에 모으는
        // `Up` 이 그것에 해당한다. 라이브러리 기본값(`MinimizeEdgeLength`)은 소스를
        // 아래로 끌어내려 시안과 배치가 달라진다. `Up` 은 "레이어 사이에 실노드가
        // 하나도 없는 빈 레이어" 가 생기지 않는다는 부수 효과도 있어, 아래
        // 레이어 번호 복원이 엣지 span 과 어긋나지 않는다.
        ranking_type: RankingType::Up,
        c_minimization: CrossingMinimization::Barycenter,
        // 교차를 한 번 더 줄이는 선택적 패스. 품질은 확실히 좋아지지만 비용이
        // 초선형이다 — 실측(release, x86_64): 200 노드 7ms → 22ms, 500 노드
        // 7ms 대 → 455ms. task DAG 는 대부분 수십 노드라 그 구간에서는 켜 두는
        // 것이 이득이고, 큰 그래프에서만 끄면 최악 시간이 묶인다.
        transpose: n <= TRANSPOSE_NODE_LIMIT,
    };

    std::panic::catch_unwind(AssertUnwindSafe(|| {
        rust_sugiyama::from_vertices_and_edges(&vertices, &lib_edges, &config)
    }))
    .ok()
}

/// 라이브러리가 준 `(id, (x, y))` 목록을 `(id, layer, cross)` 로 바꾼다.
///
/// 라이브러리는 레이어 번호를 돌려주지 않고 y 좌표만 준다. 같은 레이어의 노드는
/// 정확히 같은 y 를 공유하므로, 서로 다른 y 를 오름차순 정렬한 순서가 곧 레이어
/// 번호다. 레이어 축 좌표 자체는 여기서 버리고 나중에 `layer_gap` 으로 다시
/// 계산한다 — 그래야 간격이 config 값과 정확히 일치한다.
///
/// 형제 축은 조각 안에서 가장 왼쪽 카드의 왼쪽 변이 0 이 되도록 옮긴다.
fn to_component(component: &[(usize, (f64, f64))], m: &Metrics) -> Option<Vec<(usize, u32, f32)>> {
    let mut ys: Vec<f64> = component.iter().map(|&(_, (_, y))| y).collect();
    ys.sort_by(f64::total_cmp);
    ys.dedup_by(|a, b| (*a - *b).abs() < 1e-6);

    let min_x = component
        .iter()
        .map(|&(_, (x, _))| x)
        .fold(f64::INFINITY, f64::min);
    if !min_x.is_finite() {
        return None;
    }

    let mut out = Vec::with_capacity(component.len());
    for &(id, (x, y)) in component {
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        let layer = ys.iter().position(|c| (c - y).abs() < 1e-6)?;
        out.push((
            id,
            u32::try_from(layer).ok()?,
            (x - min_x) as f32 + m.cross_extent * 0.5,
        ));
    }
    Some(out)
}

/// 같은 레이어 안에서 카드가 겹치지 않도록 최소 간격을 보장한다.
///
/// 정상 동작에서는 라이브러리가 이미 `cross_extent + sibling_gap` 을 지키므로 이
/// 패스는 아무것도 바꾸지 않는다. 임계값을 그보다 낮게 잡은 것도 그래서다 —
/// 부동소수 오차로 매번 미세하게 밀리는 일을 막는다. 라이브러리가 규칙을 어겼을
/// 때만 순서를 유지한 채 오른쪽으로 밀어 "두 노드가 같은 좌표" 를 원천 차단한다.
fn enforce_separation(nodes: &mut [(usize, u32, f32)], m: &Metrics) {
    let min_separation = m.cross_extent + m.sibling_gap * 0.5;
    let mut by_layer: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
    for (i, &(_, layer, _)) in nodes.iter().enumerate() {
        by_layer.entry(layer).or_default().push(i);
    }
    for slots in by_layer.values_mut() {
        slots.sort_by(|&a, &b| nodes[a].2.total_cmp(&nodes[b].2).then(a.cmp(&b)));
        for w in 1..slots.len() {
            let previous = nodes[slots[w - 1]].2;
            let current = &mut nodes[slots[w]].2;
            if *current < previous + min_separation {
                *current = previous + min_separation;
            }
        }
    }
}

/// 조각들을 형제 축 방향으로 나란히 붙여 최종 배치를 만든다.
///
/// 모든 조각의 레이어 0 은 같은 줄에 정렬된다 — 고립 노드도 첫 줄에 선다.
fn place_components(
    n: usize,
    components: &[Vec<(usize, u32, f32)>],
    m: &Metrics,
    component_gap: f32,
) -> Option<Vec<Placed>> {
    let mut placed: Vec<Option<Placed>> = vec![None; n];
    let mut offset = 0.0_f32;
    for (component, nodes) in components.iter().enumerate() {
        let mut span = 0.0_f32;
        for &(id, layer, cross) in nodes {
            let entry = placed.get_mut(id)?;
            if entry.is_some() {
                return None; // 같은 노드가 두 조각에 나타나면 결과를 신뢰할 수 없다
            }
            *entry = Some(Placed {
                layer,
                cross: cross + offset,
                component,
            });
            span = span.max(cross + m.cross_extent * 0.5);
        }
        offset += span + component_gap;
    }
    placed.into_iter().collect()
}

/// 레이아웃 엔진을 신뢰할 수 없을 때의 최후 배치. 정사각형에 가까운 격자.
fn grid_fallback(n: usize, m: &Metrics) -> Vec<Placed> {
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)] // n > 0 → sqrt >= 1
    let columns = ((n as f32).sqrt().ceil() as usize).max(1);
    (0..n)
        .map(|i| Placed {
            layer: u32::try_from(i / columns).unwrap_or(u32::MAX),
            cross: (i % columns) as f32 * (m.cross_extent + m.sibling_gap) + m.cross_extent * 0.5,
            component: 0,
        })
        .collect()
}

/// 그래프 공간 배치를 화면 좌표 [`GraphLayout`] 으로 옮긴다.
fn finish(
    node_ids: &[String],
    edges: &[(usize, usize)],
    placed: &[Placed],
    m: &Metrics,
    cfg: &LayoutConfig,
) -> GraphLayout {
    let channels = build_channels(placed);
    let routes = build_routes(edges, placed, &channels, m);

    let (node_w, node_h) = (cfg.node_size.0.value(), cfg.node_size.1.value());
    let to_screen = |cross: f32, along: f32| match cfg.orientation {
        Orientation::TopDown => (cross, along),
        Orientation::LeftRight => (along, cross),
    };
    let centers: Vec<(f32, f32)> = placed
        .iter()
        .map(|p| to_screen(p.cross, along_center(p.layer, m)))
        .collect();

    let (min, max) = bounding_box(&centers, &routes, (node_w, node_h), &to_screen);

    let nodes = node_ids
        .iter()
        .zip(placed.iter())
        .zip(centers.iter())
        .map(|((id, p), &(cx, cy))| NodePosition {
            id: id.clone(),
            x: LogicalPx(cx - node_w * 0.5 - min.0),
            y: LogicalPx(cy - node_h * 0.5 - min.1),
            layer: p.layer,
        })
        .collect();

    let edges = routes
        .into_iter()
        .map(|route| EdgeRoute {
            from: route.from,
            to: route.to,
            back: route.back,
            points: route
                .points
                .into_iter()
                .map(|(cross, along)| {
                    let (x, y) = to_screen(cross, along);
                    (LogicalPx(x - min.0), LogicalPx(y - min.1))
                })
                .collect(),
        })
        .collect();

    GraphLayout {
        nodes,
        edges,
        width: LogicalPx(max.0 - min.0),
        height: LogicalPx(max.1 - min.1),
        has_cycle: false, // 호출자(`run`)가 채운다
    }
}

/// 그래프 공간에서 라우팅한 엣지 하나.
struct RawRoute {
    from: usize,
    to: usize,
    back: bool,
    points: Vec<(f32, f32)>,
}

/// 조각·레이어별 실노드 형제 축 좌표 — 엣지 꺾임점이 피해가야 할 자리다.
fn build_channels(placed: &[Placed]) -> Vec<BTreeMap<u32, Vec<f32>>> {
    let component_count = placed.iter().map(|p| p.component + 1).max().unwrap_or(0);
    let mut channels: Vec<BTreeMap<u32, Vec<f32>>> = vec![BTreeMap::new(); component_count];
    for p in placed {
        if let Some(component) = channels.get_mut(p.component) {
            component.entry(p.layer).or_default().push(p.cross);
        }
    }
    for component in &mut channels {
        for lane in component.values_mut() {
            lane.sort_by(f32::total_cmp);
        }
    }
    channels
}

fn build_routes(
    edges: &[(usize, usize)],
    placed: &[Placed],
    channels: &[BTreeMap<u32, Vec<f32>>],
    m: &Metrics,
) -> Vec<RawRoute> {
    let empty = BTreeMap::new();
    let mut routes = Vec::with_capacity(edges.len());
    for &(a, b) in edges {
        let (Some(pa), Some(pb)) = (placed.get(a), placed.get(b)) else {
            continue;
        };
        let lanes = channels.get(pa.component).unwrap_or(&empty);
        routes.push(RawRoute {
            from: a,
            to: b,
            back: pb.layer <= pa.layer,
            points: route((pa.layer, pa.cross), (pb.layer, pb.cross), lanes, m),
        });
    }
    routes
}

/// 노드 사각형과 폴리라인 전체를 감싸는 경계 상자.
fn bounding_box(
    centers: &[(f32, f32)],
    routes: &[RawRoute],
    node_size: (f32, f32),
    to_screen: &impl Fn(f32, f32) -> (f32, f32),
) -> ((f32, f32), (f32, f32)) {
    let (node_w, node_h) = node_size;
    let mut min = (f32::INFINITY, f32::INFINITY);
    let mut max = (f32::NEG_INFINITY, f32::NEG_INFINITY);
    let mut bump = |x: f32, y: f32| {
        min = (min.0.min(x), min.1.min(y));
        max = (max.0.max(x), max.1.max(y));
    };
    for &(cx, cy) in centers {
        bump(cx - node_w * 0.5, cy - node_h * 0.5);
        bump(cx + node_w * 0.5, cy + node_h * 0.5);
    }
    for route in routes {
        for &(cross, along) in &route.points {
            let (x, y) = to_screen(cross, along);
            bump(x, y);
        }
    }
    if min.0.is_finite() && min.1.is_finite() {
        (min, max)
    } else {
        ((0.0, 0.0), (0.0, 0.0))
    }
}
