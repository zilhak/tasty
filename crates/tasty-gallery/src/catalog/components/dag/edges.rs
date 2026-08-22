//! 의존 엣지 — 디자인 `elbow()` + 캔버스 엣지 렌더의 구조 전사.
//!
//! 관계는 **색과 파선 패턴을 함께** 써서 구분한다(색만으로는 색각 이상에서
//! 사라진다). 화살촉은 언제나 **의존하는 쪽**(레이어가 큰 쪽)에 붙어 "…를
//! 기다린다" 로 읽힌다.

use tasty_design_tokens::generated::component::dag::EDGE_DIM_OPACITY;
use tasty_type_appearance::theme::Theme;

use super::Rel;
use crate::catalog::spec::{self, StageVariant, TokenChip};

/// 코너 하나를 이차 베지어로 몇 조각 내어 근사할지. egui 에는 SVG `Q` 가 없다.
const CORNER_STEPS: usize = 6;

/// 직교가 아닌 세그먼트를 "흐름축 → 가로축 → 흐름축" 으로 편다.
///
/// 레이아웃 엔진은 꺾임점 좌표까지만 책임지므로 대각선 구간이 남을 수 있다.
/// 디자인 `elbow()` 가 하는 일을 임의 길이 폴리라인으로 일반화한 것이다.
pub fn orthogonalize(points: &[egui::Pos2], top_down: bool) -> Vec<egui::Pos2> {
    let mut out: Vec<egui::Pos2> = Vec::with_capacity(points.len() * 2);
    for w in points.windows(2) {
        let (a, b) = (w[0], w[1]);
        if out.last() != Some(&a) {
            out.push(a);
        }
        // 이미 축에 정렬된 구간은 그대로 두고, 대각선만 흐름축 기준으로 편다.
        let aligned = (a.x - b.x).abs() < 1.0 || (a.y - b.y).abs() < 1.0;
        if !aligned {
            if top_down {
                let m = (a.y + b.y) / 2.0;
                out.push(egui::pos2(a.x, m));
                out.push(egui::pos2(b.x, m));
            } else {
                let m = (a.x + b.x) / 2.0;
                out.push(egui::pos2(m, a.y));
                out.push(egui::pos2(m, b.y));
            }
        }
        out.push(b);
    }
    out
}

/// 직교 폴리라인의 각 꺾임을 반경 `r` 로 둥글린다 (디자인 `Q` 코너).
pub fn round_corners(points: &[egui::Pos2], r: f32) -> Vec<egui::Pos2> {
    if points.len() < 3 || r <= 0.0 {
        return points.to_vec();
    }
    let mut out = vec![points[0]];
    for i in 1..points.len() - 1 {
        let (prev, cur, next) = (points[i - 1], points[i], points[i + 1]);
        let in_len = (cur - prev).length();
        let out_len = (next - cur).length();
        let rr = r.min(in_len / 2.0).min(out_len / 2.0);
        if rr <= 0.5 {
            out.push(cur);
            continue;
        }
        let a = cur - (cur - prev).normalized() * rr;
        let b = cur + (next - cur).normalized() * rr;
        out.push(a);
        for s in 1..CORNER_STEPS {
            let t = s as f32 / CORNER_STEPS as f32;
            let u = 1.0 - t;
            out.push(egui::pos2(
                u * u * a.x + 2.0 * u * t * cur.x + t * t * b.x,
                u * u * a.y + 2.0 * u * t * cur.y + t * t * b.y,
            ));
        }
        out.push(b);
    }
    out.push(points[points.len() - 1]);
    out
}

/// 디자인 `elbow(s, t, td, r)` — 두 점을 잇는 둥근 직교 경로.
pub fn elbow(s: egui::Pos2, t: egui::Pos2, top_down: bool, r: f32) -> Vec<egui::Pos2> {
    round_corners(&orthogonalize(&[s, t], top_down), r)
}

/// 폴리라인 한 줄. `dash` 가 있으면 파선으로 끊어 그린다.
pub fn paint_path(
    painter: &egui::Painter,
    points: &[egui::Pos2],
    color: egui::Color32,
    width: f32,
    dash: Option<(f32, f32)>,
) {
    if points.len() < 2 {
        return;
    }
    let stroke = egui::Stroke::new(width, color);
    match dash {
        None => painter.add(egui::Shape::line(points.to_vec(), stroke)),
        Some((on, off)) => painter.add(egui::Shape::Vec(egui::Shape::dashed_line(
            points, stroke, on, off,
        ))),
    };
}

