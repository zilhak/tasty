//! 캔버스 크롬 — 디자인 `ZoomCluster` · `Minimap` · `CycleBanner` · `DagEmpty` +
//! LOD 힌트 칩의 구조 전사.
//!
//! 크롬은 캔버스 **위에** 뜨는 오버레이지 헤더가 아니다: 우하단 8px 안쪽에
//! 미니맵(위) + 줌 클러스터(아래)가 세로로 쌓이고, 좌하단에 LOD 칩이,
//! 상단 전폭에 사이클 배너가 붙는다.

use tasty_dag_layout::{GraphLayout, Orientation};
use tasty_icons as icons;
use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;

use super::Graph;
use super::canvas::Transform;
use super::node::Lod;
use crate::catalog::spec::{self, StageVariant, TokenChip};

/// 줌 퍼센트 판독창 최소 폭 — 디자인 `--tasty-size-46`.
const ZOOM_READOUT_WIDTH: LogicalPx = LogicalPx(46.0);
/// 이 폭 아래에서 판독창을 접고 28px 타깃 다섯 개만 남긴다 — 디자인 "compact cutoff".
const COMPACT_CUTOFF: LogicalPx = LogicalPx(400.0);
/// 빈 상태 글리프 크기 — 디자인 `--tasty-size-24`.
const EMPTY_ICON_SIZE: LogicalPx = LogicalPx(24.0);

/// 줌 클러스터 전체 크기. 버튼 4 개 + (판독창) + 1px 구분선.
pub fn zoom_cluster_size(theme: &Theme, compact: bool) -> egui::Vec2 {
    let h = theme.dag_chrome_height().value();
    let mut w = h * 4.0 + theme.border_width.value();
    if !compact {
        w += ZOOM_READOUT_WIDTH.value();
    }
    egui::vec2(w, h)
}

/// `− · % · + | fit · dir` — 28px 한 줄, 1px 보더로 둘러싼 한 덩어리.
///
/// 방향 토글 글리프는 현재 방향에 따라 바뀌지 않는다(시안과 동일) — 지금 어느
/// 방향인지는 그래프 자체가 보여주고, 버튼은 "뒤집는다" 는 동작만 뜻한다.
pub fn paint_zoom_cluster(
    ui: &mut egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    zoom: f32,
    compact: bool,
) {
    let radius = theme.corner_radius.value();
    let bw = theme.border_width.value();
    let fg = theme.dag_chrome_fg().to_egui();
    let border = theme.dag_chrome_border().to_egui();
    ui.painter()
        .rect_filled(rect, radius, theme.dag_chrome_bg().to_egui());
    ui.painter().rect_stroke(
        rect,
        radius,
        egui::Stroke::new(bw, border),
        egui::StrokeKind::Inside,
    );

    let side = theme.dag_chrome_height().value();
    let mut x = rect.min.x;
    let mut cell = |w: f32| {
        let r = egui::Rect::from_min_size(egui::pos2(x, rect.min.y), egui::vec2(w, side));
        x += w;
        r
    };

    let glyph_font = egui::FontId::monospace(theme.dag_chrome_height().value() / 2.0);
    let minus = cell(side);
    ui.painter().text(
        minus.center(),
        egui::Align2::CENTER_CENTER,
        "\u{2212}",
        glyph_font.clone(),
        fg,
    );
    if !compact {
        let readout = cell(ZOOM_READOUT_WIDTH.value());
        ui.painter().text(
            readout.center(),
            egui::Align2::CENTER_CENTER,
            format!("{}%", (zoom * 100.0).round() as i32),
            egui::FontId::monospace(theme.font_size_caption.value()),
            fg,
        );
    }
    let plus = cell(side);
    ui.painter().text(
        plus.center(),
        egui::Align2::CENTER_CENTER,
        "+",
        glyph_font,
        fg,
    );

    let sep = cell(theme.border_width.value());
    ui.painter().rect_filled(sep, 0.0, border);

    let icon_side = theme.dag_chrome_height().value() / 2.0;
    for icon in [icons::MOVE, icons::SWAP] {
        let c = cell(side).center();
        icon.image(icon_side, fg).paint_at(
            ui,
            egui::Rect::from_center_size(c, egui::vec2(icon_side, icon_side)),
        );
    }
}

