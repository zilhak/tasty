//! 풀탭 서피스 — 디자인 `DagSurface` 의 구조 전사.
//!
//! 헤더(8/12 패딩) + 캔버스(남는 공간 전부) + 상세. **640px 아래**에서 헤더는
//! 두 줄로 접히고(정체성 위 · 컨트롤 아래), 러너 힌트가 사라지고, 미니맵이 빠지고,
//! 상세가 하단 시트로 내려간다. 좁은 쪽 바닥은 320px 이고 그때도 잘리는 것이 없다.

use tasty_dag_layout::Orientation;
use tasty_icons as icons;
use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{IconButton, select};

use super::detail::Dock;
use super::{Graph, canvas, detail, runner};
use crate::catalog::spec::{self, StageVariant, TokenChip};

/// 헤더가 두 줄로 접히는 폭 — 디자인 "wraps < 640".
const NARROW_CUTOFF: LogicalPx = LogicalPx(640.0);
/// 무대에서 좁은 변형에 주는 폭 — 디자인 "narrow floor 320px".
const NARROW_STAGE_WIDTH: LogicalPx = LogicalPx(320.0);
/// 무대 높이 — 시안 520.
const STAGE_HEIGHT: LogicalPx = LogicalPx(520.0);

/// 헤더가 몇 줄로 접히는지. 시안은 `flex-wrap: wrap` 이라 320px 에서는 컨트롤이
/// 한 번 더 접혀 3 줄이 된다 — 그 자동 줄바꿈을 명시적으로 계산한다.
fn header_rows(
    ui: &egui::Ui,
    theme: &Theme,
    runner_state: &super::Runner,
    width: f32,
    narrow: bool,
) -> u32 {
    if !narrow {
        return 1;
    }
    let inner = width - theme.spacing_md.value() * 2.0;
    let need = min_select_width(theme)
        + runner::badge_width(ui, theme, runner_state)
        + theme.dag_chrome_height().value()
        + theme.spacing_sm.value() * 2.0;
    if need > inner { 3 } else { 2 }
}

/// Select 가 이보다 좁아지면 값이 안 읽혀 컨트롤 구실을 못 한다.
fn min_select_width(theme: &Theme) -> f32 {
    theme.field_width_lg.value() / 2.0
}

fn header_height(theme: &Theme, rows: u32) -> f32 {
    let row = theme.dag_chrome_height().value();
    let n = rows as f32;
    row * n + theme.spacing_xs.value() * (n - 1.0) + theme.spacing_sm.value() * 2.0
}

