use std::time::Instant;

use crate::model::Rect;
use crate::state::AppState;

/// Width of the sidebar in logical pixels.
const SIDEBAR_WIDTH: f32 = 180.0;

/// Color for notification badge / highlight.
const NOTIFICATION_COLOR: egui::Color32 = egui::Color32::from_rgb(80, 140, 255);

/// Render the egui UI and return the remaining terminal area rect (in physical pixels).
pub fn draw_ui(ctx: &egui::Context, state: &mut AppState, scale_factor: f32) -> Rect {
    // ---- Left sidebar: workspaces ----
    egui::SidePanel::left("workspace_sidebar")
        .exact_width(SIDEBAR_WIDTH)
        .resizable(false)
        .show(ctx, |ui| {
            ui.vertical(|ui| {
                ui.add_space(8.0);

                // Header with notification badge
                ui.horizontal(|ui| {
                    ui.heading("Workspaces");
                    let unread = state.notifications.unread_count();
                    if unread > 0 {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let badge_text = if unread > 99 {
                                "99+".to_string()
                            } else {
                                unread.to_string()
                            };
                            let badge = egui::RichText::new(badge_text)
                                .small()
                                .strong()
                                .color(egui::Color32::WHITE)
                                .background_color(NOTIFICATION_COLOR);
                            ui.label(badge);
                        });
                    }
                });

                ui.add_space(4.0);
                ui.separator();
                ui.add_space(4.0);

                let active_ws = state.active_workspace;
                let ws_count = state.workspaces.len();

                for i in 0..ws_count {
                    let is_active = i == active_ws;
                    let name = state.workspaces[i].name.clone();
                    let ws_id = state.workspaces[i].id;
                    let ws_unread = state.notifications.unread_count_for_workspace(ws_id);

                    // Build label with optional unread badge
                    let display = if ws_unread > 0 {
                        format!("  {} [{}]", name, ws_unread)
                    } else if is_active {
                        format!("  {}", name)
                    } else {
                        format!("  {}", name)
                    };

                    let label = if is_active {
                        egui::RichText::new(display).strong()
                    } else if ws_unread > 0 {
                        egui::RichText::new(display).color(NOTIFICATION_COLOR)
                    } else {
                        egui::RichText::new(display)
                    };

                    if ui.selectable_label(is_active, label).clicked() {
                        state.switch_workspace(i);
                    }
                }

                ui.add_space(8.0);
                if ui.button("  + New Workspace").clicked() {
                    let _ = state.add_workspace();
                }

                ui.add_space(16.0);
                ui.separator();
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new("Shortcuts")
                        .small()
                        .color(egui::Color32::GRAY),
                );
                ui.add_space(2.0);

                let shortcuts = [
                    ("Ctrl+Shift+N", "New Workspace"),
                    ("Ctrl+Shift+T", "New Tab"),
                    ("Ctrl+Tab", "Next Tab"),
                    ("Ctrl+Shift+Tab", "Prev Tab"),
                    ("Alt+1~9", "Switch WS"),
                    ("Ctrl+Shift+E", "Pane Split V"),
                    ("Ctrl+Shift+O", "Pane Split H"),
                    ("Ctrl+D", "Surface Split V"),
                    ("Ctrl+Shift+D", "Surface Split H"),
                    ("Alt+Arrow", "Focus Pane"),
                    ("Ctrl+I", "Notifications"),
                ];

                for (key, desc) in &shortcuts {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(*key)
                                .small()
                                .color(egui::Color32::from_rgb(120, 180, 255)),
                        );
                        ui.label(egui::RichText::new(*desc).small());
                    });
                }
            });
        });

    // No global top panel - tab bars are now per-pane and rendered in gpu.rs

    // Compute remaining terminal area in physical pixels
    let screen_rect = ctx.screen_rect();
    let terminal_x = SIDEBAR_WIDTH * scale_factor;
    let terminal_y = 0.0; // No global tab bar anymore
    let terminal_width = (screen_rect.width() - SIDEBAR_WIDTH) * scale_factor;
    let terminal_height = screen_rect.height() * scale_factor;

    Rect {
        x: terminal_x,
        y: terminal_y,
        width: terminal_width.max(1.0),
        height: terminal_height.max(1.0),
    }
}

