use std::time::Instant;

use crate::i18n::{t, t_fmt};
use crate::model::Rect;
use crate::settings::KeybindingSettings;
use crate::state::{AppState, WsRenameField};
use crate::theme;

/// Render the egui UI and return the remaining terminal area rect (in physical pixels).
pub fn draw_ui(ctx: &egui::Context, state: &mut AppState, scale_factor: f32) -> Rect {
    let th = theme::theme();
    let sidebar_width = state.sidebar_width;

    // ---- Left sidebar: workspaces ----
    egui::SidePanel::left("workspace_sidebar")
        .exact_width(sidebar_width)
        .resizable(false)
        .show(ctx, |ui| {
            ui.vertical(|ui| {
                ui.add_space(4.0);

                let active_ws = state.active_workspace;
                let ws_count = state.engine.workspaces.len();

                for i in 0..ws_count {
                    let is_active = i == active_ws;
                    let name = state.engine.workspaces[i].name.clone();
                    let subtitle = state.engine.workspaces[i].subtitle.clone();
                    let description = state.engine.workspaces[i].description.clone();
                    let ws_id = state.engine.workspaces[i].id;
                    let ws_unread = state.engine.notifications.unread_count_for_workspace(ws_id);

                    let bg = if is_active {
                        th.surface0
                    } else {
                        egui::Color32::TRANSPARENT
                    };
                    let border = if is_active {
                        th.blue
                    } else {
                        th.surface0
                    };

                    let frame = egui::Frame::new()
                        .fill(bg)
                        .stroke(egui::Stroke::new(1.0, border))
                        .corner_radius(4.0)
                        .inner_margin(egui::Margin::symmetric(8, 6));

                    let response = frame.show(ui, |ui| {
                        ui.set_min_width(ui.available_width());

                        // Title row with optional unread badge
                        ui.horizontal(|ui| {
                            let title_text = if is_active {
                                egui::RichText::new(&name).strong()
                            } else {
                                egui::RichText::new(&name)
                            };
                            ui.label(title_text);

                            if ws_unread > 0 {
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    let badge_text = if ws_unread > 99 {
                                        "99+".to_string()
                                    } else {
                                        ws_unread.to_string()
                                    };
                                    ui.label(
                                        egui::RichText::new(badge_text)
                                            .small()
                                            .strong()
                                            .color(egui::Color32::WHITE)
                                            .background_color(th.blue),
                                    );
                                });
                            }
                        });

                        // Subtitle
                        if !subtitle.is_empty() {
                            ui.label(
                                egui::RichText::new(&subtitle)
                                    .small()
                                    .color(th.subtext0),
                            );
                        }

                        // Description
                        if !description.is_empty() {
                            ui.label(
                                egui::RichText::new(&description)
                                    .small()
                                    .color(th.overlay0),
                            );
                        }
                    });

                    let card_response = response.response.interact(egui::Sense::click());

                    // Left click: select workspace
                    if card_response.clicked() {
                        state.switch_workspace(i);
                    }

                    // Right click: context menu
                    card_response.context_menu(|ui| {
                        if ui.button(t("context_menu.rename_title")).clicked() {
                            state.ws_rename = Some((i, WsRenameField::Name, state.engine.workspaces[i].name.clone()));
                            ui.close_menu();
                        }
                        if ui.button(t("context_menu.rename_subtitle")).clicked() {
                            state.ws_rename = Some((i, WsRenameField::Subtitle, state.engine.workspaces[i].subtitle.clone()));
                            ui.close_menu();
                        }
                    });

                    ui.add_space(2.0);
                }

                ui.add_space(4.0);
                let full_width = ui.available_width();
                if ui.add_sized([full_width, 28.0], egui::Button::new(t("button.new_workspace"))).clicked() {
                    let _ = state.add_workspace();
                }

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(t("sidebar.shortcuts_heading"))
                        .small()
                        .color(egui::Color32::GRAY),
                );
                ui.add_space(2.0);

                let kb = &state.engine.settings.keybindings;
                // Configurable bindings: show from settings
                let configurable_shortcuts: Vec<(&str, &str)> = vec![
                    (&kb.new_workspace, "shortcut.desc.new_workspace"),
                    (&kb.new_tab, "shortcut.desc.new_tab"),
                    (&kb.split_pane_vertical, "shortcut.desc.pane_split_vertical"),
                    (&kb.split_pane_horizontal, "shortcut.desc.pane_split_horizontal"),
                    (&kb.split_surface_vertical, "shortcut.desc.surface_split_vertical"),
                    (&kb.split_surface_horizontal, "shortcut.desc.surface_split_horizontal"),
                    (&kb.toggle_settings, "shortcut.desc.settings"),
                    (&kb.toggle_notifications, "shortcut.desc.notifications"),
                    (&kb.close_pane, "shortcut.desc.close_pane"),
                    (&kb.focus_pane_next, "shortcut.desc.focus_pane_next"),
                    (&kb.focus_pane_prev, "shortcut.desc.focus_pane_prev"),
                    (&kb.focus_surface_next, "shortcut.desc.focus_surface_next"),
                    (&kb.focus_surface_prev, "shortcut.desc.focus_surface_prev"),
                    (&kb.close_surface, "shortcut.desc.close_surface"),
                ];

                // Dynamic modifier-based shortcuts
                let tab_mod = if kb.tab_switch_modifier == "alt" { "Alt" } else { "Ctrl" };
                let ws_mod = if kb.workspace_switch_modifier == "alt" { "Alt" } else { "Ctrl" };
                let switch_tab_display = format!("{}+1~0", tab_mod);
                let switch_ws_display = format!("{}+1~9", ws_mod);

                // Fixed shortcuts
                let fixed_shortcuts: Vec<(String, &str)> = vec![
                    ("Ctrl+Tab".to_string(), "shortcut.desc.next_tab"),
                    ("Ctrl+Shift+Tab".to_string(), "shortcut.desc.prev_tab"),
                    (switch_tab_display, "shortcut.desc.switch_tab"),
                    (switch_ws_display, "shortcut.desc.switch_workspace"),
                ];

                for (binding, desc_key) in &configurable_shortcuts {
                    if binding.is_empty() {
                        continue;
                    }
                    let key_str = KeybindingSettings::format_display(binding);
                    let desc_str = t(desc_key).to_string();
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(&key_str)
                                .small()
                                .color(th.blue),
                        );
                        ui.label(egui::RichText::new(&desc_str).small());
                    });
                }
                for (key_str, desc_key) in &fixed_shortcuts {
                    let desc_str = t(desc_key).to_string();
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(key_str)
                                .small()
                                .color(th.blue),
                        );
                        ui.label(egui::RichText::new(&desc_str).small());
                    });
                }

                // Settings button pinned to bottom using with_layout
                let available = ui.available_height();
                if available > 50.0 {
                    ui.add_space(available - 50.0);
                }
                ui.separator();
                ui.add_space(4.0);
                let full_width = ui.available_width();
                let (rect, response) = ui.allocate_exact_size(
                    egui::vec2(full_width, 28.0),
                    egui::Sense::click().union(egui::Sense::hover()),
                );
                // Paint our own hover effect — suppress egui's default by not using Button widget
                let text_color = if response.hovered() {
                    th.subtext1
                } else {
                    th.overlay0
                };
                if response.hovered() {
                    ui.painter().rect_filled(
                        rect,
                        4.0,
                        th.hover_overlay,
                    );
                }
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    t("button.settings"),
                    egui::FontId::proportional(12.0),
                    text_color,
                );
                if response.clicked() {
                    state.settings_open = true;
                }
                ui.add_space(8.0);
            });
        });

    // Compute remaining terminal area in physical pixels
    let screen_rect = ctx.screen_rect();
    let terminal_x = sidebar_width * scale_factor;
    let terminal_y = 0.0;
    let terminal_width = (screen_rect.width() - sidebar_width) * scale_factor;
    let terminal_height = screen_rect.height() * scale_factor;

    Rect {
        x: terminal_x,
        y: terminal_y,
        width: terminal_width.max(1.0),
        height: terminal_height.max(1.0),
    }
}