/// 미니맵 — 노드를 **상태색**으로 칠해 건강 스트립을 겸한다.
pub fn paint_minimap(
    ui: &mut egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    graph: &Graph,
    layout: &GraphLayout,
    t: Transform,
    viewport: egui::Vec2,
) {
    let bw = theme.border_width.value();
    let radius = theme.corner_radius.value();
    ui.painter()
        .rect_filled(rect, radius, theme.dag_minimap_bg().to_egui());
    ui.painter().rect_stroke(
        rect,
        radius,
        egui::Stroke::new(bw, theme.dag_chrome_border().to_egui()),
        egui::StrokeKind::Inside,
    );

    let pad = theme.spacing_xs.value();
    let b = super::canvas::graph_bounds(layout, theme);
    let (gw, gh) = (b.width(), b.height());
    if gw <= 0.0 || gh <= 0.0 {
        return;
    }
    let k = ((rect.width() - pad * 2.0) / gw).min((rect.height() - pad * 2.0) / gh);
    let dx = rect.min.x + (rect.width() - gw * k) / 2.0 - b.min.x * k;
    let dy = rect.min.y + (rect.height() - gh * k) / 2.0 - b.min.y * k;
    let nw = (theme.dag_node_width().value() * k).max(theme.dag_edge_arrow_size().value() / 4.0);
    let nh = (theme.dag_node_height().value() * k).max(theme.dag_edge_arrow_size().value() / 4.0);

    for (i, p) in layout.nodes.iter().enumerate() {
        let status = graph.nodes[i].status;
        let mut c = status.accent(theme).to_egui();
        if status.is_dim() {
            c = c.gamma_multiply(super::edges::dim_factor());
        }
        ui.painter().rect_filled(
            egui::Rect::from_min_size(
                egui::pos2(dx + p.x.value() * k, dy + p.y.value() * k),
                egui::vec2(nw, nh),
            ),
            0.0,
            c,
        );
    }

    // 현재 보이는 창 — 그래프 좌표로 환산한 뒤 같은 배율로 접는다.
    let vp = egui::Rect::from_min_size(
        egui::pos2(
            dx + (-t.origin.x / t.zoom) * k,
            dy + (-t.origin.y / t.zoom) * k,
        ),
        egui::vec2((viewport.x / t.zoom) * k, (viewport.y / t.zoom) * k),
    );
    ui.painter().rect_stroke(
        vp,
        0.0,
        egui::Stroke::new(bw, theme.dag_minimap_viewport().to_egui()),
        egui::StrokeKind::Inside,
    );
}

/// 사이클 경고 — 캔버스 상단 전폭에 고정. 그래프는 뒤에서 계속 그려진다.
pub fn paint_cycle_banner(ui: &mut egui::Ui, theme: &Theme, canvas: egui::Rect, ids: &[String]) {
    let h = theme.dag_cycle_height().value();
    let rect = egui::Rect::from_min_size(canvas.min, egui::vec2(canvas.width(), h));
    ui.painter()
        .rect_filled(rect, 0.0, theme.dag_cycle_bg().to_egui());
    ui.painter().hline(
        rect.x_range(),
        rect.max.y,
        egui::Stroke::new(
            theme.border_width.value(),
            theme.dag_cycle_border().to_egui(),
        ),
    );

    let pad = theme.spacing_md.value();
    let gap = theme.spacing_sm.value();
    let icon_side = theme.dag_chrome_height().value() / 2.0;
    let fg = theme.dag_cycle_fg().to_egui();
    icons::ALERT_TRIANGLE.image(icon_side, fg).paint_at(
        ui,
        egui::Rect::from_center_size(
            egui::pos2(rect.min.x + pad + icon_side / 2.0, rect.center().y),
            egui::vec2(icon_side, icon_side),
        ),
    );
    let font = egui::FontId::proportional(theme.font_size_caption.value());
    let lead = format!(
        "Cycle detected — the runner will not advance {} tasks: ",
        ids.len()
    );
    let x = rect.min.x + pad + icon_side + gap;
    let lead_w = super::node::text_width(ui, &lead, &font);
    ui.painter().text(
        egui::pos2(x, rect.center().y),
        egui::Align2::LEFT_CENTER,
        &lead,
        font,
        fg,
    );
    // 경로는 mono + text-secondary 로 갈라 읽는다. 마지막에 첫 id 를 다시 붙여
    // "닫힌 고리" 임을 눈으로 보여준다.
    let path = format!("{} \u{2192} {}", ids.join(" \u{2192} "), ids[0]);
    ui.painter().text(
        egui::pos2(x + lead_w, rect.center().y),
        egui::Align2::LEFT_CENTER,
        path,
        egui::FontId::monospace(theme.font_size_caption.value()),
        theme.text_secondary().to_egui(),
    );
}

