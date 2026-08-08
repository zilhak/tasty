/// Layout context carrying pane/surface/workspace geometry for scope-based
/// visibility and clamping.  Consumed by PopupManager, ToastManager, and any
/// future system that needs to resolve scope → screen rect.
pub struct LayoutContext {
    pub active_workspace: usize,
    /// (pane_id, rect) for all visible panes.
    pub pane_rects: Vec<(u32, egui::Rect)>,
    /// (surface_id, rect) for all visible surfaces.
    pub surface_rects: Vec<(u32, egui::Rect)>,
    /// (pane_id, active_tab_index) for each pane.
    pub active_tabs: Vec<(u32, usize)>,
}

/// Build a [`LayoutContext`] from current [`AppState`]/[`CoreState`] and layout
/// info. `popup::frame::draw_popup_layer` 와 `overlay::draw_overlays` 가 같은
/// 프레임에 공유하므로(둘 다 popup/toast/banner scope 해석에 필요), 어느 한쪽에
/// 속하는 대신 이 타입 자체와 같은 위치에 둔다.
pub(crate) fn build_layout_context(
    state: &crate::state::AppState,
    engine: &crate::core::CoreState,
    pane_rects: &[(u32, crate::model::PhysicalRect)],
    terminal_rect: crate::model::PhysicalRect,
    scale_factor: f32,
) -> LayoutContext {
    let active_workspace = state.active_workspace;

    // Convert physical pixel pane rects to logical pixel egui rects
    let pane_rects_logical: Vec<(u32, egui::Rect)> = pane_rects
        .iter()
        .map(|(id, r)| {
            (
                *id,
                egui::Rect::from_min_size(
                    egui::pos2(r.x.value() / scale_factor, r.y.value() / scale_factor),
                    egui::vec2(
                        r.width.value() / scale_factor,
                        r.height.value() / scale_factor,
                    ),
                ),
            )
        })
        .collect();

    // Compute surface rects using surface_regions
    let mut surface_rects = Vec::new();
    for (_pane_id, _pane_rect, regions) in state.surface_regions(engine, terminal_rect) {
        for r in regions {
            surface_rects.push((
                r.id,
                egui::Rect::from_min_size(
                    egui::pos2(
                        r.rect.x.value() / scale_factor,
                        r.rect.y.value() / scale_factor,
                    ),
                    egui::vec2(
                        r.rect.width.value() / scale_factor,
                        r.rect.height.value() / scale_factor,
                    ),
                ),
            ));
        }
    }

    // Collect active tab indices
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