/// 헤더 — 정체성(아이콘 · 이름 · task 수) + 컨트롤(Select · 러너 배지 · 새로고침).
fn header(
    ui: &mut egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    graphs: &[Graph],
    picked: &mut usize,
    rows: u32,
) {
    ui.painter()
        .rect_filled(rect, 0.0, theme.bg_sidebar().to_egui());
    ui.painter().hline(
        rect.x_range(),
        rect.max.y,
        egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
    );

    let narrow = rows > 1;
    let graph = &graphs[*picked];
    let pad_x = theme.spacing_md.value();
    let pad_y = theme.spacing_sm.value();
    let gap = theme.spacing_sm.value();
    let row_h = theme.dag_chrome_height().value();
    let inner = egui::Rect::from_min_max(
        egui::pos2(rect.min.x + pad_x, rect.min.y + pad_y),
        egui::pos2(rect.max.x - pad_x, rect.max.y - pad_y),
    );
    let line = |i: u32| {
        let y = inner.min.y + (row_h + theme.spacing_xs.value()) * i as f32;
        egui::Rect::from_min_size(egui::pos2(inner.min.x, y), egui::vec2(inner.width(), row_h))
    };

    // ── 1행: 정체성 ──
    let ident = line(0);
    let icon = theme.dag_canvas_dot_gap().value();
    icons::GIT_TREE
        .image(icon, theme.text_muted().to_egui())
        .paint_at(
            ui,
            egui::Rect::from_center_size(
                egui::pos2(ident.min.x + icon / 2.0, ident.center().y),
                egui::vec2(icon, icon),
            ),
        );
    let name_font = egui::FontId::proportional(theme.font_size_body.value());
    let count = format!("{} tasks", graph.nodes.len());
    let count_font = egui::FontId::monospace(theme.font_size_caption.value());
    let count_w = super::node::text_width(ui, &count, &count_font);
    let name_x = ident.min.x + icon + gap;
    let name_limit = if narrow {
        ident.max.x - count_w - gap - name_x
    } else {
        theme.field_width_lg.value()
    };
    let name = super::node::ellipsize(ui, &graph.name, &name_font, name_limit);
    let name_w = super::node::text_width(ui, &name, &name_font);
    ui.painter().text(
        egui::pos2(name_x, ident.center().y),
        egui::Align2::LEFT_CENTER,
        &name,
        name_font,
        theme.text_primary().to_egui(),
    );
    ui.painter().text(
        egui::pos2(name_x + name_w + gap, ident.center().y),
        egui::Align2::LEFT_CENTER,
        &count,
        count_font,
        theme.text_muted().to_egui(),
    );

    // ── 컨트롤 ──
    //
    // 시안은 flex(`Select` 가 `1 1 auto`)로 남는 폭을 나눠 가진다. egui 에는 그
    // 협상이 없으니 고정 폭(배지 · 새로고침 · 간격)을 먼저 빼고 남은 만큼을
    // Select 에 준다 — 320px 에서도 서로 겹치지 않는 유일한 순서다.
    let runner_state = &graphs[*picked].runner;
    let badge_w = runner::badge_width(ui, theme, runner_state);
    let refresh_w = row_h;
    let (select_rect, trailing) = match rows {
        1 => {
            let x = name_x + name_w + gap + count_w + gap;
            let r = egui::Rect::from_min_size(
                egui::pos2(x, inner.min.y),
                egui::vec2(theme.field_width_lg.value(), row_h),
            );
            (
                r,
                egui::Rect::from_min_max(
                    egui::pos2(r.max.x + gap, inner.min.y),
                    egui::pos2(inner.max.x, inner.min.y + row_h),
                ),
            )
        }
        2 => {
            let row = line(1);
            let w = (row.width() - badge_w - refresh_w - gap * 2.0).max(min_select_width(theme));
            let r = egui::Rect::from_min_size(row.min, egui::vec2(w, row_h));
            (
                r,
                egui::Rect::from_min_max(egui::pos2(r.max.x + gap, row.min.y), row.max),
            )
        }
        _ => {
            let r = line(1);
            (r, line(2))
        }
    };

    let mut picker = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(select_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    let labels: Vec<String> = graphs
        .iter()
        .map(|g| format!("{} / {}", g.workspace, g.name))
        .collect();
    let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    select(
        &mut picker,
        theme,
        "dag_surface_pick",
        picked,
        &refs,
        select_rect.width(),
        true,
    );

    let badge_rect = egui::Rect::from_min_size(
        egui::pos2(
            trailing.min.x,
            trailing.center().y - theme.dag_runner_height().value() / 2.0,
        ),
        egui::vec2(
            badge_w.min(trailing.width()),
            theme.dag_runner_height().value(),
        ),
    );
    runner::paint_badge(ui, theme, badge_rect, runner_state);
    // 재개 힌트는 넓은 헤더에서만 — 좁으면 알약만 남는다.
    if !narrow && runner::wants_hint(runner_state) {
        let font = egui::FontId::proportional(theme.font_size_caption.value());
        let avail = trailing.max.x - refresh_w - gap - (badge_rect.max.x + gap);
        ui.painter().text(
            egui::pos2(badge_rect.max.x + gap, trailing.center().y),
            egui::Align2::LEFT_CENTER,
            super::node::ellipsize(ui, runner::RESUME_HINT, &font, avail),
            font,
            theme.text_muted().to_egui(),
        );
    }

    let refresh_rect = egui::Rect::from_min_size(
        egui::pos2(trailing.max.x - refresh_w, trailing.min.y),
        egui::vec2(refresh_w, row_h),
    );
    let mut refresh = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(refresh_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    IconButton::new().show(&mut refresh, theme, &|ui, rect, c| {
        icons::REFRESH.image(rect.height(), c).paint_at(ui, rect);
    });
}

/// 서피스 한 장 — 헤더 + 캔버스 + (선택 시) 상세.
pub fn paint(
    ui: &mut egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    graphs: &[Graph],
    salt: &str,
    initial: Option<&str>,
) {
    let narrow = rect.width() < NARROW_CUTOFF.value();
    let pick_key = ui.id().with(("dag_surface_pick", salt));
    let sel_key = ui.id().with(("dag_surface_sel", salt));
    let mut picked: usize = ui.data(|d| d.get_temp(pick_key)).unwrap_or(0);
    let mut sel: Option<String> = ui
        .data(|d| d.get_temp(sel_key))
        .unwrap_or_else(|| initial.map(str::to_owned));

    ui.painter()
        .rect_filled(rect, 0.0, theme.bg_panel().to_egui());

    let rows = header_rows(ui, theme, &graphs[picked].runner, rect.width(), narrow);
    let hh = header_height(theme, rows);
    let header_rect = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), hh));
    header(ui, theme, header_rect, graphs, &mut picked, rows);
    picked = picked.min(graphs.len() - 1);
    let graph = &graphs[picked];
    if sel.as_deref().is_some_and(|s| graph.node(s).is_none()) {
        sel = None;
    }

    let body = egui::Rect::from_min_max(egui::pos2(rect.min.x, header_rect.max.y), rect.max);
    let (canvas_rect, detail_rect) = match (&sel, narrow) {
        (None, _) => (body, None),
        (Some(_), false) => {
            let w = theme.dag_detail_width().value().min(body.width() / 2.0);
            (
                egui::Rect::from_min_max(body.min, egui::pos2(body.max.x - w, body.max.y)),
                Some(egui::Rect::from_min_max(
                    egui::pos2(body.max.x - w, body.min.y),
                    body.max,
                )),
            )
        }
        (Some(_), true) => {
            let h = theme
                .dag_detail_sheet_height()
                .value()
                .min(body.height() / 2.0);
            (
                egui::Rect::from_min_max(body.min, egui::pos2(body.max.x, body.max.y - h)),
                Some(egui::Rect::from_min_max(
                    egui::pos2(body.min.x, body.max.y - h),
                    body.max,
                )),
            )
        }
    };

    canvas::paint(
        ui,
        theme,
        canvas_rect,
        graph,
        Orientation::TopDown,
        &mut sel,
        !narrow,
    );

    if let (Some(dr), Some(id)) = (detail_rect, sel.clone()) {
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(dr)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        child.set_clip_rect(dr);
        let dock = if narrow { Dock::Sheet } else { Dock::Side };
        if let Some(jump) = detail::draw_docked(&mut child, theme, graph, &id, dock, dr.size()) {
            sel = Some(jump);
        }
    }

    ui.data_mut(|d| {
        d.insert_temp(pick_key, picked);
        d.insert_temp(sel_key, sel);
    });
}