/// 좌하단 LOD 힌트 칩 — full 티어에서는 뜨지 않는다.
fn paint_lod_chip(ui: &mut egui::Ui, theme: &Theme, canvas: egui::Rect, lod: Lod) {
    let text = match lod {
        Lod::Full => return,
        Lod::Compact => "names only — zoom in for status",
        Lod::Block => "status blocks — zoom in for names",
    };
    let inset = theme.dag_chrome_inset().value();
    let pad = theme.spacing_sm.value();
    let h = theme.dag_runner_height().value();
    let font = egui::FontId::monospace(theme.font_size_micro.value());
    let w = super::node::text_width(ui, text, &font) + pad * 2.0;
    let rect = egui::Rect::from_min_size(
        egui::pos2(canvas.min.x + inset, canvas.max.y - inset - h),
        egui::vec2(w, h),
    );
    let radius = theme.corner_radius_sm.value();
    ui.painter()
        .rect_filled(rect, radius, theme.dag_chrome_bg().to_egui());
    ui.painter().rect_stroke(
        rect,
        radius,
        egui::Stroke::new(
            theme.border_width.value(),
            theme.dag_chrome_border().to_egui(),
        ),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        font,
        theme.text_muted().to_egui(),
    );
}

/// 우하단 오버레이 전체 + 좌하단 칩.
#[allow(clippy::too_many_arguments)]
pub fn paint_canvas_chrome(
    ui: &mut egui::Ui,
    theme: &Theme,
    canvas: egui::Rect,
    graph: &Graph,
    layout: &GraphLayout,
    t: Transform,
    minimap: bool,
    lod: Lod,
) {
    let inset = theme.dag_chrome_inset().value();
    let gap = theme.spacing_sm.value();
    let compact = canvas.width() < COMPACT_CUTOFF.value();
    let cluster = zoom_cluster_size(theme, compact);
    let cluster_rect = egui::Rect::from_min_size(
        egui::pos2(
            canvas.max.x - inset - cluster.x,
            canvas.max.y - inset - cluster.y,
        ),
        cluster,
    );
    paint_zoom_cluster(ui, theme, cluster_rect, t.zoom, compact);

    if minimap && canvas.width() >= theme.dag_minimap_min_surface().value() {
        let size = egui::vec2(
            theme.dag_minimap_width().value(),
            theme.dag_minimap_height().value(),
        );
        let rect = egui::Rect::from_min_size(
            egui::pos2(
                canvas.max.x - inset - size.x,
                cluster_rect.min.y - gap - size.y,
            ),
            size,
        );
        paint_minimap(ui, theme, rect, graph, layout, t, canvas.size());
    }

    paint_lod_chip(ui, theme, canvas, lod);
}

