use crate::i18n::t;
use crate::state::{AppState, WsRenameField};
use crate::theme;

/// Draw the collapsed sidebar (workspace numbers + expand/settings buttons).
/// Returns (expand_clicked, settings_clicked, switch_ws, add_ws).
pub fn draw_collapsed_sidebar(
    ctx: &egui::Context,
    state: &AppState,
    sidebar_width: f32,
) -> (bool, bool, Option<usize>, bool) {
    let th = theme::theme();
    let mut expand_clicked = false;
    let mut settings_clicked = false;
    let mut switch_ws: Option<usize> = None;
    let mut add_ws = false;

    egui::SidePanel::left("workspace_sidebar")
        .exact_width(sidebar_width)
        .resizable(false)
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(4.0);

                let active_ws = state.active_workspace;
                let ws_count = state.engine.workspaces.len();
                for i in 0..ws_count {
                    let is_active = i == active_ws;
                    let ws_surface_ids = state.engine.workspaces[i].all_surface_ids();
                    let ws_has_highlight = state.engine.notifications.has_highlighted_surface(&ws_surface_ids);
                    let label = format!("{}", i + 1);
                    let bg = if is_active { th.surface0 } else { th.mantle };
                    let text_color = if is_active { th.text } else { th.subtext0 };

                    let (rect, resp) = ui.allocate_exact_size(
                        egui::vec2(32.0, 28.0),
                        egui::Sense::click(),
                    );
                    ui.painter().rect_filled(rect, 4.0, bg);
                    if resp.hovered() {
                        ui.painter().rect_filled(rect, 4.0, th.hover_overlay);
                    }
                    if ws_has_highlight {
                        ui.painter().rect_stroke(rect, 4.0, egui::Stroke::new(1.0, th.blue), egui::StrokeKind::Outside);
                    }
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        &label,
                        egui::FontId::proportional(12.0),
                        text_color,
                    );
                    if resp.clicked() {
                        switch_ws = Some(i);
                    }
                }

                // "+" add workspace button
                ui.add_space(2.0);
                let (rect, resp) = ui.allocate_exact_size(
                    egui::vec2(32.0, 22.0),
                    egui::Sense::click(),
                );
                if resp.hovered() {
                    ui.painter().rect_filled(rect, 4.0, th.hover_overlay);
                }
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "+",
                    egui::FontId::proportional(14.0),
                    th.overlay0,
                );
                if resp.clicked() {
                    add_ws = true;
                }

                // Bottom: expand + settings
                let available = ui.available_height();
                if available > 80.0 {
                    ui.add_space(available - 80.0);
                }
                ui.separator();
                ui.add_space(2.0);

                // Expand button ">"
                let (rect, resp) = ui.allocate_exact_size(
                    egui::vec2(32.0, 22.0),
                    egui::Sense::click(),
                );
                if resp.hovered() {
                    ui.painter().rect_filled(rect, 4.0, th.hover_overlay);
                }
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    ">",
                    egui::FontId::proportional(14.0),
                    if resp.hovered() { th.subtext1 } else { th.overlay0 },
                );
                if resp.clicked() {
                    expand_clicked = true;
                }

                // Settings icon (gear)
                ui.add_space(2.0);
                let (rect, resp) = ui.allocate_exact_size(
                    egui::vec2(32.0, 22.0),
                    egui::Sense::click(),
                );
                if resp.hovered() {
                    ui.painter().rect_filled(rect, 4.0, th.hover_overlay);
                }
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "\u{2699}",  // ⚙
                    egui::FontId::proportional(14.0),
                    if resp.hovered() { th.subtext1 } else { th.overlay0 },
                );
                if resp.clicked() {
                    settings_clicked = true;
                }
                ui.add_space(12.0);
            });
        });

    (expand_clicked, settings_clicked, switch_ws, add_ws)
}

