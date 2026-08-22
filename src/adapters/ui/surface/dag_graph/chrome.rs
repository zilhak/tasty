//! 캔버스를 둘러싼 것들 — 헤더 · 러너 배지 · 줌 클러스터 · 미니맵 · 사이클 배너 ·
//! LOD 칩 · 빈 상태.
//!
//! 좁은 폭에서는 **정보 밀도가 아니라 우선순위** 로 접는다. 러너 배지(그래프가
//! 진행 중인가)와 사이클 배너(그래프가 애초에 돌 수 없는가)는 마지막까지 남고,
//! 줌 퍼센트 숫자와 미니맵처럼 없어도 조작이 가능한 것부터 사라진다.

use tasty_dag_layout::GraphLayout;
use tasty_model::DagDirection;
use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{ControlSize, IconButton, IconButtonVariant, hspace, margin_sym};

use super::model::{DagData, DagGraphData, RunnerBadgeData};
use super::view::{DagGraphView, Lod, ZOOM_MAX, ZOOM_MIN};
use crate::adapters::ui::icons;
use crate::i18n::{t, t_fmt, t_fmt2};

/// 헤더에서 나온 조작.
pub enum ChromeAction {
    SelectDag(String),
    ToggleDirection,
    Fit,
    /// 줌 단계 변경. 앵커는 헤더가 아니라 **캔버스 중심**이어야 하므로 여기서
    /// 적용하지 않고 캔버스 크기를 아는 호출자에게 넘긴다.
    Zoom(f32),
}

/// 상단 헤더 — DAG 선택 · 진척 · 러너 배지 · 줌 클러스터.
pub fn draw_header(
    ui: &mut egui::Ui,
    theme: &Theme,
    data: &DagData,
    view: &mut DagGraphView,
    direction: DagDirection,
) -> Option<ChromeAction> {
    let mut action = None;
    let width = ui.available_width();

    egui::Frame::NONE
        .fill(theme.dag_chrome_bg().to_egui())
        .inner_margin(margin_sym(theme.dag_chrome_inset(), theme.spacing_xs))
        .show(ui, |ui| {
            ui.set_height(theme.dag_chrome_height().value());
            ui.horizontal_centered(|ui| {
                if let Some(picked) = dag_picker(ui, theme, data) {
                    action = Some(ChromeAction::SelectDag(picked));
                }
                if let Some(graph) = &data.current {
                    hspace(ui, theme.spacing_sm);
                    ui.label(
                        egui::RichText::new(t_fmt2(
                            "dag.header.progress",
                            &graph.done.to_string(),
                            &graph.total().to_string(),
                        ))
                        .size(theme.dag_row_count_font_size().value())
                        .color(theme.dag_row_count_fg().to_egui()),
                    );
                }
                hspace(ui, theme.spacing_sm);
                runner_badge(ui, theme, &data.runner);

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(a) = zoom_cluster(ui, theme, view, direction, width) {
                        action = Some(a);
                    }
                });
            });
        });

    // 헤더 ↔ 캔버스 구분선.
    let (sep, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().hline(
        sep.x_range(),
        sep.center().y,
        egui::Stroke::new(
            theme.border_width.value(),
            theme.dag_chrome_border().to_egui(),
        ),
    );
    action
}

/// DAG 선택 드롭다운. 목록이 하나뿐이면 그냥 이름만 보인다.
fn dag_picker(ui: &mut egui::Ui, theme: &Theme, data: &DagData) -> Option<String> {
    let current_name = data
        .current
        .as_ref()
        .map(|g| g.name.clone())
        .unwrap_or_else(|| t("dag.header.no_dag").to_string());

    if data.dags.len() <= 1 {
        ui.label(
            egui::RichText::new(current_name)
                .size(theme.font_size_body.value())
                .color(theme.dag_chrome_fg().to_egui()),
        );
        return None;
    }

    let mut picked = None;
    egui::ComboBox::from_id_salt("dag_picker")
        .selected_text(current_name)
        .show_ui(ui, |ui| {
            for entry in &data.dags {
                let selected = data.current.as_ref().is_some_and(|g| g.id == entry.id);
                let label = t_fmt2(
                    "dag.header.entry",
                    &entry.name,
                    &entry.task_count.to_string(),
                );
                let text = egui::RichText::new(format!("{} {}", entry.rollup.glyph(), label))
                    .size(theme.font_size_caption.value());
                if ui.selectable_label(selected, text).clicked() {
                    picked = Some(entry.id.clone());
                }
            }
        });
    picked
}

