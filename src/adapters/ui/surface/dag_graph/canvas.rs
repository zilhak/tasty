//! 캔버스 페인팅과 인터랙션.
//!
//! 노드/엣지는 위젯이 아니라 [`egui::Painter`] 직접 페인팅이다. 노드 수백 개마다
//! 위젯을 할당하면 매 프레임 id/레이아웃 비용이 노드 수에 비례해 붙는데, 이 화면에
//! 필요한 상호작용은 **hover 와 클릭 하나씩**뿐이라 직접 히트테스트가 훨씬 싸다.
//!
//! 좌표계는 두 개다. **그래프 좌표**(레이아웃 엔진이 준 logical px)와 **화면 좌표**.
//! 변환은 `canvas.min + offset + graph * zoom` 하나뿐이며 [`Transform`] 이 소유한다.

use tasty_dag_layout::GraphLayout;
use tasty_design_tokens::generated::component::dag::EDGE_DIM_OPACITY;
use tasty_model::DagDirection;
use tasty_type_appearance::theme::Theme;

use super::chrome::ChromeAction;
use super::model::{DagGraphData, DagRelation};
use super::node::{NodeVisual, paint_node, paint_selection_ring};
use super::view::{DagGraphView, Lod};

/// 그래프 좌표 ↔ 화면 좌표.
#[derive(Clone, Copy)]
pub struct Transform {
    pub origin: egui::Pos2,
    pub zoom: f32,
}

impl Transform {
    pub fn to_screen(self, x: f32, y: f32) -> egui::Pos2 {
        egui::pos2(self.origin.x + x * self.zoom, self.origin.y + y * self.zoom)
    }

    pub fn rect(self, x: f32, y: f32, w: f32, h: f32) -> egui::Rect {
        egui::Rect::from_min_size(
            self.to_screen(x, y),
            egui::vec2(w * self.zoom, h * self.zoom),
        )
    }
}

/// 캔버스를 그리고 상호작용을 처리한다. 반환값은 캔버스 위 크롬(줌 클러스터)에서
/// 나온 조작이다.
///
/// 키 단축키는 **여기서 만들지 않는다**. tasty 의 모든 단축키는 `KeybindingSettings`
/// 를 거쳐야 하고(`docs/design/policies/key-mapping.md`), 이 캔버스가 자체 조합을
/// 박으면 그 조합이 이미 배정된 전역 액션과 조용히 겹친다. 방향 전환·fit·줌은
/// 우하단 줌 클러스터의 버튼이 담당한다.
pub fn draw_canvas(
    ui: &mut egui::Ui,
    theme: &Theme,
    view: &mut DagGraphView,
    graph: &DagGraphData,
    layout: &GraphLayout,
    direction: DagDirection,
    now_ms: u64,
) -> Option<ChromeAction> {
    let (rect, response) =
        ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, theme.dag_canvas_bg().to_egui());

    let graph_size = egui::vec2(layout.width.value(), layout.height.value());
    // auto-fit 은 (DAG, 방향, 뷰포트 버킷) 조합마다 딱 한 번이다. 폴링으로 상태가
    // 바뀌었다고 다시 맞추면 사용자가 잡아 둔 시야가 0.5 초마다 리셋된다.
    if !graph.nodes.is_empty() && view.take_fit(&graph.id, direction, rect.size()) {
        view.fit(graph_size, rect.size(), theme.dag_canvas_padding().value());
    }

    // 줌 클러스터는 캔버스 위에 떠 있다 — 그 자리에서 시작한 클릭·드래그는 pan 도
    // 선택도 아니다. 히트테스트보다 먼저 자리를 알아야 하므로 rect 를 미리 잡는다.
    let cluster = super::chrome::zoom_cluster_rect(theme, rect);
    interact(ui, theme, &response, rect, cluster, view, graph, layout);

    let tr = Transform {
        origin: rect.min + view.offset,
        zoom: view.zoom,
    };
    let lod = Lod::of(view.zoom);

    paint_dot_grid(&painter, theme, rect, &tr);

    let dead = dead_path(graph);
    let cycle_nodes = cycle_set(graph);
    let selected_idx = view.selected.as_deref().and_then(|id| graph.index_of(id));

    for (edge, route) in graph.edges.iter().zip(layout.edges.iter()) {
        let dim = dead[route.from] || dead[route.to];
        let highlight = selected_idx == Some(route.from) || selected_idx == Some(route.to);
        paint_edge(
            &painter,
            theme,
            &tr,
            route,
            edge.relation,
            direction,
            dim,
            highlight,
        );
    }

    let hovered = response
        .hover_pos()
        .filter(|p| !cluster.contains(*p))
        .and_then(|p| node_at(layout, &tr, theme, p));

    for (i, pos) in layout.nodes.iter().enumerate() {
        let Some(node) = graph.nodes.get(i) else {
            continue;
        };
        let r = tr.rect(
            pos.x.value(),
            pos.y.value(),
            theme.dag_node_width().value(),
            theme.dag_node_height().value(),
        );
        // 화면 밖 카드는 그리지 않는다 — 500 노드에서 대부분이 여기서 걸러진다.
        if !rect.intersects(r.expand(theme.dag_node_selected_ring_width().value())) {
            continue;
        }
        let vis = NodeVisual {
            lod,
            zoom: view.zoom,
            now_ms,
            selected: selected_idx == Some(i),
            hovered: hovered == Some(i),
            dimmed: dead[i],
            in_cycle: cycle_nodes.contains(&node.id),
        };
        paint_node(ui, &painter, theme, r, node, &vis);
        if vis.selected {
            paint_selection_ring(&painter, theme, r, view.zoom);
        }
    }

    // 다음 폴링 시점에 스스로 깨어난다. **보이는 동안만** 예약하므로 배경 탭은
    // 프레임을 소모하지 않는다.
    ui.ctx().request_repaint_after(view.until_next_poll());

    super::chrome::draw_canvas_chrome(ui, theme, rect, view, layout, direction, lod)
}