/// Draw the full (expanded) sidebar with workspace cards.
/// Returns (collapse_clicked, settings_clicked).
pub fn draw_full_sidebar(
    ctx: &egui::Context,
    state: &mut AppState,
    sidebar_width: f32,
) -> (bool, bool) {
    let th = theme::theme();
    let mut sidebar_collapse = false;
    let mut sidebar_settings = false;

    egui::SidePanel::left("workspace_sidebar")
        .exact_width(sidebar_width)
        .resizable(false)
        .show(ctx, |ui| {
            let bottom_height = 80.0;
            let scroll_height = (ui.available_height() - bottom_height).max(50.0);

            egui::ScrollArea::vertical()
                .max_height(scroll_height)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                ui.add_space(4.0);

                let active_ws = state.active_workspace;
                let ws_count = state.engine.workspaces.len();

                for i in 0..ws_count {
                    let is_active = i == active_ws;
                    let name = state.engine.workspaces[i].name.clone();
                    let subtitle = state.engine.workspaces[i].subtitle.clone();
                    let description = state.engine.workspaces[i].description.clone();
                    let ws_surface_ids = state.engine.workspaces[i].all_surface_ids();
                    let ws_has_highlight = state.engine.notifications.has_highlighted_surface(&ws_surface_ids);

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

                        ui.horizontal(|ui| {
                            let title_text = if is_active {
                                egui::RichText::new(&name).strong()
                            } else {
                                egui::RichText::new(&name)
                            };
                            ui.label(title_text);

                            if ws_has_highlight {
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    let badge_size = egui::vec2(18.0, 16.0);
                                    let (rect, _) = ui.allocate_exact_size(badge_size, egui::Sense::hover());
                                    ui.painter().rect_stroke(rect, 3.0, egui::Stroke::new(1.0, th.blue), egui::StrokeKind::Inside);
                                    ui.painter().text(
                                        rect.center(),
                                        egui::Align2::CENTER_CENTER,
                                        "!",
                                        egui::FontId::proportional(10.0),
                                        th.blue,
                                    );
                                });
                            }
                        });

                        if !subtitle.is_empty() {
                            ui.label(
                                egui::RichText::new(&subtitle)
                                    .small()
                                    .color(th.subtext0),
                            );
                        }

                        if !description.is_empty() {
                            ui.label(
                                egui::RichText::new(&description)
                                    .small()
                                    .color(th.overlay0),
                            );
                        }
                    });

                    let card_response = response.response.interact(egui::Sense::click());

                    if card_response.clicked() {
                        state.switch_workspace(i);
                    }

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
                ui.add_space(4.0);
            });

            // === Fixed bottom: Collapse + Settings ===
            ui.separator();
            ui.add_space(2.0);

            {
                let full_width = ui.available_width();
                let (collapse_rect, collapse_resp) = ui.allocate_exact_size(
                    egui::vec2(full_width, 22.0),
                    egui::Sense::click().union(egui::Sense::hover()),
                );
                if collapse_resp.hovered() {
                    ui.painter().rect_filled(collapse_rect, 4.0, th.hover_overlay);
                }
                ui.painter().text(
                    collapse_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "<  Collapse",
                    egui::FontId::proportional(11.0),
                    if collapse_resp.hovered() { th.subtext1 } else { th.overlay0 },
                );
                if collapse_resp.clicked() {
                    sidebar_collapse = true;
                }
            }

            ui.add_space(2.0);
            {
                let full_width = ui.available_width();
                let (rect, response) = ui.allocate_exact_size(
                    egui::vec2(full_width, 28.0),
                    egui::Sense::click().union(egui::Sense::hover()),
                );
                let text_color = if response.hovered() { th.subtext1 } else { th.overlay0 };
                if response.hovered() {
                    ui.painter().rect_filled(rect, 4.0, th.hover_overlay);
                }
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    t("button.settings"),
                    egui::FontId::proportional(12.0),
                    text_color,
                );
                if response.clicked() {
                    sidebar_settings = true;
                }
            }
            ui.add_space(8.0);
        });

    (sidebar_collapse, sidebar_settings)
}
