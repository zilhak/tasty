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

    struct PaneTabInfo {
        pane_id: u32,
        tab_names: Vec<String>,
        active_tab: usize,
        is_focused: bool,
        logical_x: f32,
        logical_y: f32,
        logical_w: f32,
        scroll_offset: f32,
    }

    let mut infos = Vec::new();
    {
        let ws = state.active_workspace();
        for &(pane_id, pane_rect) in pane_rects {
            let pane = match ws.pane_layout().find_pane(pane_id) {
                Some(p) => p,
                None => continue,
            };
            infos.push(PaneTabInfo {
                pane_id,
                tab_names: pane.tabs.iter().map(|t| t.name.clone()).collect(),
                active_tab: pane.active_tab,
                is_focused: pane_id == focused_pane_id,
                logical_x: (pane_rect.x / scale_factor).round_ui(),
                logical_y: (pane_rect.y / scale_factor).round_ui(),
                logical_w: (pane_rect.width / scale_factor).round_ui(),
                scroll_offset: pane.tab_scroll_offset,
            });
        }
    }

    let mut actions: Vec<(u32, PaneTabAction)> = Vec::new();
    let mut measured_tab_bar_height: Option<f32> = None;

    let tab_w: f32 = 150.0 / scale_factor;
    let plus_w: f32 = 28.0;
    let arrow_w: f32 = 20.0;
    let separator_w: f32 = 1.0;

    for info in &infos {
        let bar_h = state.tab_bar_height / scale_factor;
        let n = info.tab_names.len();
        // Total content width: tabs + separators + separator before "+" + "+"
        let content_w = n as f32 * tab_w + (n.max(1) - 1) as f32 * separator_w + separator_w + plus_w;
        let needs_scroll = content_w > info.logical_w;
        // Available width for tab content (minus arrows if scrolling)
        let viewport_w = if needs_scroll {
            info.logical_w - arrow_w * 2.0
        } else {
            info.logical_w
        };
        let max_scroll = (content_w - viewport_w).max(0.0);
        let scroll = info.scroll_offset.clamp(0.0, max_scroll);

        let area_response = egui::Area::new(egui::Id::new(format!("pane_tabs_{}", info.pane_id)))
            .fixed_pos(egui::pos2(info.logical_x, info.logical_y))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                let bg = if info.is_focused { th.surface0 } else { th.mantle };

                egui::Frame::new()
                    .fill(bg)
                    .inner_margin(egui::Margin::ZERO)
                    .show(ui, |ui| {
                        ui.set_min_width(info.logical_w);
                        ui.set_max_width(info.logical_w);
                        ui.set_min_height(bar_h);
                        ui.set_max_height(bar_h);

                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;

                            // Left arrow
                            if needs_scroll {
                                let can_left = scroll > 0.0;
                                let (r, resp) = ui.allocate_exact_size(
                                    egui::vec2(arrow_w, bar_h), egui::Sense::click(),
                                );
                                let arrow_color = if can_left { th.subtext0 } else { th.surface1 };
                                if resp.hovered() && can_left {
                                    ui.painter().rect_filled(r, 0.0, th.surface0);
                                }
                                ui.painter().text(
                                    r.center(), egui::Align2::CENTER_CENTER,
                                    "<", egui::FontId::proportional(11.0), arrow_color,
                                );
                                if resp.clicked() && can_left {
                                    actions.push((info.pane_id, PaneTabAction::ScrollLeft));
                                }
                            }

                            // Clipped tab area
                            let clip_start_x = ui.cursor().min.x;
                            let clip_rect = egui::Rect::from_min_size(
                                egui::pos2(clip_start_x, ui.cursor().min.y),
                                egui::vec2(viewport_w, bar_h),
                            );
                            // Reserve the viewport space
                            ui.allocate_exact_size(egui::vec2(viewport_w, bar_h), egui::Sense::hover());

                            // Draw tabs inside the clip rect using painter with clip
                            let painter = ui.painter().with_clip_rect(clip_rect);
                            let mut x = clip_start_x - scroll;

                            for (i, name) in info.tab_names.iter().enumerate() {
                                if i > 0 {
                                    // 1px separator
                                    let sep = egui::Rect::from_min_size(
                                        egui::pos2(x, clip_rect.min.y),
                                        egui::vec2(separator_w, bar_h),
                                    );
                                    painter.rect_filled(sep, 0.0, th.surface0);
                                    x += separator_w;
                                }

                                let is_active = i == info.active_tab;
                                let tab_bg = if is_active { th.base } else { bg };
                                let text_color = if is_active { th.text } else { th.subtext0 };

                                let tab_rect = egui::Rect::from_min_size(
                                    egui::pos2(x, clip_rect.min.y),
                                    egui::vec2(tab_w, bar_h),
                                );

                                painter.rect_filled(tab_rect, 0.0, tab_bg);

                                if is_active {
                                    let line_rect = egui::Rect::from_min_size(
                                        egui::pos2(tab_rect.min.x, tab_rect.min.y),
                                        egui::vec2(tab_w, 2.0),
                                    );
                                    painter.rect_filled(line_rect, 0.0, th.blue);
                                }

                                painter.text(
                                    tab_rect.center(), egui::Align2::CENTER_CENTER,
                                    name, egui::FontId::proportional(11.0), text_color,
                                );

                                // Click detection via ctx input
                                let tab_clip = tab_rect.intersect(clip_rect);
                                if !tab_clip.is_negative() {
                                    let resp = ui.interact(tab_clip, egui::Id::new(format!("tab_{}_{}", info.pane_id, i)), egui::Sense::click());
                                    if resp.clicked() {
                                        actions.push((info.pane_id, PaneTabAction::SwitchTab(i)));
                                    }
                                }

                                x += tab_w;
                            }

                            // Separator before "+"
                            {
                                let sep = egui::Rect::from_min_size(
                                    egui::pos2(x, clip_rect.min.y),
                                    egui::vec2(separator_w, bar_h),
                                );
                                painter.rect_filled(sep, 0.0, th.surface0);
                                x += separator_w;
                            }

                            // "+" button
                            {
                                let plus_rect = egui::Rect::from_min_size(
                                    egui::pos2(x, clip_rect.min.y),
                                    egui::vec2(plus_w, bar_h),
                                );
                                let plus_clip = plus_rect.intersect(clip_rect);
                                if !plus_clip.is_negative() {
                                    let resp = ui.interact(plus_clip, egui::Id::new(format!("tab_plus_{}", info.pane_id)), egui::Sense::click());
                                    if resp.hovered() {
                                        painter.rect_filled(plus_rect, 0.0, th.surface0);
                                    }
                                    painter.text(
                                        plus_rect.center(), egui::Align2::CENTER_CENTER,
                                        "+", egui::FontId::proportional(13.0), th.subtext0,
                                    );
                                    if resp.clicked() {
                                        actions.push((info.pane_id, PaneTabAction::AddTab));
                                    }
                                }
                            }

                            // Right arrow
                            if needs_scroll {
                                let can_right = scroll < max_scroll;
                                let (r, resp) = ui.allocate_exact_size(
                                    egui::vec2(arrow_w, bar_h), egui::Sense::click(),
                                );
                                let arrow_color = if can_right { th.subtext0 } else { th.surface1 };
                                if resp.hovered() && can_right {
                                    ui.painter().rect_filled(r, 0.0, th.surface0);
                                }
                                ui.painter().text(
                                    r.center(), egui::Align2::CENTER_CENTER,
                                    ">", egui::FontId::proportional(11.0), arrow_color,
                                );
                                if resp.clicked() && can_right {
                                    actions.push((info.pane_id, PaneTabAction::ScrollRight));
                                }
                            }
                        });
                    });
            });

        if measured_tab_bar_height.is_none() {
            let logical_h = area_response.response.rect.height();
            measured_tab_bar_height = Some(logical_h * scale_factor);
        }
    }

    if let Some(h) = measured_tab_bar_height {
        state.tab_bar_height = h;
    }

    // Apply actions
    for (pane_id, action) in actions {
        match action {
            PaneTabAction::SwitchTab(idx) => {
                if let Some(pane) = state.active_workspace_mut().pane_layout_mut().find_pane_mut(pane_id) {
                    pane.active_tab = idx;
                }
            }
            PaneTabAction::AddTab => {
                state.active_workspace_mut().focused_pane = pane_id;
                let _ = state.add_tab();
            }
            PaneTabAction::ScrollLeft => {
                if let Some(pane) = state.active_workspace_mut().pane_layout_mut().find_pane_mut(pane_id) {
                    pane.tab_scroll_offset = (pane.tab_scroll_offset - tab_w).max(0.0);
                }
            }
            PaneTabAction::ScrollRight => {
                if let Some(pane) = state.active_workspace_mut().pane_layout_mut().find_pane_mut(pane_id) {
                    pane.tab_scroll_offset += tab_w;
                }
            }
        }
    }
}

enum PaneTabAction {
    SwitchTab(usize),
    AddTab,
    ScrollLeft,
    ScrollRight,
}