/// Draw per-pane tab bars using egui Areas positioned at each pane's top.
/// This is called during the egui frame (from gpu.rs render).
pub fn draw_pane_tab_bars(
    ctx: &egui::Context,
    state: &mut AppState,
    pane_rects: &[(u32, Rect)],
    scale_factor: f32,
) {
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
            let pane = match ws.pane_layout.find_pane(pane_id) {
                Some(p) => p,
                None => continue,
            };
            if pane.tabs.len() <= 1 {
                continue;
            }
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

    for info in &infos {
        egui::Area::new(egui::Id::new(format!("pane_tabs_{}", info.pane_id)))
            .fixed_pos(egui::pos2(info.logical_x, info.logical_y))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                ui.set_min_width(info.logical_w);
                ui.set_max_width(info.logical_w);

                let bg = if info.is_focused {
                    egui::Color32::from_rgb(40, 40, 48)
                } else {
                    egui::Color32::from_rgb(30, 30, 36)
                };

                egui::Frame::new()
                    .fill(bg)
                    .inner_margin(egui::Margin::symmetric(4, 2))
                    .show(ui, |ui| {
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
    }

    // Third pass: apply actions.
    for (pane_id, action) in actions {
        match action {
            PaneTabAction::SwitchTab(idx) => {
                if let Some(pane) = state.active_workspace_mut().pane_layout.find_pane_mut(pane_id) {
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

/// Draw the notification panel overlay (toggled with Ctrl+I).
pub fn draw_notification_panel(ctx: &egui::Context, state: &mut AppState) {
    if !state.notification_panel_open {
        return;
    }

    let mut open = state.notification_panel_open;

    egui::Window::new("Notifications")
        .open(&mut open)
        .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-8.0, 8.0))
        .default_width(350.0)
        .default_height(400.0)
        .resizable(true)
        .collapsible(false)
        .show(ctx, |ui| {
            // Header with mark-all-read button
            ui.horizontal(|ui| {
                let unread = state.notifications.unread_count();
                ui.label(
                    egui::RichText::new(format!("{} unread", unread))
                        .small()
                        .color(egui::Color32::GRAY),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Mark all read").clicked() {
                        state.notifications.mark_all_read();
                    }
                });
            });
            ui.separator();

            // Scrollable notification list (newest first)
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let notifications = state.notifications.all();
                    if notifications.is_empty() {
                        ui.centered_and_justified(|ui| {
                            ui.label(
                                egui::RichText::new("No notifications")
                                    .color(egui::Color32::GRAY),
                            );
                        });
                        return;
                    }

                    // Collect notification info for display (iterate in reverse for newest first)
                    let now = Instant::now();
                    let entries: Vec<_> = notifications
                        .iter()
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
                                .workspaces
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
                            egui::Color32::from_rgba_premultiplied(80, 140, 255, 20)
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
                                                .color(NOTIFICATION_COLOR)
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
                                            .color(egui::Color32::from_rgb(100, 160, 220)),
                                    );

                                    if ui
                                        .small_button("Jump")
                                        .on_hover_text("Switch to this workspace")
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
                        state.notifications.mark_read(id);
                    }
                    if let Some(ws_id) = jump_to_ws {
                        if let Some(idx) = state.workspaces.iter().position(|ws| ws.id == ws_id) {
                            state.switch_workspace(idx);
                        }
                    }
                });
        });

    state.notification_panel_open = open;
}

enum PaneTabAction {
    SwitchTab(usize),
    AddTab,
}
