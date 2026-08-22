//! 최상위 조립 — 헤더 + (사이클 배너) + 캔버스 + 상세.
//!
//! 상세는 넓은 화면에서 우측 고정폭 패널, 좁은 화면에서 하단 시트다. **콘텐츠는
//! 어느 쪽이든 [`super::detail::draw_detail`] 하나**이고 여기서는 자리만 다르게 준다.

use tasty_model::DagGraphSurface;

use super::chrome::{self, ChromeAction, NARROW_DETAIL_SHEET};
use super::detail::{DetailAction, DetailDock, dock_divider, draw_detail};
use super::view::{DagGraphView, layout_config};

/// DAG surface 한 개를 그린다.
pub fn draw_dag_graph(ui: &mut egui::Ui, panel: &mut DagGraphSurface, view: &mut DagGraphView) {
    let th = crate::theme::theme();
    let theme = &th;
    ui.set_min_size(ui.available_size());
    ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);

    let Some(data) = view.data.clone() else {
        // 첫 폴링 전 한 프레임. 여기서 스피너를 돌리면 0.5 초짜리 깜빡임만 남는다.
        ui.painter()
            .rect_filled(ui.max_rect(), 0.0, theme.dag_canvas_bg().to_egui());
        return;
    };

    let surface_width = ui.available_width();
    let now = now_ms();

    let chrome_action = chrome::draw_header(ui, theme, &data, view, panel.direction, surface_width);

    if chrome::is_empty(data.current.as_ref()) {
        chrome::draw_empty(ui, theme, &data, panel.dag_id.as_deref());
        apply_chrome(
            panel,
            view,
            chrome_action,
            egui::Vec2::ZERO,
            egui::Vec2::ZERO,
            theme,
        );
        return;
    }
    // 위에서 비었으면 이미 반환했다.
    let graph = data
        .current
        .as_ref()
        .expect("non-empty graph checked above");

    if let Some(cycle) = &graph.cycle {
        chrome::draw_cycle_banner(ui, theme, cycle);
    }

    let cfg = layout_config(theme, panel.direction);
    // 캐시 조회 — 그래프 모양이 그대로면 좌표를 다시 계산하지 않는다.
    let layout = view.layout(panel.direction, &cfg);
    let graph_size = egui::vec2(layout.width.value(), layout.height.value());

    let selected = view
        .selected
        .as_deref()
        .and_then(|id| graph.index_of(id))
        .and_then(|i| graph.nodes.get(i));
    let show_detail = selected.is_some();
    let wide = surface_width >= NARROW_DETAIL_SHEET.value();

    let mut detail_action = None;
    let mut viewport = ui.available_size();

    if show_detail && !wide {
        // 좁은 화면: 캔버스(위) + 하단 시트. 경계선은 시트 **위쪽 가로선**이다.
        let sheet_h = theme.dag_detail_sheet_height().value();
        let canvas_h = (ui.available_height() - sheet_h).max(0.0);
        viewport = egui::vec2(ui.available_width(), canvas_h);
        ui.allocate_ui(viewport, |ui| {
            canvas(ui, theme, view, &data, &layout, panel, now);
        });
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), sheet_h),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                let dock = ui.available_rect_before_wrap();
                if let Some(node) = selected {
                    detail_action = draw_detail(ui, theme, graph, node, now);
                }
                dock_divider(ui.painter(), theme, dock, DetailDock::Sheet);
            },
        );
    } else if show_detail {
        // 넓은 화면: 캔버스 | 우측 패널. 경계선은 패널 **왼쪽 세로선**이다.
        let panel_w = theme.dag_detail_width().value();
        let canvas_w = (ui.available_width() - panel_w).max(0.0);
        viewport = egui::vec2(canvas_w, ui.available_height());
        ui.horizontal_top(|ui| {
            ui.allocate_ui(viewport, |ui| {
                canvas(ui, theme, view, &data, &layout, panel, now);
            });
            ui.allocate_ui_with_layout(
                egui::vec2(panel_w, ui.available_height()),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    let dock = ui.available_rect_before_wrap();
                    if let Some(node) = selected {
                        detail_action = draw_detail(ui, theme, graph, node, now);
                    }
                    dock_divider(ui.painter(), theme, dock, DetailDock::Side);
                },
            );
        });
    } else {
        canvas(ui, theme, view, &data, &layout, panel, now);
    }

    if let Some(DetailAction::Select(id)) = detail_action {
        view.selected = Some(id);
    }
    apply_chrome(panel, view, chrome_action, graph_size, viewport, theme);
}

#[allow(clippy::too_many_arguments)]
fn canvas(
    ui: &mut egui::Ui,
    theme: &tasty_type_appearance::theme::Theme,
    view: &mut DagGraphView,
    data: &super::model::DagData,
    layout: &tasty_dag_layout::GraphLayout,
    panel: &DagGraphSurface,
    now: u64,
) {
    let Some(graph) = data.current.as_ref() else {
        return;
    };
    super::canvas::draw_canvas(ui, theme, view, graph, layout, panel.direction, now);
}

fn apply_chrome(
    panel: &mut DagGraphSurface,
    view: &mut DagGraphView,
    action: Option<ChromeAction>,
    graph_size: egui::Vec2,
    viewport: egui::Vec2,
    theme: &tasty_type_appearance::theme::Theme,
) {
    match action {
        Some(ChromeAction::SelectDag(id)) => {
            // 사용자가 명시적으로 고른 순간부터 대상이 고정된다 — 폴링의 자동 선택이
            // 더 이상 개입하지 않는다.
            panel.dag_id = Some(id);
            view.selected = None;
        }
        Some(ChromeAction::ToggleDirection) => {
            // 선택은 유지한다 — 같은 그래프를 다른 축으로 다시 그릴 뿐이라, 보던
            // 노드를 놓치게 만들 이유가 없다. auto-fit 키에 방향이 들어 있어 프레이밍
            // 은 다음 프레임에 새로 맞춰진다.
            panel.direction = panel.direction.toggled();
        }
        Some(ChromeAction::Fit) => {
            view.fit(graph_size, viewport, theme.dag_canvas_padding().value());
        }
        Some(ChromeAction::Zoom(steps)) => {
            // 헤더 버튼으로 줌할 때 고정되는 점은 캔버스 한가운데다 — 보고 있던
            // 부분이 화면 밖으로 밀려나지 않는다.
            view.zoom_by(steps, viewport / 2.0);
        }
        None => {}
    }
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
