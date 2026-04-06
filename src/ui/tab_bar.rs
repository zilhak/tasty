use egui::emath::GuiRounding as _;

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
                logical_x: (pane_rect.x / scale_factor).round_ui(),
                logical_y: (pane_rect.y / scale_factor).round_ui(),
                logical_w: (pane_rect.width / scale_factor).round_ui(),
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
                    .inner_margin(egui::Margin::ZERO)
                    .show(ui, |ui| {
                        ui.set_min_width(info.logical_w);
                        ui.set_max_width(info.logical_w);

                        let tab_w = 150.0 / scale_factor;
                        let bar_h = state.tab_bar_height / scale_factor;

                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;

                            for (i, name) in info.tab_names.iter().enumerate() {
                                // 1px vertical border between tabs
                                if i > 0 {
                                    let rect = ui.allocate_space(egui::vec2(1.0, bar_h)).1;
                                    ui.painter().rect_filled(rect, 0.0, th.surface0);
                                }

                                let is_active = i == info.active_tab;
                                let tab_bg = if is_active { th.base } else { bg };
                                let text_color = if is_active { th.text } else { th.subtext0 };

                                let (rect, response) = ui.allocate_exact_size(
                                    egui::vec2(tab_w, bar_h),
                                    egui::Sense::click(),
                                );

                                ui.painter().rect_filled(rect, 0.0, tab_bg);

                                // Active tab: 2px accent line at bottom
                                if is_active {
                                    let line_rect = egui::Rect::from_min_size(
                                        egui::pos2(rect.min.x, rect.max.y - 2.0),
                                        egui::vec2(rect.width(), 2.0),
                                    );
                                    ui.painter().rect_filled(line_rect, 0.0, th.blue);
                                }

                                ui.painter().text(
                                    rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    name,
                                    egui::FontId::proportional(11.0),
                                    text_color,
                                );

                                if response.clicked() {
                                    actions.push((info.pane_id, PaneTabAction::SwitchTab(i)));
                                }
                            }

                            // 1px border before + button
                            {
                                let rect = ui.allocate_space(egui::vec2(1.0, bar_h)).1;
                                ui.painter().rect_filled(rect, 0.0, th.surface0);
                            }

                            // "+" button — same height, narrow width
                            let (plus_rect, plus_resp) = ui.allocate_exact_size(
                                egui::vec2(28.0, bar_h),
                                egui::Sense::click(),
                            );
                            if plus_resp.hovered() {
                                ui.painter().rect_filled(plus_rect, 0.0, th.surface0);
                            }
                            ui.painter().text(
                                plus_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                "+",
                                egui::FontId::proportional(13.0),
                                th.subtext0,
                            );
                            if plus_resp.clicked() {
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
