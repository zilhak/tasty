use crate::i18n::t;
use crate::state::AppState;
use crate::theme;

/// Draw the collapsed sidebar (workspace numbers + tools/expand/settings buttons).
/// Returns `CollapsedSidebarResult`.
/// `tools_rect` is Some(rect) when the tools button was clicked (rect = button position).
pub struct CollapsedSidebarResult {
    pub expand_clicked: bool,
    pub plugins_clicked: bool,
    pub settings_clicked: bool,
    pub tools_rect: Option<egui::Rect>,
    pub switch_ws: Option<usize>,
    pub add_ws: bool,
}

pub fn draw_collapsed_sidebar(
    ctx: &egui::Context,
    state: &AppState,
    sidebar_width: f32,
) -> CollapsedSidebarResult {
    let th = theme::theme();
    let mut expand_clicked = false;
    let mut plugins_clicked = false;
    let mut settings_clicked = false;
    let mut tools_rect: Option<egui::Rect> = None;
    let mut switch_ws: Option<usize> = None;
    let mut add_ws = false;

    egui::SidePanel::left("workspace_sidebar")
        .exact_width(sidebar_width)
        .resizable(false)
        .show(ctx, |ui| {
            // 바닥 고정 섹션 — TopBottomPanel::bottom으로 anchor.
            // 픽셀 계산 없이 자기 콘텐츠 높이만큼만 점유하고 나머지는 위쪽 ui가 사용.
            egui::TopBottomPanel::bottom("workspace_sidebar_collapsed_bottom")
                .frame(egui::Frame::NONE)
                .show_separator_line(false)
                .show_inside(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.separator();
                        ui.add_space(2.0);

                        // Tools button "[T]" — opens tools menu popup
                        let (tools_btn_rect, tools_resp) =
                            ui.allocate_exact_size(egui::vec2(32.0, 22.0), egui::Sense::click());
                        if tools_resp.hovered() {
                            ui.painter().rect_filled(
                                tools_btn_rect,
                                4.0,
                                th.hover_overlay.to_egui_premultiplied(),
                            );
                        }
                        ui.painter().text(
                            tools_btn_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "T",
                            egui::FontId::proportional(12.0),
                            if tools_resp.hovered() {
                                th.subtext1.into()
                            } else {
                                th.overlay0.into()
                            },
                        );
                        let tools_resp = tools_resp.on_hover_text(t("sidebar.tools_button"));
                        if tools_resp.clicked() {
                            tools_rect = Some(tools_btn_rect);
                        }
                        ui.add_space(2.0);

                        // Expand button ">"
                        let (rect, resp) =
                            ui.allocate_exact_size(egui::vec2(32.0, 22.0), egui::Sense::click());
                        if resp.hovered() {
                            ui.painter().rect_filled(
                                rect,
                                4.0,
                                th.hover_overlay.to_egui_premultiplied(),
                            );
                        }
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            ">",
                            egui::FontId::proportional(14.0),
                            if resp.hovered() {
                                th.subtext1.into()
                            } else {
                                th.overlay0.into()
                            },
                        );
                        if resp.clicked() {
                            expand_clicked = true;
                        }

                        // Plugins icon (puzzle piece)
                        ui.add_space(2.0);
                        let (rect, resp) =
                            ui.allocate_exact_size(egui::vec2(32.0, 22.0), egui::Sense::click());
                        if resp.hovered() {
                            ui.painter().rect_filled(
                                rect,
                                4.0,
                                th.hover_overlay.to_egui_premultiplied(),
                            );
                        }
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "\u{1F9E9}", // 🧩
                            egui::FontId::proportional(14.0),
                            if resp.hovered() {
                                th.subtext1.into()
                            } else {
                                th.overlay0.into()
                            },
                        );
                        if resp.clicked() {
                            plugins_clicked = true;
                        }

                        // Settings icon (gear)
                        ui.add_space(2.0);
                        let (rect, resp) =
                            ui.allocate_exact_size(egui::vec2(32.0, 22.0), egui::Sense::click());
                        if resp.hovered() {
                            ui.painter().rect_filled(
                                rect,
                                4.0,
                                th.hover_overlay.to_egui_premultiplied(),
                            );
                        }
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "\u{2699}", // ⚙
                            egui::FontId::proportional(14.0),
                            if resp.hovered() {
                                th.subtext1.into()
                            } else {
                                th.overlay0.into()
                            },
                        );
                        if resp.clicked() {
                            settings_clicked = true;
                        }
                        ui.add_space(12.0);
                    });
                });

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
                    let ws_busy_count = state.busy_count(&ws_surface_ids);
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
                        ui.painter().rect_filled(rect, 4.0, th.hover_overlay.to_egui_premultiplied());
                    }
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        &label,
                        egui::FontId::proportional(12.0),
                        text_color.into(),
                    );
                    if ws_busy_count > 0 {
                        let dot_radius = 3.0;
                        let dot_pad = 4.0;
                        let dot_center = egui::pos2(
                            rect.max.x - dot_pad - dot_radius,
                            rect.min.y + dot_pad + dot_radius,
                        );
                        ui.painter().circle_filled(dot_center, dot_radius, th.green);
                    }
                    if resp.clicked() {
                        switch_ws = Some(i);
                    }
                }

                // "+" add workspace button
                ui.add_space(2.0);
                let (rect, resp) =
                    ui.allocate_exact_size(egui::vec2(32.0, 22.0), egui::Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(rect, 4.0, th.hover_overlay.to_egui_premultiplied());
                }
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "+",
                    egui::FontId::proportional(14.0),
                    th.overlay0.into(),
                );
                if resp.clicked() {
                    add_ws = true;
                }

            });
        });

    CollapsedSidebarResult {
        expand_clicked,
        plugins_clicked,
        settings_clicked,
        tools_rect,
        switch_ws,
        add_ws,
    }
}

