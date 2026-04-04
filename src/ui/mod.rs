mod context_menu;
mod dialog;
mod divider;
mod non_terminal;
mod notification;
mod tab_bar;

pub use context_menu::draw_pane_context_menu;
pub use dialog::{draw_markdown_path_dialog, draw_ws_rename_dialog};
pub use divider::{draw_pane_dividers, draw_surface_highlights};
pub use non_terminal::draw_non_terminal_panels;
pub use notification::draw_notification_panel;
pub use tab_bar::draw_pane_tab_bars;

use crate::i18n::t;
use crate::model::Rect;
use crate::state::{AppState, WsRenameField};
use crate::theme;

/// Render the egui UI and return the remaining terminal area rect (in physical pixels).
pub fn draw_ui(ctx: &egui::Context, state: &mut AppState, scale_factor: f32) -> Rect {
    let th = theme::theme();
    let sidebar_width = state.sidebar_width;

    // ---- Left sidebar ----
    if !state.sidebar_visible {
        // Sidebar hidden — skip rendering entirely
    } else if state.sidebar_collapsed {
        // Collapsed sidebar: workspace numbers + expand/settings buttons
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

                    // Workspace number buttons
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
                        // Draw highlight border if workspace has alerted surfaces
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

        // Apply actions outside the borrow
        if expand_clicked { state.sidebar_collapsed = false; }
        if settings_clicked { state.settings_open = true; }
        if let Some(i) = switch_ws { state.switch_workspace(i); }
        if add_ws { let _ = state.add_workspace(); }
    } else {
    // Full sidebar — two zones: scrollable workspace list + fixed bottom buttons
    let mut sidebar_collapse = false;
    let mut sidebar_settings = false;

    egui::SidePanel::left("workspace_sidebar")
        .exact_width(sidebar_width)
        .resizable(false)
        .show(ctx, |ui| {
            // === Scrollable workspace list area ===
            // Reserve: separator(1) + space(2) + collapse(22) + space(2) + settings(28) + space(8) = 63
            // Plus egui panel internal margins (~16px)
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
                    let _ws_id = state.engine.workspaces[i].id;
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

                        // Title row with optional alert badge
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
                                    // Draw border-only badge with "!"
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
                ui.add_space(4.0);
            }); // end ScrollArea

            // === Fixed bottom: Collapse + Settings ===
            ui.separator();
            ui.add_space(2.0);

            // Collapse button
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

            // Settings button
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
        }); // end SidePanel

        // Apply actions outside the borrow
        if sidebar_collapse { state.sidebar_collapsed = true; }
        if sidebar_settings { state.settings_open = true; }
    } // end of sidebar visible/collapsed/full

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