/// 러너 배지. `stalled`(할 일이 있는데 아무도 안 돌린다)는 1급 경고다 — 이 화면이
/// 없으면 사용자는 "왜 안 도는지" 를 CLI 로 파헤쳐야 한다.
fn runner_badge(ui: &mut egui::Ui, theme: &Theme, runner: &RunnerBadgeData) {
    let stalled = runner.is_stalled();
    let (bg, border, fg, text) = if runner.crashed {
        (
            theme.dag_runner_crashed_bg(),
            theme.dag_runner_crashed_border(),
            theme.dag_runner_crashed_fg(),
            t("dag.runner.crashed").to_string(),
        )
    } else if stalled {
        (
            theme.dag_runner_stalled_bg(),
            theme.dag_runner_stalled_border(),
            theme.dag_runner_stalled_fg(),
            t_fmt("dag.runner.stalled", &runner.ready.to_string()),
        )
    } else if runner.running {
        (
            theme.dag_runner_bg(),
            theme.dag_runner_border(),
            theme.dag_runner_fg(),
            t_fmt("dag.runner.running", &runner.running_count.to_string()),
        )
    } else {
        (
            theme.dag_runner_bg(),
            theme.dag_runner_border(),
            theme.dag_runner_idle_fg(),
            t("dag.runner.idle").to_string(),
        )
    };

    let resp = egui::Frame::NONE
        .fill(bg.to_egui())
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            border.to_egui(),
        ))
        .corner_radius(theme.dag_runner_radius().value())
        .inner_margin(margin_sym(theme.dag_runner_padding_x(), theme.spacing_xs))
        .show(ui, |ui| {
            ui.set_height(theme.dag_runner_height().value());
            ui.label(
                egui::RichText::new(text)
                    .size(theme.font_size_caption.value())
                    .color(fg.to_egui()),
            );
        })
        .response;

    if runner.crashed || stalled {
        // 실행 가능한 복구 수단을 툴팁에 그대로 적는다. 이 화면에는 러너를 켜는
        // 버튼이 없다 — 관찰 전용 surface 라 상태를 바꾸지 않는다.
        resp.on_hover_text(t("dag.runner.resume_hint"));
    }
}

/// 줌 클러스터 — `−  %  +  |  fit  dir`. 우측 정렬 레이아웃 안에서 호출되므로
/// **역순**으로 배치한다.
fn zoom_cluster(
    ui: &mut egui::Ui,
    theme: &Theme,
    view: &DagGraphView,
    direction: DagDirection,
    surface_width: f32,
) -> Option<ChromeAction> {
    let mut action = None;

    let dir_icon = match direction {
        DagDirection::LeftRight => icons::ARROW_RIGHT,
        DagDirection::TopDown => icons::ARROW_DOWN,
    };
    if icon_btn(ui, theme, dir_icon, true)
        .on_hover_text(t("dag.zoom.direction"))
        .clicked()
    {
        action = Some(ChromeAction::ToggleDirection);
    }
    if icon_btn(ui, theme, icons::FIT, true)
        .on_hover_text(t("dag.zoom.fit"))
        .clicked()
    {
        action = Some(ChromeAction::Fit);
    }
    hspace(ui, theme.spacing_xs);

    if icon_btn(ui, theme, icons::PLUS, view.zoom < ZOOM_MAX)
        .on_hover_text(t("dag.zoom.in"))
        .clicked()
    {
        action = Some(ChromeAction::Zoom(1.0));
    }
    // 퍼센트 숫자는 좁은 폭에서 제일 먼저 접는다 — ± 버튼만 있어도 조작은 된다.
    if surface_width >= NARROW_ZOOM_LABEL.value() {
        ui.label(
            egui::RichText::new(format!("{}%", (view.zoom * 100.0).round() as i32))
                .size(theme.font_size_caption.value())
                .color(theme.dag_chrome_fg().to_egui()),
        );
    }
    if icon_btn(ui, theme, icons::MINUS, view.zoom > ZOOM_MIN)
        .on_hover_text(t("dag.zoom.out"))
        .clicked()
    {
        action = Some(ChromeAction::Zoom(-1.0));
    }
    action
}

/// 줌 클러스터의 ghost 아이콘 버튼 한 개.
fn icon_btn(ui: &mut egui::Ui, theme: &Theme, icon: icons::Icon, enabled: bool) -> egui::Response {
    IconButton::new()
        .variant(IconButtonVariant::Ghost)
        .size(ControlSize::Sm)
        .enabled(enabled)
        .show(ui, theme, &|ui, rect, c| {
            icon.image(rect.height(), c).paint_at(ui, rect)
        })
}

/// 줌 퍼센트 라벨을 유지하는 최소 surface 폭.
///
/// 미니맵의 `dag-minimap-min-surface` 와 달리 이 둘은 대응 디자인 토큰이 없다 —
/// 시안이 좁은 폭 축약을 미니맵에 대해서만 명시했다. 토큰이 생기면 `Theme` 에서
/// 가져오도록 바꾼다.
const NARROW_ZOOM_LABEL: LogicalPx = LogicalPx(400.0);
/// 상세를 우측 패널로 둘 수 있는 최소 surface 폭. 그 아래는 하단 시트다.
pub const NARROW_DETAIL_SHEET: LogicalPx = LogicalPx(640.0);

