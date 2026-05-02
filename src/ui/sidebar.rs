use crate::i18n::t;
use crate::state::AppState;
use crate::theme;

/// Draw the collapsed sidebar (workspace numbers + tools/expand/settings buttons).
/// Returns (expand_clicked, settings_clicked, tools_clicked, switch_ws, add_ws).
pub fn draw_collapsed_sidebar(
    ctx: &egui::Context,
    state: &AppState,
    sidebar_width: f32,
) -> (bool, bool, bool, Option<usize>, bool) {
    let th = theme::theme();
    let mut expand_clicked = false;
    let mut settings_clicked = false;
    let mut tools_clicked = false;
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
                    let ws_has_highlight = state
                        .engine
                        .notifications
                        .has_highlighted_surface(&ws_surface_ids);
                    let label = format!("{}", i + 1);
                    let bg = if is_active { th.surface0 } else { th.mantle };
                    let text_color = if is_active {
                        th.text
                    } else if ws_has_highlight {
                        th.yellow
                    } else {
                        th.subtext0
                    };

                    let (rect, resp) =
                        ui.allocate_exact_size(egui::vec2(32.0, 28.0), egui::Sense::click());
                    ui.painter().rect_filled(rect, 4.0, bg);
                    if is_active {
                        ui.painter().rect_stroke(
                            rect,
                            4.0,
                            egui::Stroke::new(1.0, th.blue),
                            egui::StrokeKind::Inside,
                        );
                    }
                    if resp.hovered() {
                        ui.painter().rect_filled(rect, 4.0, th.hover_overlay);
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
                let (rect, resp) =
                    ui.allocate_exact_size(egui::vec2(32.0, 22.0), egui::Sense::click());
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

                // Bottom: tools + expand + settings
                let available = ui.available_height();
                if available > 104.0 {
                    ui.add_space(available - 104.0);
                }
                ui.separator();
                ui.add_space(2.0);

                // Tools button "[T]" — opens clipboard viewer popup
                let (tools_rect, tools_resp) =
                    ui.allocate_exact_size(egui::vec2(32.0, 22.0), egui::Sense::click());
                if tools_resp.hovered() {
                    ui.painter().rect_filled(tools_rect, 4.0, th.hover_overlay);
                }
                ui.painter().text(
                    tools_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "T",
                    egui::FontId::proportional(12.0),
                    if tools_resp.hovered() {
                        th.subtext1
                    } else {
                        th.overlay0
                    },
                );
                let tools_resp = tools_resp.on_hover_text(t("sidebar.tools_button"));
                if tools_resp.clicked() {
                    tools_clicked = true;
                }
                ui.add_space(2.0);

                // Expand button ">"
                let (rect, resp) =
                    ui.allocate_exact_size(egui::vec2(32.0, 22.0), egui::Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(rect, 4.0, th.hover_overlay);
                }
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    ">",
                    egui::FontId::proportional(14.0),
                    if resp.hovered() {
                        th.subtext1
                    } else {
                        th.overlay0
                    },
                );
                if resp.clicked() {
                    expand_clicked = true;
                }

                // Settings icon (gear)
                ui.add_space(2.0);
                let (rect, resp) =
                    ui.allocate_exact_size(egui::vec2(32.0, 22.0), egui::Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(rect, 4.0, th.hover_overlay);
                }
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "\u{2699}", // ⚙
                    egui::FontId::proportional(14.0),
                    if resp.hovered() {
                        th.subtext1
                    } else {
                        th.overlay0
                    },
                );
                if resp.clicked() {
                    settings_clicked = true;
                }
                ui.add_space(12.0);
            });
        });

    (
        expand_clicked,
        settings_clicked,
        tools_clicked,
        switch_ws,
        add_ws,
    )
}

