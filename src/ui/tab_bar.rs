use crate::model::Rect;
use crate::state::AppState;
use crate::theme;

/// Draw per-pane tab bars using egui Areas positioned at each pane's top.
/// This is called during the egui frame (from gpu.rs render).
pub fn draw_pane_tab_bars(
    ctx: &egui::Context,
    state: &mut AppState,
    pane_rects: &[(u32, Rect)],
    scale_factor: f32,
) {
    let th = theme::theme();
    let focused_pane_id = state.focused_pane_id();

    // First pass: gather tab info (read-only) and render UI, collecting user actions.
    struct PaneTabInfo {
        pane_id: u32,
        tab_names: Vec<String>,
        active_tab: usize,
        is_focused: bool,
        logical_x: f32,
        logical_y: f32,
        logical_w: f32,
    }

    let mut infos = Vec::new();
    {
        let ws = state.active_workspace();
        for &(pane_id, pane_rect) in pane_rects {
            let pane = match ws.pane_layout().find_pane(pane_id) {
                Some(p) => p,
                None => continue,
            };
            // Always show tab bar, even with a single tab
            infos.push(PaneTabInfo {
                pane_id,
                tab_names: pane.tabs.iter().map(|t| t.name.clone()).collect(),
                active_tab: pane.active_tab,
                is_focused: pane_id == focused_pane_id,
                logical_x: pane_rect.x / scale_factor,
                logical_y: pane_rect.y / scale_factor,
                logical_w: pane_rect.width / scale_factor,
            });
        }
    }

    // Second pass: render egui and collect actions.
    let mut actions: Vec<(u32, PaneTabAction)> = Vec::new();

    let mut measured_tab_bar_height: Option<f32> = None;

    for info in &infos {
        let area_response = egui::Area::new(egui::Id::new(format!("pane_tabs_{}", info.pane_id)))
            .fixed_pos(egui::pos2(info.logical_x, info.logical_y))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                let bg = if info.is_focused {
                    th.surface0
                } else {
                    th.mantle
                };

                egui::Frame::new()
                    .fill(bg)
                    .inner_margin(egui::Margin::symmetric(4, 2))
                    .show(ui, |ui| {
                        // Force the frame content to fill the full pane width
                        // (subtract inner_margin: 4px left + 4px right = 8px)
                        let inner_w = info.logical_w - 8.0;
                        ui.set_min_width(inner_w);
                        ui.set_max_width(inner_w);

                        ui.horizontal(|ui| {
                            for (i, name) in info.tab_names.iter().enumerate() {
                                let is_active = i == info.active_tab;
                                let label = if is_active {
                                    egui::RichText::new(name).strong().small()
                                } else {
                                    egui::RichText::new(name).small()
                                };

                                if ui.selectable_label(is_active, label).clicked() {
                                    actions.push((info.pane_id, PaneTabAction::SwitchTab(i)));
                                }
                            }

                            if ui.small_button("+").clicked() {
                                actions.push((info.pane_id, PaneTabAction::AddTab));
                            }
                        });
                    });
            });

        // Measure actual tab bar height from the first rendered tab bar
        if measured_tab_bar_height.is_none() {
            let logical_h = area_response.response.rect.height();
            measured_tab_bar_height = Some(logical_h * scale_factor);
        }
    }

    // Update state with measured tab bar height
    if let Some(h) = measured_tab_bar_height {
        state.tab_bar_height = h;
    }

    // Third pass: apply actions.
    for (pane_id, action) in actions {
        match action {
            PaneTabAction::SwitchTab(idx) => {
                if let Some(pane) = state.active_workspace_mut().pane_layout_mut().find_pane_mut(pane_id) {
                    pane.active_tab = idx;
                }
            }
            PaneTabAction::AddTab => {
                // Focus this pane first, then add tab
                state.active_workspace_mut().focused_pane = pane_id;
                let _ = state.add_tab();
            }
        }
    }
}

enum PaneTabAction {
    SwitchTab(usize),
    AddTab,
}
