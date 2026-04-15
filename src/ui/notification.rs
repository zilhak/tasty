use std::time::Instant;

use crate::i18n::{t, t_fmt};
use crate::state::AppState;
use crate::theme;
use crate::ui::convert_popup;

/// Draw notification panel content inside a popup Ui.
fn draw_notification_content(ui: &mut egui::Ui, state: &mut AppState) {
    let th = theme::theme();

    // Header with mark-all-read button
    ui.horizontal(|ui| {
        let unread = state.engine.notifications.unread_count();
        ui.label(
            egui::RichText::new(t_fmt("notification_panel.unread_count", &unread.to_string()))
                .small()
                .color(th.subtext0),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button(t("button.mark_all_read")).clicked() {
                state.engine.notifications.mark_all_read();
            }
        });
    });
    ui.separator();

    // Scrollable notification list (newest first)
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let notification_count = state.engine.notifications.all().len();
            if notification_count == 0 {
                ui.centered_and_justified(|ui| {
                    ui.label(
                        egui::RichText::new(t("notification_panel.empty_message"))
                            .color(th.subtext0),
                    );
                });
                return;
            }

            let now = Instant::now();
            let entries: Vec<_> = state.engine.notifications.all()
                .rev()
                .map(|n| {
                    let elapsed = now.duration_since(n.timestamp);
                    let time_str = if elapsed.as_secs() < 60 {
                        format!("{}s ago", elapsed.as_secs())
                    } else if elapsed.as_secs() < 3600 {
                        format!("{}m ago", elapsed.as_secs() / 60)
                    } else {
                        format!("{}h ago", elapsed.as_secs() / 3600)
                    };

                    let ws_name = state
                        .engine.workspaces
                        .iter()
                        .find(|ws| ws.id == n.source_workspace)
                        .map(|ws| ws.name.as_str())
                        .unwrap_or("Unknown");

                    (
                        n.id,
                        n.read,
                        n.title.clone(),
                        n.body.clone(),
                        time_str,
                        ws_name.to_string(),
                        n.source_workspace,
                    )
                })
                .collect();

            let mut mark_read_id = None;
            let mut jump_to_ws = None;

            for (id, read, title, body, time_str, ws_name, ws_id) in &entries {
                let bg = if *read {
                    egui::Color32::TRANSPARENT
                } else {
                    egui::Color32::from_rgba_unmultiplied(137, 180, 250, 20)
                };

                egui::Frame::new()
                    .fill(bg)
                    .inner_margin(egui::Margin::same(4))
                    .corner_radius(4.0)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            if !*read {
                                ui.label(
                                    egui::RichText::new("*")
                                        .color(th.blue)
                                        .strong(),
                                );
                            }
                            ui.label(egui::RichText::new(title).strong().small());
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        egui::RichText::new(time_str)
                                            .small()
                                            .color(th.subtext0),
                                    );
                                },
                            );
                        });

                        if !body.is_empty() {
                            ui.label(egui::RichText::new(body).small());
                        }

                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(ws_name)
                                    .small()
                                    .color(th.blue),
                            );

                            if ui
                                .small_button(t("button.jump_to_workspace"))
                                .on_hover_text(t("tooltip.jump_to_workspace"))
                                .clicked()
                            {
                                jump_to_ws = Some(*ws_id);
                                mark_read_id = Some(*id);
                            }
                        });
                    });

                ui.add_space(2.0);
            }

            if let Some(id) = mark_read_id {
                state.engine.notifications.mark_read(id);
            }
            if let Some(ws_id) = jump_to_ws {
                if let Some(idx) = state.engine.workspaces.iter().position(|ws| ws.id == ws_id) {
                    state.switch_workspace(idx);
                }
            }
        });
}

