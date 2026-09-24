/// 팝업·토스트의 표시 범위와 화면 안 배치를 계산할 pane·surface·workspace 영역.
pub struct LayoutContext {
    pub active_workspace: usize,
    /// (pane_id, rect) for all visible panes.
    pub pane_rects: Vec<(u32, egui::Rect)>,
    /// (surface_id, rect) for all visible surfaces.
    pub surface_rects: Vec<(u32, egui::Rect)>,
    /// (pane_id, active_tab_index) for each pane.
    pub active_tabs: Vec<(u32, usize)>,
}

/// 현재 상태와 레이아웃으로 팝업·토스트·배너가 공유할 영역 정보를 만든다.
pub(crate) fn build_layout_context(
    state: &crate::state::AppState,
    engine: &crate::core::CoreState,
    pane_rects: &[(u32, crate::model::PhysicalRect)],
    terminal_rect: crate::model::PhysicalRect,
    scale_factor: f32,
) -> LayoutContext {
    let active_workspace = state.active_workspace;

    let pane_rects_logical: Vec<(u32, egui::Rect)> = pane_rects
        .iter()
        .map(|(id, r)| (*id, crate::adapters::ui::to_egui_rect(*r, scale_factor)))
        .collect();

    let mut surface_rects = Vec::new();
    for (_pane_id, _pane_rect, regions) in
        state.surface_regions(engine, terminal_rect, scale_factor)
    {
        for r in regions {
            surface_rects.push((
                r.id,
                crate::adapters::ui::to_egui_rect(r.rect, scale_factor),
            ));
        }
    }

    let mut active_tabs = Vec::new();
    let ws = state.active_workspace(engine);
    for &pid in &ws.pane_layout().all_pane_ids() {
        if let Some(pane) = ws.pane_layout().find_pane(pid) {
            active_tabs.push((pid, pane.active_tab));
        }
    }

    LayoutContext {
        active_workspace,
        pane_rects: pane_rects_logical,
        surface_rects,
        active_tabs,
    }
}
