use std::time::Instant;

use crate::i18n::{t, t_fmt};
use crate::state::AppState;
use crate::theme;

/// Draw the notification panel overlay (toggled with Ctrl+I).
pub fn draw_notification_panel(ctx: &egui::Context, state: &mut AppState) {
    let th = theme::theme();
    if !state.notification_panel_open {
        return;
    }

    let mut open = state.notification_panel_open;

    egui::Window::new(t("notification_panel.window_title"))
        .open(&mut open)
        .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-8.0, 8.0))
        .default_width(350.0)
        .default_height(400.0)
        .resizable(true)
        .collapsible(false)
        .show(ctx, |ui| {
            // Header with mark-all-read button
            ui.horizontal(|ui| {
                let unread = state.engine.notifications.unread_count();
                ui.label(
                    egui::RichText::new(t_fmt("notification_panel.unread_count", &unread.to_string()))
                        .small()
                        .color(egui::Color32::GRAY),
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
                                    .color(egui::Color32::GRAY),
                            );
                        });
                        return;
                    }

                    // Collect notification info for display (iterate in reverse for newest first)
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

                            // Find workspace name
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
                            egui::Color32::from_rgba_unmultiplied(137, 180, 250, 20) // Blue at ~8%
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
                                                    .color(egui::Color32::GRAY),
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

                    // Apply actions
                    if let Some(id) = mark_read_id {
                        state.engine.notifications.mark_read(id);
                    }
                    if let Some(ws_id) = jump_to_ws {
                        if let Some(idx) = state.engine.workspaces.iter().position(|ws| ws.id == ws_id) {
                            state.switch_workspace(idx);
                        }
                    }
                });
        });

    state.notification_panel_open = open;
}