/// Draw all popups via the PopupManager. Called from egui_bridge.
pub fn draw_popups(
    ctx: &egui::Context,
    state: &mut AppState,
    pane_rects: &[(u32, crate::model::Rect)],
    terminal_rect: crate::model::Rect,
    scale_factor: f32,
) {
    // Build scope context for popup visibility/clamping
    let draw_ctx = build_popup_draw_ctx(state, pane_rects, terminal_rect, scale_factor);

    // Update convert_surface popup title (i18n may change)
    if let Some(p) = state.popups.get_mut("convert_surface") {
        p.title = t("convert_popup.title").to_string();
    }

    // Temporarily take the popup manager to avoid borrow conflicts
    // (popup manager needs &mut, and content callbacks need &mut state).
    let mut popups = std::mem::replace(&mut state.popups, crate::ui::PopupManager::new());

    let mut convert_result: Option<convert_popup::ConvertResult> = None;
    let closed = popups.draw(ctx, &mut |id, ui| {
        match id {
            "notifications" => draw_notification_content(ui, state),
            "convert_surface" => {
                convert_result = convert_popup::draw_convert_content(ui, state);
            }
            _ => {}
        }
    }, Some(&draw_ctx));

    state.popups = popups;

    // Handle convert popup close (via X button, outside click, or Escape)
    let convert_closed = closed.contains(&"convert_surface")
        || matches!(convert_result, Some(convert_popup::ConvertResult::Close));
    if convert_closed {
        state.popups.close("convert_surface");
        state.dialogs.convert_popup = None;
        state.dialogs.convert_popup_selected = None;
    }

    // Handle convert popup action
    if let Some(convert_popup::ConvertResult::Action(action)) = convert_result {
        convert_popup::apply_convert_action(state, action);
        state.popups.close("convert_surface");
        state.dialogs.convert_popup = None;
        state.dialogs.convert_popup_selected = None;
    }
}

/// Build PopupDrawContext from current AppState and layout info.
fn build_popup_draw_ctx(
    state: &AppState,
    pane_rects: &[(u32, crate::model::Rect)],
    terminal_rect: crate::model::Rect,
    scale_factor: f32,
) -> crate::ui::PopupDrawContext {
    let active_workspace = state.active_workspace;

    // Convert physical pixel pane rects to logical pixel egui rects
    let pane_rects_logical: Vec<(u32, egui::Rect)> = pane_rects
        .iter()
        .map(|(id, r)| {
            (*id, egui::Rect::from_min_size(
                egui::pos2(r.x / scale_factor, r.y / scale_factor),
                egui::vec2(r.width / scale_factor, r.height / scale_factor),
            ))
        })
        .collect();

    // Compute surface rects from render_regions
    let mut surface_rects = Vec::new();
    let regions = state.render_regions(terminal_rect);
    for (pane_id, pane_rect, terminal_regions) in &regions {
        if terminal_regions.is_empty() {
            // Non-terminal panel (Markdown, Explorer, Html, Empty):
            // the surface fills the pane content area (pane rect minus tab bar)
            let ws = state.active_workspace();
            if let Some(pane) = ws.pane_layout().find_pane(*pane_id) {
                if let Some(panel) = pane.active_panel() {
                    if let Some(sid) = panel.focused_surface_id() {
                        let tab_bar_h = state.tab_bar_height;
                        let content_rect = egui::Rect::from_min_size(
                            egui::pos2(pane_rect.x / scale_factor, (pane_rect.y + tab_bar_h) / scale_factor),
                            egui::vec2(pane_rect.width / scale_factor, ((pane_rect.height - tab_bar_h).max(1.0)) / scale_factor),
                        );
                        surface_rects.push((sid, content_rect));
                    }
                }
            }
        } else {
            for (sid, _term, rect) in terminal_regions {
                surface_rects.push((*sid, egui::Rect::from_min_size(
                    egui::pos2(rect.x / scale_factor, rect.y / scale_factor),
                    egui::vec2(rect.width / scale_factor, rect.height / scale_factor),
                )));
            }
        }
    }

    // Collect active tab indices
    let mut active_tabs = Vec::new();
    let ws = state.active_workspace();
    for &pid in &ws.pane_layout().all_pane_ids() {
        if let Some(pane) = ws.pane_layout().find_pane(pid) {
            active_tabs.push((pid, pane.active_tab));
        }
    }

    crate::ui::PopupDrawContext {
        active_workspace,
        pane_rects: pane_rects_logical,
        surface_rects,
        active_tabs,
    }
}
