use std::time::Instant;

use crate::i18n::{t, t_fmt};
use crate::state::AppState;
use crate::theme;

/// Draw notification panel content inside a popup Ui.
/// Also called by NotificationPopup's PopupContent impl.
pub(crate) fn draw_notification_content_inner(ui: &mut egui::Ui, state: &mut AppState) {
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

    // Update popup titles/sizes from trait objects (e.g. i18n changes)
    for content in &state.popup_contents {
        if let Some(p) = state.popups.get_mut(content.id()) {
            p.title = content.title();
            p.size = content.default_size();
        }
    }

    // Temporarily take the popup manager and contents to avoid borrow conflicts
    let mut popups = std::mem::replace(&mut state.popups, crate::ui::PopupManager::new());
    let mut contents = std::mem::replace(&mut state.popup_contents, Vec::new());

    let mut trait_closed: Vec<&'static str> = Vec::new();
    let closed = popups.draw(ctx, &mut |id, ui| {
        if let Some(content) = contents.iter_mut().find(|c| c.id() == id) {
            let content_id = content.id();
            if matches!(content.draw(ui, state), crate::ui::PopupAction::Close) {
                trait_closed.push(content_id);
            }
        }
    }, Some(&draw_ctx));

    state.popups = popups;
    state.popup_contents = contents;

    // Close popups requested by trait dispatch or X button / outside click
    for id in trait_closed.iter().chain(closed.iter()) {
        state.popups.close(id);
    }

    // Clean up convert_surface dialog state when closed
    let convert_closed = trait_closed.contains(&"convert_surface")
        || closed.contains(&"convert_surface");
    if convert_closed {
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
                if let Some(tab) = pane.tabs.get(pane.active_tab) {
                    if let Some(sid) = tab.surface().focused_surface_id() {
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