/// `surfaces` 섹션 Spec — 넓은 서피스 + 320px 좁은 서피스.
pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    let graphs = vec![
        super::build_dag(),
        super::index_dag(),
        super::dense_dag(),
        super::cycle_dag(),
    ];
    let narrow_graphs = vec![super::build_dag(), super::index_dag()];
    let h = STAGE_HEIGHT.value();
    spec::stage(ui, theme, StageVariant::Tight, |ui| {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = theme.spacing_lg.value();
            let narrow_w = NARROW_STAGE_WIDTH.value();
            let wide_w = (ui.available_width() - narrow_w - theme.spacing_lg.value()).max(narrow_w);
            for (graphs, w, salt, initial) in [
                (&graphs, wide_w, "wide", Some("build_linux")),
                (&narrow_graphs, narrow_w, "narrow", Some("unit")),
            ] {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
                let radius = theme.corner_radius.value();
                ui.painter().rect_stroke(
                    rect,
                    radius,
                    egui::Stroke::new(theme.border_width.value(), theme.border_strong().to_egui()),
                    egui::StrokeKind::Inside,
                );
                let mut child = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(rect.shrink(theme.border_width.value()))
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                child.set_clip_rect(rect);
                // 두 서피스가 같은 Spec 안에 있어 위젯 id 가 겹친다 — 서피스마다
                // id scope 를 따로 판다(Select · IconButton · ScrollArea 전부 포함).
                child.push_id(salt, |ui| {
                    paint(
                        ui,
                        theme,
                        rect.shrink(theme.border_width.value()),
                        graphs,
                        salt,
                        initial,
                    );
                });
            }
        });
    });
    spec::meta(
        ui,
        theme,
        &[
            ("header", "8/12 padding · wraps < 640"),
            ("canvas", "all remaining space"),
            ("detail", "288 side → 220 sheet"),
            ("narrow floor", "320px, nothing clipped"),
            ("DAG switch", "instant swap + auto-fit"),
        ],
        &[
            TokenChip::new(
                "--tasty-bg-sidebar",
                "header band",
                theme.bg_sidebar().to_egui(),
            ),
            TokenChip::new(
                "--tasty-separator",
                "header hairline",
                theme.separator.to_egui(),
            ),
            TokenChip::new(
                "--tasty-dag-detail-sheet-height",
                "220 sheet",
                theme.dag_detail_bg().to_egui(),
            ),
        ],
    );
    spec::note(
        ui,
        theme,
        "Switching DAG in the header Select replaces the canvas instantly (0ms) and re-fits — a new \
         graph has no position to preserve, so fit is the honest default. Within one DAG the view \
         never re-fits on its own.",
    );
    spec::note(
        ui,
        theme,
        "Both variants open with a task already selected so the two docking modes are visible side \
         by side: 288px side panel on the wide surface, 220px bottom sheet on the 320px one. \
         The host opens with no selection and no detail.",
    );
}
