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
    engine: &crate::core::CoreState,
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
                let ws_count = engine.workspaces.len();
                for i in 0..ws_count {
                    let is_active = i == active_ws;
                    let ws_surface_ids = engine.workspaces[i].all_surface_ids();
                    let ws_has_highlight = engine
                        .notifications
                        .has_highlighted_surface(&ws_surface_ids);
                    let ws_busy_count = engine.busy_count(&ws_surface_ids);
                    // 작업 J-2: 점유(attach)된 workspace = 빨강 인디케이터.
                    let ws_attached =
                        engine.attach.workspace_holder(engine.workspaces[i].id).is_some();
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
                        ui.painter().rect_filled(
                            rect,
                            4.0,
                            th.hover_overlay.to_egui_premultiplied(),
                        );
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
                    if ws_attached {
                        // 점유 인디케이터(빨강) — 우하단(busy 녹점과 구별).
                        let dot_radius = 3.0;
                        let dot_pad = 4.0;
                        let dot_center = egui::pos2(
                            rect.max.x - dot_pad - dot_radius,
                            rect.max.y - dot_pad - dot_radius,
                        );
                        ui.painter().circle_filled(dot_center, dot_radius, th.red);
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
                    ui.painter()
                        .rect_filled(rect, 4.0, th.hover_overlay.to_egui_premultiplied());
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
