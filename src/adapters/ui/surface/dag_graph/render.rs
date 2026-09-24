//! 최상위 조립 — 헤더 + (사이클 배너) + 캔버스 + 상세.
//!
//! 상세는 넓은 화면에서 우측 고정폭 패널, 좁은 화면에서 하단 시트다. **콘텐츠는
//! 어느 쪽이든 [`super::detail::draw_detail`] 하나**이고 여기서는 자리만 다르게 준다.

use super::chrome::{self, ChromeAction, NARROW_DETAIL_SHEET};
use super::detail::{DetailAction, DetailDock, dock_divider, draw_detail};
use super::view::{DagGraphView, DagTarget, layout_config};

/// surface는 자체 헤더·줌 버튼을 그리고 팝업은 back bar의 버튼 동작을 받아 처리한다.
pub enum DagChrome {
    /// 탭 surface — 헤더 띠 + 캔버스 위 줌 클러스터를 이 함수가 그린다.
    Own,
    /// 본문보다 먼저 처리된 팝업 back bar 동작.
    BackBar(Option<ChromeAction>),
}

/// 공용 그래프 렌더링. 화면 폭에 따라 상세를 오른쪽 또는 아래에 배치한다.
pub fn draw_dag_graph(
    ui: &mut egui::Ui,
    target: DagTarget<'_>,
    view: &mut DagGraphView,
    chrome_mode: DagChrome,
) {
    let th = crate::theme::theme();
    let theme = &th;
    ui.set_min_size(ui.available_size());
    ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);

    let Some(data) = view.data.clone() else {
        ui.painter()
            .rect_filled(ui.max_rect(), 0.0, theme.dag_canvas_bg().to_egui());
        return;
    };

    let surface_width = ui.available_width();
    let now = now_ms();

    let own_chrome = matches!(chrome_mode, DagChrome::Own);
    let chrome_action = match chrome_mode {
        DagChrome::Own => chrome::draw_header(ui, theme, &data, surface_width),
        DagChrome::BackBar(pending) => pending,
    };

    if chrome::is_empty(data.current.as_ref()) {
        chrome::draw_empty(ui, theme, &data, target.dag_id.as_deref());
        apply_chrome(
            target,
            view,
            chrome_action,
            egui::Vec2::ZERO,
            egui::Vec2::ZERO,
            theme,
        );
        return;
    }
    let graph = data
        .current
        .as_ref()
        .expect("non-empty graph checked above");

    if let Some(cycle) = &graph.cycle {
        chrome::draw_cycle_banner(ui, theme, cycle);
    }

    let direction = *target.direction;
    let cfg = layout_config(theme, direction);
    let layout = view.layout(direction, &cfg);
    let graph_size = egui::vec2(layout.width.value(), layout.height.value());

    let selected = view
        .selected
        .as_deref()
        .and_then(|id| graph.index_of(id))
        .and_then(|i| graph.nodes.get(i));
    let show_detail = selected.is_some();
    let wide = surface_width >= NARROW_DETAIL_SHEET.value();

    let mut detail_action = None;
    let mut canvas_action = None;
    let mut viewport = ui.available_size();

    if show_detail && !wide {
        // 좁은 화면: 캔버스(위) + 하단 시트. 경계선은 시트 **위쪽 가로선**이다.
        let sheet_h = theme.dag_detail_sheet_height().value();
        let canvas_h = (ui.available_height() - sheet_h).max(0.0);
        viewport = egui::vec2(ui.available_width(), canvas_h);
        ui.allocate_ui(viewport, |ui| {
            canvas_action = canvas(ui, theme, view, &data, &layout, direction, now, own_chrome);
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
                canvas_action = canvas(ui, theme, view, &data, &layout, direction, now, own_chrome);
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
        canvas_action = canvas(ui, theme, view, &data, &layout, direction, now, own_chrome);
    }

    match detail_action {
        Some(DetailAction::Select(id)) => view.selected = Some(id),
        Some(DetailAction::Close) => view.selected = None,
        None => {}
    }
    apply_chrome(
        target,
        view,
        canvas_action.or(chrome_action),
        graph_size,
        viewport,
        theme,
    );
}

#[allow(clippy::too_many_arguments)]
fn canvas(
    ui: &mut egui::Ui,
    theme: &tasty_type_appearance::theme::Theme,
    view: &mut DagGraphView,
    data: &super::model::DagData,
    layout: &tasty_dag_layout::GraphLayout,
    direction: tasty_model::DagDirection,
    now: u64,
    own_zoom_cluster: bool,
) -> Option<ChromeAction> {
    let graph = data.current.as_ref()?;
    super::canvas::draw_canvas(
        ui,
        theme,
        view,
        graph,
        layout,
        direction,
        now,
        own_zoom_cluster,
    )
}

fn apply_chrome(
    target: DagTarget<'_>,
    view: &mut DagGraphView,
    action: Option<ChromeAction>,
    graph_size: egui::Vec2,
    viewport: egui::Vec2,
    theme: &tasty_type_appearance::theme::Theme,
) {
    match action {
        Some(ChromeAction::SelectDag(id)) => {
            // 사용자가 고른 DAG로 대상을 고정하고 다음 프레임에 즉시 조회한다.
            *target.dag_id = Some(id);
            view.selected = None;
            view.invalidate_poll();
        }
        Some(ChromeAction::ToggleDirection) => {
            // 방향이 바뀌어도 선택은 유지하고 자동 맞춤은 다음 프레임에 다시 계산한다.
            *target.direction = target.direction.toggled();
        }
        Some(ChromeAction::Fit) => {
            view.fit(graph_size, viewport, theme.dag_canvas_padding().value());
        }
        Some(ChromeAction::Zoom(steps)) => {
            view.zoom_by(steps, viewport / 2.0);
        }
        Some(ChromeAction::Refresh) => {
            view.invalidate_poll();
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