/// 사이클 배너. 사이클이면 그래프는 그리되 "이 그래프는 완주할 수 없다" 를 알린다.
pub fn draw_cycle_banner(ui: &mut egui::Ui, theme: &Theme, cycle: &[String]) {
    egui::Frame::NONE
        .fill(theme.dag_cycle_bg().to_egui())
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            theme.dag_cycle_border().to_egui(),
        ))
        .inner_margin(margin_sym(theme.dag_chrome_inset(), theme.spacing_xs))
        .show(ui, |ui| {
            ui.set_height(theme.dag_cycle_height().value());
            ui.horizontal_centered(|ui| {
                ui.label(
                    egui::RichText::new(t_fmt("dag.cycle.banner", &cycle.join(" → ")))
                        .size(theme.font_size_caption.value())
                        .color(theme.dag_cycle_fg().to_egui()),
                );
            });
        });
}

/// 좌하단 LOD 칩 — 지금 카드가 왜 축약돼 보이는지 알린다.
pub fn paint_lod_chip(painter: &egui::Painter, theme: &Theme, rect: egui::Rect, lod: Lod) {
    if lod == Lod::Full {
        return;
    }
    let text = match lod {
        Lod::Compact => t("dag.lod.compact"),
        _ => t("dag.lod.block"),
    };
    let pad = theme.spacing_xs.value();
    let font = egui::FontId::proportional(theme.font_size_micro.value());
    let galley = painter.layout_no_wrap(text.to_string(), font, theme.dag_chrome_fg().to_egui());
    let size = galley.size() + egui::vec2(pad * 2.0, pad);
    let chip = egui::Rect::from_min_size(
        egui::pos2(rect.min.x + pad, rect.max.y - size.y - pad),
        size,
    );
    painter.rect_filled(
        chip,
        theme.dag_runner_radius().value(),
        theme.dag_chrome_bg().to_egui(),
    );
    painter.galley(
        chip.min + egui::vec2(pad, pad / 2.0),
        galley,
        theme.dag_chrome_fg().to_egui(),
    );
}

/// 우하단 미니맵. surface 가 좁으면 그리지 않는다 — 캔버스를 가리는 손해가 크다.
pub fn paint_minimap(
    painter: &egui::Painter,
    theme: &Theme,
    rect: egui::Rect,
    view: &DagGraphView,
    layout: &GraphLayout,
    graph_size: egui::Vec2,
) {
    if rect.width() < theme.dag_minimap_min_surface().value()
        || graph_size.x <= 0.0
        || graph_size.y <= 0.0
    {
        return;
    }
    let pad = theme.spacing_sm.value();
    let size = egui::vec2(
        theme.dag_minimap_width().value(),
        theme.dag_minimap_height().value(),
    );
    let map = egui::Rect::from_min_size(
        egui::pos2(rect.max.x - size.x - pad, rect.max.y - size.y - pad),
        size,
    );
    painter.rect_filled(
        map,
        theme.dag_node_radius().value(),
        theme.dag_minimap_bg().to_egui(),
    );

    let k = (size.x / graph_size.x).min(size.y / graph_size.y);
    let base = map.min + (size - graph_size * k) / 2.0;
    let node = theme.dag_minimap_node().to_egui();
    let (nw, nh) = (
        theme.dag_node_width().value() * k,
        theme.dag_node_height().value() * k,
    );
    for n in &layout.nodes {
        painter.rect_filled(
            egui::Rect::from_min_size(
                base + egui::vec2(n.x.value() * k, n.y.value() * k),
                egui::vec2(nw.max(1.0), nh.max(1.0)),
            ),
            0.0,
            node,
        );
    }

    // 현재 뷰포트가 그래프의 어디를 보고 있는지. 화면 좌표 → 그래프 좌표 역변환.
    let vp = egui::Rect::from_min_size(
        base + egui::vec2(
            -view.offset.x / view.zoom * k,
            -view.offset.y / view.zoom * k,
        ),
        egui::vec2(rect.width() / view.zoom * k, rect.height() / view.zoom * k),
    );
    painter.rect_stroke(
        vp.intersect(map),
        0.0,
        egui::Stroke::new(
            theme.border_width.value(),
            theme.dag_minimap_viewport().to_egui(),
        ),
        egui::StrokeKind::Inside,
    );
}

/// 빈 상태 2 종 — "workspace 에 DAG 가 없다" 와 "지정한 DAG 가 사라졌다".
pub fn draw_empty(ui: &mut egui::Ui, theme: &Theme, data: &DagData, dag_id: Option<&str>) {
    let (title, hint) = if data.target_missing {
        (
            t_fmt("dag.empty.missing", dag_id.unwrap_or("")),
            t("dag.empty.missing_hint").to_string(),
        )
    } else {
        (
            t("dag.empty.none").to_string(),
            t_fmt("dag.empty.none_hint", &data.workspace_id.to_string()),
        )
    };
    egui::Frame::NONE
        .fill(theme.dag_canvas_bg().to_egui())
        .show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() / 3.0);
                ui.label(
                    egui::RichText::new(title)
                        .size(theme.font_size_body.value())
                        .color(theme.text_primary().to_egui()),
                );
                ui.label(
                    egui::RichText::new(hint)
                        .size(theme.font_size_caption.value())
                        .color(theme.text_muted().to_egui()),
                );
            });
        });
}

/// 그래프 데이터가 비어 있는지(노드 0 개).
pub fn is_empty(graph: Option<&DagGraphData>) -> bool {
    graph.is_none_or(|g| g.nodes.is_empty())
}