/// 빈 상태 — surface(워크스페이스에 DAG 없음) / search(필터 무매치).
pub fn paint_empty(ui: &mut egui::Ui, theme: &Theme, rect: egui::Rect, query: Option<&str>) {
    let (icon, title, body) = match query {
        Some(q) => (
            icons::SEARCH,
            format!("No DAGs match \u{201c}{q}\u{201d}"),
            "Clear the filter or widen the scope to all workspaces.".to_owned(),
        ),
        None => (
            icons::GIT_TREE,
            "No task DAGs in this workspace".to_owned(),
            "An agent creates one with tasty dag add; it appears here as soon as the host \
             registers it."
                .to_owned(),
        ),
    };
    let gap = theme.spacing_sm.value();
    let side = EMPTY_ICON_SIZE.value();
    let title_font = egui::FontId::proportional(theme.font_size_body.value());
    let title_h = super::node::row_height(ui, &title_font);
    let measure = theme
        .measure_sm
        .value()
        .min(rect.width() - theme.spacing_xl.value() * 2.0)
        .max(theme.spacing_xl.value());
    let body_galley = ui.painter().layout(
        body,
        egui::FontId::proportional(theme.font_size_caption.value()),
        theme.text_muted().to_egui(),
        measure,
    );

    // 아이콘 → 제목 → 본문을 한 덩어리로 묶어 무대 정중앙에 놓는다.
    let total = side + gap + title_h + gap + body_galley.size().y;
    let mut y = rect.center().y - total / 2.0;
    icon.image(side, theme.text_disabled().to_egui()).paint_at(
        ui,
        egui::Rect::from_min_size(
            egui::pos2(rect.center().x - side / 2.0, y),
            egui::vec2(side, side),
        ),
    );
    y += side + gap;
    ui.painter().text(
        egui::pos2(rect.center().x, y),
        egui::Align2::CENTER_TOP,
        &title,
        title_font,
        theme.text_secondary().to_egui(),
    );
    y += title_h + gap;
    ui.painter().galley(
        egui::pos2(rect.center().x - body_galley.size().x / 2.0, y),
        body_galley,
        theme.text_muted().to_egui(),
    );
}

/// `chrome` 섹션 Spec — 줌 클러스터 + 미니맵.
pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    let graph = super::build_dag();
    let layout = super::layout(&graph, theme, Orientation::TopDown);
    spec::stage(ui, theme, StageVariant::Solo, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = theme.spacing_xl.value();
            let size = egui::vec2(
                theme.dag_minimap_width().value(),
                theme.dag_minimap_height().value(),
            );
            let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
            let t = Transform {
                origin: egui::vec2(theme.spacing_xl.value(), theme.spacing_lg.value()),
                zoom: 0.8,
            };
            paint_minimap(ui, theme, rect, &graph, &layout, t, size * 3.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = theme.spacing_md.value();
                for compact in [false, true] {
                    let s = zoom_cluster_size(theme, compact);
                    let (r, _) = ui.allocate_exact_size(s, egui::Sense::hover());
                    paint_zoom_cluster(ui, theme, r, 0.8, compact);
                }
            });
        });
    });
    spec::meta(
        ui,
        theme,
        &[
            ("cluster", "28px row · 1px border"),
            ("inset", "8px from canvas edge"),
            ("minimap", "160 × 112"),
            ("viewport", "1px accent rect"),
            ("minimap cutoff", "surface < 560px"),
            ("compact cutoff", "surface < 400px"),
        ],
        &[
            TokenChip::new(
                "--tasty-dag-chrome-bg",
                "cluster fill",
                theme.dag_chrome_bg().to_egui(),
            ),
            TokenChip::new(
                "--tasty-dag-minimap-viewport",
                "viewport rect",
                theme.dag_minimap_viewport().to_egui(),
            ),
            TokenChip::new(
                "--tasty-dag-minimap-bg",
                "minimap bed",
                theme.dag_minimap_bg().to_egui(),
            ),
        ],
    );
    spec::note(
        ui,
        theme,
        "The minimap paints each node in its status colour, so it doubles as a health strip: \
         a red block anywhere in the graph is visible without zooming out.",
    );
}