/// Draw the full (expanded) sidebar with workspace cards.
pub struct FullSidebarResult {
    pub collapse_clicked: bool,
    pub plugins_clicked: bool,
    pub settings_clicked: bool,
    pub tools_rect: Option<egui::Rect>,
}

pub fn draw_full_sidebar(
    ctx: &egui::Context,
    state: &mut AppState,
    sidebar_width: f32,
) -> FullSidebarResult {
    let th = theme::theme();
    let mut sidebar_collapse = false;
    let mut sidebar_plugins = false;
    let mut sidebar_settings = false;
    let mut sidebar_tools_rect: Option<egui::Rect> = None;

    egui::SidePanel::left("workspace_sidebar")
        .exact_width(sidebar_width)
        .resizable(false)
        .show(ctx, |ui| {
            // 바닥 고정 섹션 — TopBottomPanel::bottom으로 anchor.
            // 픽셀 계산 없이 자기 콘텐츠 높이만큼만 점유하고 나머지를 ScrollArea가 사용.
            egui::TopBottomPanel::bottom("workspace_sidebar_bottom")
                .frame(egui::Frame::NONE)
                .show_separator_line(false)
                .show_inside(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.separator();
                    ui.add_space(2.0);

                    // Tools button
                    {
                        let full_width = ui.available_width();
                        let (rect, resp) = ui.allocate_exact_size(
                            egui::vec2(full_width, 28.0),
                            egui::Sense::click().union(egui::Sense::hover()),
                        );
                        if resp.hovered() {
                            ui.painter().rect_filled(
                                rect,
                                4.0,
                                th.hover_overlay.to_egui_premultiplied(),
                            );
                        }
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            t("sidebar.tools_button"),
                            egui::FontId::proportional(12.0),
                            if resp.hovered() {
                                th.subtext1.into()
                            } else {
                                th.overlay0.into()
                            },
                        );
                        if resp.clicked() {
                            sidebar_tools_rect = Some(rect);
                        }
                    }

                    ui.add_space(2.0);

                    {
                        let full_width = ui.available_width();
                        let (collapse_rect, collapse_resp) = ui.allocate_exact_size(
                            egui::vec2(full_width, 28.0),
                            egui::Sense::click().union(egui::Sense::hover()),
                        );
                        if collapse_resp.hovered() {
                            ui.painter().rect_filled(
                                collapse_rect,
                                4.0,
                                th.hover_overlay.to_egui_premultiplied(),
                            );
                        }
                        ui.painter().text(
                            collapse_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "<  Collapse",
                            egui::FontId::proportional(12.0),
                            if collapse_resp.hovered() {
                                th.subtext1.into()
                            } else {
                                th.overlay0.into()
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
                            ui.painter().rect_filled(
                                rect,
                                4.0,
                                th.hover_overlay.to_egui_premultiplied(),
                            );
                        }
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            t("button.plugins"),
                            egui::FontId::proportional(12.0),
                            text_color.into(),
                        );
                        if response.clicked() {
                            sidebar_plugins = true;
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
                            ui.painter().rect_filled(
                                rect,
                                4.0,
                                th.hover_overlay.to_egui_premultiplied(),
                            );
                        }
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            t("button.settings"),
                            egui::FontId::proportional(12.0),
                            text_color.into(),
                        );
                        if response.clicked() {
                            sidebar_settings = true;
                        }
                    }
                    ui.add_space(8.0);
                });

            egui::ScrollArea::vertical()
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
                        let ws_busy_count = state.busy_count(&ws_surface_ids);

                        let bg = if is_active {
                            th.surface0.to_egui()
                        } else {
                            egui::Color32::TRANSPARENT
                        };
                        let border = if is_active {
                            th.blue.to_egui()
                        } else {
                            th.surface0.to_egui()
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

                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ws_has_highlight {
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
                                                th.blue.into(),
                                            );
                                        }

                                        if ws_busy_count > 0 {
                                            let count_text = format!("{ws_busy_count}");
                                            ui.label(
                                                egui::RichText::new(&count_text)
                                                    .small()
                                                    .color(th.green),
                                            );
                                            let dot_radius = 3.0;
                                            let (dot_rect, _) = ui.allocate_exact_size(
                                                egui::vec2(
                                                    dot_radius * 2.0 + 2.0,
                                                    dot_radius * 2.0,
                                                ),
                                                egui::Sense::hover(),
                                            );
                                            ui.painter().circle_filled(
                                                dot_rect.center(),
                                                dot_radius,
                                                th.green,
                                            );
                                        }
                                    },
                                );
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
        });

    FullSidebarResult {
        collapse_clicked: sidebar_collapse,
        plugins_clicked: sidebar_plugins,
        settings_clicked: sidebar_settings,
        tools_rect: sidebar_tools_rect,
    }
}