/// Draw the full (expanded) sidebar with workspace cards.
/// Returns (collapse_clicked, settings_clicked, tools_clicked).
pub fn draw_full_sidebar(
    ctx: &egui::Context,
    state: &mut AppState,
    sidebar_width: f32,
) -> (bool, bool, bool) {
    let th = theme::theme();
    let mut sidebar_collapse = false;
    let mut sidebar_settings = false;
    let mut sidebar_tools = false;

    egui::SidePanel::left("workspace_sidebar")
        .exact_width(sidebar_width)
        .resizable(false)
        .show(ctx, |ui| {
            let bottom_height = 108.0;
            let scroll_height = (ui.available_height() - bottom_height).max(50.0);

            egui::ScrollArea::vertical()
                .max_height(scroll_height)
                .auto_shrink([false, false])
                .drag_to_scroll(false)
                .show(ui, |ui| {
                    ui.add_space(4.0);

                    let active_ws = state.active_workspace;
                    let ws_count = state.engine.workspaces.len();
                    let mut ws_card_rects: Vec<(usize, egui::Rect)> = Vec::new();

                    for i in 0..ws_count {
                        let is_active = i == active_ws;
                        let name = state.engine.workspaces[i].name.clone();
                        let subtitle = state.engine.workspaces[i].subtitle.clone();
                        let description = state.engine.workspaces[i].description.clone();
                        let ws_surface_ids = state.engine.workspaces[i].all_surface_ids();
                        let ws_has_highlight = state
                            .engine
                            .notifications
                            .has_highlighted_surface(&ws_surface_ids);

                        let bg = if is_active {
                            th.surface0
                        } else {
                            egui::Color32::TRANSPARENT
                        };
                        let border = if is_active { th.blue } else { th.surface0 };

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
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            let badge_size = egui::vec2(18.0, 16.0);
                                            let (rect, _) = ui.allocate_exact_size(
                                                badge_size,
                                                egui::Sense::hover(),
                                            );
                                            ui.painter().rect_stroke(
                                                rect,
                                                3.0,
                                                egui::Stroke::new(1.0, th.blue),
                                                egui::StrokeKind::Inside,
                                            );
                                            ui.painter().text(
                                                rect.center(),
                                                egui::Align2::CENTER_CENTER,
                                                "!",
                                                egui::FontId::proportional(10.0),
                                                th.blue,
                                            );
                                        },
                                    );
                                }
                            });

                            if !subtitle.is_empty() {
                                ui.label(egui::RichText::new(&subtitle).small().color(th.subtext0));
                            }

                            if !description.is_empty() {
                                ui.label(
                                    egui::RichText::new(&description).small().color(th.overlay0),
                                );
                            }
                        });

                        let card_response =
                            response.response.interact(egui::Sense::click_and_drag());

                        if card_response.clicked() {
                            state.switch_workspace(i);
                        }

                        if card_response.secondary_clicked() {
                            let pos = card_response.interact_pointer_pos().unwrap_or_default();
                            state.dialogs.pending_native_menu =
                                Some(crate::state::PendingNativeMenu::Workspace {
                                    ws_idx: i,
                                    x: pos.x,
                                    y: pos.y,
                                });
                        }

                        // Drag-and-drop for workspace reordering
                        if card_response.drag_started_by(egui::PointerButton::Primary) {
                            state.dialogs.ws_drag = Some(crate::state::WsDragState {
                                ws_idx: i,
                                current_y: card_response
                                    .interact_pointer_pos()
                                    .map(|p| p.y)
                                    .unwrap_or(0.0),
                            });
                        }
                        if card_response.dragged_by(egui::PointerButton::Primary) {
                            if let Some(ref mut drag) = state.dialogs.ws_drag {
                                if drag.ws_idx == i {
                                    if let Some(pos) = card_response.interact_pointer_pos() {
                                        drag.current_y = pos.y;
                                    }
                                }
                            }
                        }

                        // Store card rect for drop calculation
                        ws_card_rects.push((i, response.response.rect));

                        ui.add_space(2.0);
                    }

                    // Handle drag stop — check if mouse was released
                    if let Some(drag) = state.dialogs.ws_drag.clone() {
                        let released = !ui.input(|i| i.pointer.primary_down());
                        if released {
                            state.dialogs.ws_drag = None;
                            // Compute drop target from card rects
                            let target = ws_card_rects
                                .iter()
                                .position(|(_, rect)| drag.current_y < rect.center().y)
                                .unwrap_or(ws_card_rects.len().saturating_sub(1));
                            let target = target.min(ws_count.saturating_sub(1));
                            if target != drag.ws_idx {
                                state.move_workspace(drag.ws_idx, target);
                            }
                        } else {
                            // Draw insert marker
                            let insert_idx = ws_card_rects
                                .iter()
                                .position(|(_, rect)| drag.current_y < rect.center().y)
                                .unwrap_or(ws_card_rects.len());
                            if let Some(marker_rect) = if insert_idx < ws_card_rects.len() {
                                Some(ws_card_rects[insert_idx].1)
                            } else {
                                ws_card_rects.last().map(|(_, r)| *r)
                            } {
                                let marker_y = if insert_idx < ws_card_rects.len() {
                                    marker_rect.min.y - 1.0
                                } else {
                                    marker_rect.max.y + 1.0
                                };
                                let line = egui::Rect::from_min_size(
                                    egui::pos2(marker_rect.min.x, marker_y),
                                    egui::vec2(marker_rect.width(), 2.0),
                                );
                                ui.painter().rect_filled(line, 0.0, th.blue);
                            }

                            // Draw ghost card at mouse position
                            if let Some(name) =
                                state.engine.workspaces.get(drag.ws_idx).map(|w| w.name.clone())
                            {
                                if let Some((_, first_rect)) = ws_card_rects.first() {
                                    let ghost_rect = egui::Rect::from_min_size(
                                        egui::pos2(
                                            first_rect.min.x,
                                            drag.current_y - first_rect.height() / 2.0,
                                        ),
                                        first_rect.size(),
                                    );
                                    let ghost_bg = egui::Color32::from_rgba_unmultiplied(
                                        th.surface0.r(),
                                        th.surface0.g(),
                                        th.surface0.b(),
                                        180,
                                    );
                                    let ghost_fg = egui::Color32::from_rgba_unmultiplied(
                                        th.text.r(),
                                        th.text.g(),
                                        th.text.b(),
                                        180,
                                    );
                                    ui.painter().rect_filled(ghost_rect, 4.0, ghost_bg);
                                    ui.painter().text(
                                        ghost_rect.center(),
                                        egui::Align2::CENTER_CENTER,
                                        &name,
                                        egui::FontId::proportional(12.0),
                                        ghost_fg,
                                    );
                                }
                            }
                        }
                    }

                    ui.add_space(4.0);
                    let full_width = ui.available_width();
                    if ui
                        .add_sized(
                            [full_width, 28.0],
                            egui::Button::new(t("button.new_workspace")),
                        )
                        .clicked()
                    {
                        if let Err(e) = state.add_workspace() {
                            tracing::warn!("add_workspace failed: {e}");
                        }
                    }
                    ui.add_space(4.0);
                });

            // === Fixed bottom: Tools + Collapse + Settings ===
            ui.separator();
            ui.add_space(2.0);

            // Tools button
            {
                let full_width = ui.available_width();
                let (rect, resp) = ui.allocate_exact_size(
                    egui::vec2(full_width, 22.0),
                    egui::Sense::click().union(egui::Sense::hover()),
                );
                if resp.hovered() {
                    ui.painter().rect_filled(rect, 4.0, th.hover_overlay);
                }
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    t("sidebar.tools_button"),
                    egui::FontId::proportional(11.0),
                    if resp.hovered() {
                        th.subtext1
                    } else {
                        th.overlay0
                    },
                );
                if resp.clicked() {
                    sidebar_tools = true;
                }
            }

            ui.add_space(2.0);

            {
                let full_width = ui.available_width();
                let (collapse_rect, collapse_resp) = ui.allocate_exact_size(
                    egui::vec2(full_width, 22.0),
                    egui::Sense::click().union(egui::Sense::hover()),
                );
                if collapse_resp.hovered() {
                    ui.painter()
                        .rect_filled(collapse_rect, 4.0, th.hover_overlay);
                }
                ui.painter().text(
                    collapse_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "<  Collapse",
                    egui::FontId::proportional(11.0),
                    if collapse_resp.hovered() {
                        th.subtext1
                    } else {
                        th.overlay0
                    },
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
                let text_color = if response.hovered() {
                    th.subtext1
                } else {
                    th.overlay0
                };
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

    (sidebar_collapse, sidebar_settings, sidebar_tools)
}