/// Draw the workspace rename dialog (if active).
pub fn draw_ws_rename_dialog(ctx: &egui::Context, state: &mut AppState) {
    let Some((ws_idx, field, ref mut buffer)) = state.ws_rename else {
        return;
    };

    if ws_idx >= state.engine.workspaces.len() {
        state.ws_rename = None;
        return;
    }

    let heading = match field {
        WsRenameField::Name => t("rename_dialog.title_heading"),
        WsRenameField::Subtitle => t("rename_dialog.subtitle_heading"),
    };

    let mut do_apply = false;
    let mut do_cancel = false;

    egui::Window::new(heading)
        .fixed_size(egui::vec2(280.0, 60.0))
        .collapsible(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            let response = ui.text_edit_singleline(buffer);
            // Auto-focus the text field on first frame
            if !response.has_focus() {
                response.request_focus();
            }
            // Enter to confirm, Escape to cancel
            if response.lost_focus() {
                if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    do_apply = true;
                } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    do_cancel = true;
                }
            }

            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(t("button.cancel")).clicked() {
                        do_cancel = true;
                    }
                    if ui.button(t("button.save")).clicked() {
                        do_apply = true;
                    }
                });
            });
        });

    if do_apply {
        let (ws_idx, field, buffer) = state.ws_rename.take().unwrap();
        if ws_idx < state.engine.workspaces.len() {
            match field {
                WsRenameField::Name => {
                    if !buffer.is_empty() {
                        state.engine.workspaces[ws_idx].name = buffer;
                    }
                }
                WsRenameField::Subtitle => {
                    state.engine.workspaces[ws_idx].subtitle = buffer;
                }
            }
        }
    } else if do_cancel {
        state.ws_rename = None;
    }
}