/// pan / zoom / 선택.
#[allow(clippy::too_many_arguments)]
fn interact(
    ui: &egui::Ui,
    theme: &Theme,
    response: &egui::Response,
    rect: egui::Rect,
    cluster: egui::Rect,
    view: &mut DagGraphView,
    graph: &DagGraphData,
    layout: &GraphLayout,
) {
    // 중클릭 드래그는 노드 위에서도 pan 이다. 좌클릭 드래그는 빈 곳에서만 pan —
    // 노드 위 좌클릭 드래그를 pan 으로 삼으면 "노드를 옮기는 화면" 으로 오독된다.
    let tr = Transform {
        origin: rect.min + view.offset,
        zoom: view.zoom,
    };
    let on_chrome = response.hover_pos().is_some_and(|p| cluster.contains(p));
    let on_node = response
        .hover_pos()
        .filter(|_| !on_chrome)
        .and_then(|p| node_at(layout, &tr, theme, p));
    let panning = !on_chrome
        && (response.dragged_by(egui::PointerButton::Middle)
            || (response.dragged_by(egui::PointerButton::Primary) && on_node.is_none()));
    if panning {
        view.offset += response.drag_delta();
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }

    if response.clicked() && !on_chrome {
        view.selected = on_node
            .and_then(|i| graph.nodes.get(i))
            .map(|n| n.id.clone());
    }

    if !response.hovered() || on_chrome {
        return;
    }
    let (scroll, modifiers) = ui.input(|i| (i.smooth_scroll_delta, i.modifiers));
    if scroll != egui::Vec2::ZERO {
        if modifiers.command {
            let anchor = response
                .hover_pos()
                .map(|p| p - rect.min)
                .unwrap_or(rect.size() / 2.0);
            view.zoom_by(scroll.y.signum(), anchor);
        } else if modifiers.shift {
            view.offset.x += scroll.y + scroll.x;
        } else {
            view.offset += scroll;
        }
    }
    // `Esc` 는 단축키 배정이 아니라 "열린 것을 닫는다" 는 OS 공통 관례라 예외다 —
    // `KeybindingSettings` 에 노출되는 조합이 아니고 재배정 대상도 아니다.
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        view.selected = None;
    }
}