/// 도착점의 삼각 화살촉. `size` = `--tasty-dag-edge-arrow-size`(8).
///
/// 디자인 폴리곤은 밑변 8 · 높이 7 이다 — 높이는 밑변에서 1px(보더 폭) 뺀 값으로
/// 잡아 카드 변에 정확히 닿게 한다.
pub fn paint_arrow(
    painter: &egui::Painter,
    tip: egui::Pos2,
    top_down: bool,
    size: f32,
    border_width: f32,
    color: egui::Color32,
) {
    let half = size / 2.0;
    let len = size - border_width;
    let pts = if top_down {
        vec![
            egui::pos2(tip.x - half, tip.y - len),
            egui::pos2(tip.x + half, tip.y - len),
            tip,
        ]
    } else {
        vec![
            egui::pos2(tip.x - len, tip.y - half),
            egui::pos2(tip.x - len, tip.y + half),
            tip,
        ]
    };
    painter.add(egui::Shape::convex_polygon(pts, color, egui::Stroke::NONE));
}

/// 죽은 경로 감쇠 계수 (`--tasty-dag-edge-dim-opacity`).
pub fn dim_factor() -> f32 {
    EDGE_DIM_OPACITY
}

/// 디자인 `EdgeSpecimen` — 시안 뷰박스 120×56 안의 elbow 한 줄 + 관계 이름 + 라벨.
///
/// `node_box` 와 같은 이유로 한 번의 `allocate_exact_size` 로 자리를 잡는다.
fn edge_specimen(ui: &mut egui::Ui, theme: &Theme, rel: Rel) {
    let gap = theme.spacing_sm.value();
    let caption = theme.font_size_caption.value();
    let font = egui::FontId::proportional(caption);
    let line_h = super::node::row_height(ui, &font);
    // 시안 120×56: 카드 폭에서 레이어 간격과 여백을 뺀 값 / 카드 높이 + 8.
    let w =
        theme.dag_node_width().value() - theme.dag_layer_gap().value() - theme.spacing_lg.value();
    let h = theme.dag_node_height().value() + gap;
    let (outer, _) = ui.allocate_exact_size(
        egui::vec2(w, h + gap + line_h * 2.0 + gap),
        egui::Sense::hover(),
    );
    let rect = egui::Rect::from_min_size(outer.min, egui::vec2(w, h));
    let inset = theme.spacing_md.value();
    let s = egui::pos2(rect.min.x + inset, rect.min.y + gap);
    let t = egui::pos2(rect.max.x - inset, rect.max.y - gap);
    let color = rel.color(theme).to_egui();
    let path = elbow(s, t, true, theme.dag_edge_corner_radius().value());
    paint_path(
        ui.painter(),
        &path,
        color,
        theme.dag_edge_width().value(),
        rel.dash(),
    );
    paint_arrow(
        ui.painter(),
        t,
        true,
        theme.dag_edge_arrow_size().value(),
        theme.border_width.value(),
        color,
    );
    let cx = outer.center().x;
    ui.painter().text(
        egui::pos2(cx, rect.max.y + gap),
        egui::Align2::CENTER_TOP,
        rel.key(),
        egui::FontId::monospace(caption),
        color,
    );
    ui.painter().text(
        egui::pos2(cx, rect.max.y + gap + line_h),
        egui::Align2::CENTER_TOP,
        rel.label(),
        font,
        theme.text_muted().to_egui(),
    );
}

/// `edges` 섹션 Spec — 관계 3 종.
pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        for rel in [Rel::DependsOn, Rel::Fallback, Rel::Reduce] {
            edge_specimen(ui, theme, rel);
        }
    });
    spec::meta(
        ui,
        theme,
        &[
            ("routing", "orthogonal, 4px elbow"),
            ("width", "1px"),
            ("fallback dash", "6 3"),
            ("reduce dash", "2 3"),
            ("arrow", "8px triangle at target"),
            ("selected node", "its edges take the accent"),
        ],
        &[
            TokenChip::new(
                "--tasty-dag-edge-depends",
                "depends_on",
                theme.dag_edge_depends().to_egui(),
            ),
            TokenChip::new(
                "--tasty-dag-edge-fallback",
                "fallback",
                theme.dag_edge_fallback().to_egui(),
            ),
            TokenChip::new(
                "--tasty-dag-edge-reduce",
                "reduce",
                theme.dag_edge_reduce().to_egui(),
            ),
            TokenChip::new(
                "--tasty-dag-edge-highlight",
                "selected",
                theme.dag_edge_highlight().to_egui(),
            ),
        ],
    );
    spec::do_(
        ui,
        theme,
        "Dim the dead path: an edge leaving a failed/cancelled task, or entering a skipped one, \
         drops to 0.4 along with the node. The failure stays loud; everything it stranded recedes.",
    );
}