/// Draw pane dividers (borders between split panes).
pub fn draw_pane_dividers(ctx: &egui::Context, dividers: &[Rect], scale_factor: f32) {
    let th = theme::theme();
    if dividers.is_empty() {
        return;
    }
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Middle,
        egui::Id::new("pane_dividers"),
    ));
    let border_color = th.surface2;
    for div in dividers {
        let rect = egui::Rect::from_min_size(
            egui::pos2(div.x / scale_factor, div.y / scale_factor),
            egui::vec2(div.width / scale_factor, div.height / scale_factor),
        );
        painter.rect_filled(rect, 0.0, border_color);
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
                    th.surface0
                } else {
                    th.mantle
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
            let tab_bar_h = if pane.tabs.len() > 1 { 24.0 } else { 0.0 };
            infos.push(NonTerminalInfo {
                pane_id,
                logical_x: pane_rect.x / scale_factor,
                logical_y: (pane_rect.y + tab_bar_h) / scale_factor,
                logical_w: pane_rect.width / scale_factor,
                logical_h: (pane_rect.height - tab_bar_h).max(1.0) / scale_factor,
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

/// Render the pane right-click context menu.
pub fn draw_pane_context_menu(
    ctx: &egui::Context,
    state: &mut AppState,
    _scale_factor: f32,
) {
    let th = theme::theme();
    let menu = match &state.pane_context_menu {
        Some(m) => m.clone(),
        None => return,
    };

    let mut close_menu = false;
    let mut open_markdown_dialog = false;
    let mut open_explorer = false;

    egui::Area::new(egui::Id::new("pane_context_menu"))
        .fixed_pos(egui::pos2(menu.x, menu.y))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(th.surface0)
                .stroke(egui::Stroke::new(1.0, th.surface1))
                .corner_radius(4.0)
                .inner_margin(egui::Margin::same(4))
                .show(ui, |ui| {
                    ui.set_min_width(160.0);
                    if ui.button("Open Markdown...").clicked() {
                        open_markdown_dialog = true;
                        close_menu = true;
                    }
                    if ui.button("Open Explorer").clicked() {
                        open_explorer = true;
                        close_menu = true;
                    }
                    ui.separator();
                    if ui.button("Cancel").clicked() {
                        close_menu = true;
                    }
                });
        });

    // Close menu if clicked outside (skip the frame the menu was opened on)
    let current_frame = ctx.cumulative_pass_nr();
    if current_frame > menu.open_frame
        && ctx.input(|i| i.pointer.any_click())
        && !open_markdown_dialog
        && !open_explorer
    {
        close_menu = true;
    }

    if open_markdown_dialog {
        state.markdown_path_dialog = Some((menu.pane_id, String::new()));
        state.pane_context_menu = None;
    } else if open_explorer {
        // Open explorer at home directory as default
        let home = directories::BaseDirs::new()
            .map(|d| d.home_dir().to_string_lossy().to_string())
            .unwrap_or_else(|| ".".to_string());
        // Focus the target pane
        state.active_workspace_mut().focused_pane = menu.pane_id;
        let _ = state.add_explorer_tab(home);
        state.pane_context_menu = None;
    } else if close_menu {
        state.pane_context_menu = None;
    }
}

/// Render the markdown file path dialog.
pub fn draw_markdown_path_dialog(
    ctx: &egui::Context,
    state: &mut AppState,
) {
    let (pane_id, mut path_buf) = match state.markdown_path_dialog.take() {
        Some(d) => d,
        None => return,
    };

    let mut keep_open = true;
    let mut confirm = false;

    egui::Window::new("Open Markdown File")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.label("Enter file path:");
            let response = ui.text_edit_singleline(&mut path_buf);
            if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                confirm = true;
            }
            // Request focus on first frame
            response.request_focus();
            ui.horizontal(|ui| {
                if ui.button("OK").clicked() {
                    confirm = true;
                }
                if ui.button("Cancel").clicked() {
                    keep_open = false;
                }
            });
        });

    if confirm && !path_buf.is_empty() {
        state.active_workspace_mut().focused_pane = pane_id;
        let _ = state.add_markdown_tab(path_buf);
        // Don't keep dialog open
    } else if keep_open && !confirm {
        state.markdown_path_dialog = Some((pane_id, path_buf));
    }
}

enum PaneTabAction {
    SwitchTab(usize),
    AddTab,
}