/// 점 아래의 노드 인덱스. 뒤에서부터 찾아 위에 그려진 카드가 이긴다.
fn node_at(layout: &GraphLayout, tr: &Transform, theme: &Theme, p: egui::Pos2) -> Option<usize> {
    let (w, h) = (
        theme.dag_node_width().value(),
        theme.dag_node_height().value(),
    );
    layout
        .nodes
        .iter()
        .enumerate()
        .rev()
        .find(|(_, n)| tr.rect(n.x.value(), n.y.value(), w, h).contains(p))
        .map(|(i, _)| i)
}

/// 배경 점 격자. 그래프 좌표에 고정돼 pan/zoom 을 따라 움직인다 — 화면에 고정하면
/// 팬 중에 배경만 정지해 보여 오히려 이동이 안 읽힌다.
fn paint_dot_grid(painter: &egui::Painter, theme: &Theme, rect: egui::Rect, tr: &Transform) {
    let gap = theme.dag_canvas_dot_gap().value() * tr.zoom;
    // 너무 촘촘해지면 점이 뭉쳐 회색 판이 된다 — 그 배율에서는 격자를 접는다.
    if gap < 6.0 {
        return;
    }
    let size = (theme.dag_canvas_dot_size().value() * tr.zoom).max(1.0) / 2.0;
    let color = theme.dag_canvas_dot().to_egui();
    let start_x = rect.min.x + (tr.origin.x - rect.min.x).rem_euclid(gap);
    let start_y = rect.min.y + (tr.origin.y - rect.min.y).rem_euclid(gap);
    let mut y = start_y - gap;
    while y < rect.max.y {
        let mut x = start_x - gap;
        while x < rect.max.x {
            painter.circle_filled(egui::pos2(x, y), size, color);
            x += gap;
        }
        y += gap;
    }
}

/// 엣지 하나 — 직교 세그먼트 + 라운드 코너 + 화살촉.
#[allow(clippy::too_many_arguments)]
fn paint_edge(
    painter: &egui::Painter,
    theme: &Theme,
    tr: &Transform,
    route: &tasty_dag_layout::EdgeRoute,
    relation: DagRelation,
    direction: DagDirection,
    dim: bool,
    highlight: bool,
) {
    let base = if highlight {
        theme.dag_edge_highlight()
    } else {
        match relation {
            DagRelation::DependsOn => theme.dag_edge_depends(),
            DagRelation::Fallback => theme.dag_edge_fallback(),
            DagRelation::Reduce => theme.dag_edge_reduce(),
        }
    };
    let mut color = base.to_egui();
    if dim {
        color = color.gamma_multiply(EDGE_DIM_OPACITY);
    }
    let stroke = egui::Stroke::new(theme.dag_edge_width().value().max(1.0), color);

    let raw: Vec<egui::Pos2> = route
        .points
        .iter()
        .map(|(x, y)| tr.to_screen(x.value(), y.value()))
        .collect();
    if raw.len() < 2 {
        return;
    }
    let ortho = orthogonalize(&raw, direction);
    let rounded = round_corners(&ortho, theme.dag_edge_corner_radius().value() * tr.zoom);

    match relation.dash() {
        Some((on, off)) => painter.add(egui::Shape::dashed_line(
            &rounded,
            stroke,
            on * tr.zoom,
            off * tr.zoom,
        )),
        None => painter.add(egui::Shape::line(rounded.clone(), stroke)),
    };

    paint_arrow(
        painter,
        theme.dag_edge_arrow_size().value() * tr.zoom,
        color,
        &ortho,
    );
}

/// 대각 세그먼트를 레이어 축 중간에서 꺾어 직교로 편다.
fn orthogonalize(points: &[egui::Pos2], direction: DagDirection) -> Vec<egui::Pos2> {
    let mut out = vec![points[0]];
    for w in points.windows(2) {
        let (a, b) = (w[0], w[1]);
        let diag = (a.x - b.x).abs() > 0.5 && (a.y - b.y).abs() > 0.5;
        if diag {
            match direction {
                DagDirection::LeftRight => {
                    let mid = (a.x + b.x) / 2.0;
                    out.push(egui::pos2(mid, a.y));
                    out.push(egui::pos2(mid, b.y));
                }
                DagDirection::TopDown => {
                    let mid = (a.y + b.y) / 2.0;
                    out.push(egui::pos2(a.x, mid));
                    out.push(egui::pos2(b.x, mid));
                }
            }
        }
        out.push(b);
    }
    out
}

