use egui::emath::GuiRounding as _;

use crate::model::Rect;
use crate::state::AppState;
use crate::theme;

/// Render non-terminal panels (Markdown, Explorer) using egui.
/// These panels don't have a wgpu terminal renderer; they are fully egui-based.
pub fn draw_non_terminal_panels(
    ctx: &egui::Context,
    state: &mut AppState,
    pane_rects: &[(u32, Rect)],
    scale_factor: f32,
) {
    let th = theme::theme();
    // First pass: gather info about non-terminal panels (read-only).
    struct NonTerminalInfo {
        pane_id: u32,
        logical_x: f32,
        logical_y: f32,
        logical_w: f32,
        logical_h: f32,
    }

    let mut infos = Vec::new();
    {
        let ws = state.active_workspace();
        for &(pane_id, pane_rect) in pane_rects {
            let pane = match ws.pane_layout().find_pane(pane_id) {
                Some(p) => p,
                None => continue,
            };
            let panel = match pane.active_panel() {
                Some(p) => p,
                None => continue,
            };
            if !panel.is_non_terminal() {
                continue;
            }
            let tab_bar_h = state.tab_bar_height;
            infos.push(NonTerminalInfo {
                pane_id,
                logical_x: (pane_rect.x / scale_factor).round_ui(),
                logical_y: ((pane_rect.y + tab_bar_h) / scale_factor).round_ui(),
                logical_w: (pane_rect.width / scale_factor).round_ui(),
                logical_h: ((pane_rect.height - tab_bar_h).max(1.0) / scale_factor).round_ui(),
            });
        }
    }

    // Second pass: render each non-terminal panel.
    for info in &infos {
        let ws = state.active_workspace_mut();
        let pane = match ws.pane_layout_mut().find_pane_mut(info.pane_id) {
            Some(p) => p,
            None => continue,
        };
        let tab = match pane.active_tab_mut() {
            Some(t) => t,
            None => continue,
        };

        match tab.panel_mut() {
            crate::model::Panel::Markdown(md_panel) => {
                egui::Area::new(egui::Id::new(format!("md_panel_{}", info.pane_id)))
                    .fixed_pos(egui::pos2(info.logical_x, info.logical_y))
                    .order(egui::Order::Background)
                    .show(ctx, |ui| {
                        ui.set_min_size(egui::vec2(info.logical_w, info.logical_h));
                        ui.set_max_size(egui::vec2(info.logical_w, info.logical_h));
                        egui::Frame::new()
                            .fill(th.crust)
                            .inner_margin(egui::Margin::same(8))
                            .show(ui, |ui| {
                                egui::ScrollArea::vertical()
                                    .id_salt(format!("md_scroll_{}", info.pane_id))
                                    .show(ui, |ui| {
                                        // Clone content to avoid borrow issues
                                        let content = md_panel.content.clone();
                                        crate::markdown_ui::render_markdown(ui, &content);
                                    });
                            });
                    });
            }
            crate::model::Panel::Explorer(exp_panel) => {
                egui::Area::new(egui::Id::new(format!("explorer_{}", info.pane_id)))
                    .fixed_pos(egui::pos2(info.logical_x, info.logical_y))
                    .order(egui::Order::Background)
                    .show(ctx, |ui| {
                        ui.set_min_size(egui::vec2(info.logical_w, info.logical_h));
                        ui.set_max_size(egui::vec2(info.logical_w, info.logical_h));
                        egui::Frame::new()
                            .fill(th.crust)
                            .inner_margin(egui::Margin::same(4))
                            .show(ui, |ui| {
                                crate::explorer_ui::draw_explorer(ui, exp_panel);
                            });
                    });
            }
            _ => {}
        }
    }
}