/// 각 꺾임점을 반경 `r` 짜리 사분원으로 깎는다.
fn round_corners(points: &[egui::Pos2], r: f32) -> Vec<egui::Pos2> {
    if points.len() < 3 || r <= 0.5 {
        return points.to_vec();
    }
    const ARC_STEPS: usize = 4;
    let mut out = vec![points[0]];
    for i in 1..points.len() - 1 {
        let (prev, cur, next) = (points[i - 1], points[i], points[i + 1]);
        let din = (cur - prev).normalized();
        let dout = (next - cur).normalized();
        let rin = r.min((cur - prev).length() / 2.0);
        let rout = r.min((next - cur).length() / 2.0);
        let start = cur - din * rin;
        let end = cur + dout * rout;
        out.push(start);
        // 제어점을 모서리로 두는 2 차 베지어 — 직각에서 사분원과 시각적으로 같다.
        for s in 1..ARC_STEPS {
            let t = s as f32 / ARC_STEPS as f32;
            let u = 1.0 - t;
            out.push(egui::pos2(
                u * u * start.x + 2.0 * u * t * cur.x + t * t * end.x,
                u * u * start.y + 2.0 * u * t * cur.y + t * t * end.y,
            ));
        }
        out.push(end);
    }
    out.push(points[points.len() - 1]);
    out
}

/// 끝점 화살촉. 마지막 세그먼트 방향을 따른다.
fn paint_arrow(painter: &egui::Painter, size: f32, color: egui::Color32, points: &[egui::Pos2]) {
    if size < 2.0 || points.len() < 2 {
        return;
    }
    let tip = points[points.len() - 1];
    let dir = (tip - points[points.len() - 2]).normalized();
    if !dir.x.is_finite() || !dir.y.is_finite() {
        return;
    }
    let normal = egui::vec2(-dir.y, dir.x);
    let back = tip - dir * size;
    painter.add(egui::Shape::convex_polygon(
        vec![tip, back + normal * size * 0.4, back - normal * size * 0.4],
        color,
        egui::Stroke::NONE,
    ));
}

/// 절대 실행되지 않을 경로 — 실패/취소/스킵 노드의 하류 전부.
///
/// 상류가 죽었는데도 대기 상태로 남아 있는 노드는 "곧 돌 것" 처럼 보이는데 실제로는
/// 영원히 돌지 않는다. 그 사실이 카드에 드러나야 사용자가 재시도 지점을 찾는다.
fn dead_path(graph: &DagGraphData) -> Vec<bool> {
    let mut dead = vec![false; graph.nodes.len()];
    for (i, n) in graph.nodes.iter().enumerate() {
        if n.status.kills_outgoing() {
            dead[i] = true;
        }
    }
    // 노드 수만큼 반복하면 어떤 위상이든 수렴한다(사이클이 있어도 종료한다).
    for _ in 0..graph.nodes.len() {
        let mut changed = false;
        for e in &graph.edges {
            // fallback 엣지는 상류 실패가 **발동 조건**이라 죽은 경로가 아니다.
            if e.relation == DagRelation::Fallback {
                continue;
            }
            if dead[e.from] && !dead[e.to] && !graph.nodes[e.to].status.is_terminal() {
                dead[e.to] = true;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    // 이미 끝난 노드는 흐리게 하지 않는다 — 결과 자체는 읽어야 하는 정보다.
    for (i, n) in graph.nodes.iter().enumerate() {
        if n.status.is_terminal() {
            dead[i] = false;
        }
    }
    dead
}

fn cycle_set(graph: &DagGraphData) -> std::collections::HashSet<String> {
    graph
        .cycle
        .as_ref()
        .map(|ids| ids.iter().cloned().collect())
        .unwrap_or_default()
}
