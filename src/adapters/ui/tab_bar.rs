use egui::emath::GuiRounding as _;

use crate::model::PhysicalRect;
use crate::state::AppState;
use crate::theme;

/// Draw per-pane tab bars using egui Areas positioned at each pane's top.
/// This is called during the egui frame (from gpu.rs render).
pub fn draw_pane_tab_bars(
    ctx: &egui::Context,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    pane_rects: &[(u32, PhysicalRect)],
    scale_factor: f32,
) {
    let th = theme::theme();
    let focused_pane_id = state.focused_pane_id(engine);

    struct PaneTabInfo {
        pane_id: u32,
        tab_names: Vec<String>,
        tab_has_notification: Vec<bool>,
        tab_is_busy: Vec<bool>,
        active_tab: usize,
        is_focused: bool,
        logical_x: f32,
        logical_y: f32,
        logical_w: f32,
        scroll_offset: f32,
    }

    let mut infos = Vec::new();
    {
        let ws = state.active_workspace(engine);
        for &(pane_id, pane_rect) in pane_rects {
            let pane = match ws.pane_layout().find_pane(pane_id) {
                Some(p) => p,
                None => continue,
            };
            let tab_has_notification: Vec<bool> = pane
                .tabs
                .iter()
                .map(|t| {
                    let sids = t.all_surface_ids();
                    engine.notifications.has_highlighted_surface(&sids)
                })
                .collect();
            let tab_is_busy: Vec<bool> = pane
                .tabs
                .iter()
                .map(|t| {
                    let sids = t.all_surface_ids();
                    sids.iter().any(|sid| engine.busy_surfaces.contains(sid))
                })
                .collect();
            infos.push(PaneTabInfo {
                pane_id,
                tab_names: pane.tabs.iter().map(|t| t.display_name()).collect(),
                tab_has_notification,
                tab_is_busy,
                active_tab: pane.active_tab,
                is_focused: pane_id == focused_pane_id,
                logical_x: (pane_rect.x.value() / scale_factor).round_ui(),
                logical_y: (pane_rect.y.value() / scale_factor).round_ui(),
                logical_w: (pane_rect.width.value() / scale_factor).round_ui(),
                scroll_offset: pane.tab_scroll_offset,
            });
        }
    }

    let mut actions: Vec<(u32, PaneTabAction)> = Vec::new();
    let mut measured_tab_bar_height: Option<f32> = None;

    // 탭 바 독자 scale. 기존 ui_scale 과 분리 — 사용자가 settings 에서 직접
    // tab_width / tab_font_size / monitor scale 반영도 (0..=1) 조정.
    let appearance = &engine.settings.appearance;
    let adjust = appearance.tab_adjust_factor(scale_factor);
    let tab_w = appearance.tab_width * adjust;
    let label_font_size = appearance.tab_font_size * adjust;
    // bar_h / "+" / arrow / padding / busy dot / active indicator 굵기는 사용자
    // 옵션을 따로 두지 않고 theme/hardcoded 값에 adjust 만 곱 — 폰트와 함께
    // 비례 확대되도록.
    let bar_h = th.item_height_tab.value() * adjust;
    let plus_w: f32 = 28.0 * adjust;
    let arrow_w: f32 = 20.0 * adjust;
    let separator_w: f32 = 1.0;
    let h_padding: f32 = 8.0 * adjust;
    let dot_radius: f32 = 3.0 * adjust;
    let dot_pad: f32 = 6.0 * adjust;
    let active_indicator_h: f32 = 2.0 * adjust;
    let plus_font_size = th.font_size_body.value() * adjust;
    let arrow_font_size = th.font_size_caption.value() * adjust;

    for info in &infos {
        let n = info.tab_names.len();
        // Total content width: tabs + separators + separator before "+" + "+"
        let content_w =
            n as f32 * tab_w + (n.max(1) - 1) as f32 * separator_w + separator_w + plus_w;
        let needs_scroll = content_w > info.logical_w;
        // Available width for tab content (minus arrows if scrolling)
        let viewport_w = if needs_scroll {
            (info.logical_w - arrow_w * 2.0).max(0.0)
        } else {
            info.logical_w.max(0.0)
        };
        let max_scroll = (content_w - viewport_w).max(0.0);
        let scroll = info.scroll_offset.clamp(0.0, max_scroll);

        let area_response = egui::Area::new(egui::Id::new(format!("pane_tabs_{}", info.pane_id)))
            .fixed_pos(egui::pos2(info.logical_x, info.logical_y))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                let bg = if info.is_focused {
                    th.surface0
                } else {
                    th.mantle
                };

                egui::Frame::new()
                    .fill(bg.into())
                    .inner_margin(egui::Margin::ZERO)
                    .show(ui, |ui| {
                        ui.set_min_width(info.logical_w);
                        ui.set_max_width(info.logical_w);
                        ui.set_min_height(bar_h);
                        ui.set_max_height(bar_h);

                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;

                            // Left arrow
                            if needs_scroll {
                                let can_left = scroll > 0.0;
                                let (r, resp) = ui.allocate_exact_size(
                                    egui::vec2(arrow_w, bar_h),
                                    egui::Sense::click(),
                                );
                                let arrow_color = if can_left { th.subtext0 } else { th.surface1 };
                                if resp.hovered() && can_left {
                                    ui.painter().rect_filled(r, 0.0, th.surface0);
                                }
                                ui.painter().text(
                                    r.center(),
                                    egui::Align2::CENTER_CENTER,
                                    "<",
                                    egui::FontId::proportional(arrow_font_size),
                                    arrow_color.into(),
                                );
                                if resp.clicked() && can_left {
                                    actions.push((info.pane_id, PaneTabAction::ScrollLeft));
                                }
                            }

                            // Clipped tab area
                            let clip_start_x = ui.cursor().min.x;
                            let clip_rect = egui::Rect::from_min_size(
                                egui::pos2(clip_start_x, ui.cursor().min.y),
                                egui::vec2(viewport_w, bar_h),
                            );
                            // Reserve the viewport space (click sense for right-click context menu on empty area)
                            let (_, viewport_resp) = ui.allocate_exact_size(
                                egui::vec2(viewport_w, bar_h),
                                egui::Sense::click(),
                            );
                            if viewport_resp.secondary_clicked() {
                                actions.push((
                                    info.pane_id,
                                    PaneTabAction::OpenPaneContextMenu(
                                        viewport_resp.interact_pointer_pos().unwrap_or_default(),
                                    ),
                                ));
                            }

                            // Draw tabs inside the clip rect using painter with clip
                            let painter = ui.painter().with_clip_rect(clip_rect);
                            let mut x = clip_start_x - scroll;

                            for (i, name) in info.tab_names.iter().enumerate() {
                                if i > 0 {
                                    // 1px separator
                                    let sep = egui::Rect::from_min_size(
                                        egui::pos2(x, clip_rect.min.y),
                                        egui::vec2(separator_w, bar_h),
                                    );
                                    painter.rect_filled(sep, 0.0, th.surface1);
                                    x += separator_w;
                                }

                                let is_active = i == info.active_tab;
                                let has_notif =
                                    info.tab_has_notification.get(i).copied().unwrap_or(false);
                                let is_busy = info.tab_is_busy.get(i).copied().unwrap_or(false);
                                let tab_bg = if is_active { th.base } else { bg };
                                let text_color = if is_active {
                                    th.text
                                } else if has_notif {
                                    th.yellow
                                } else {
                                    th.subtext0
                                };

                                let tab_rect = egui::Rect::from_min_size(
                                    egui::pos2(x, clip_rect.min.y),
                                    egui::vec2(tab_w, bar_h),
                                );

                                painter.rect_filled(tab_rect, 0.0, tab_bg);

                                if is_active {
                                    let line_rect = egui::Rect::from_min_size(
                                        egui::pos2(tab_rect.min.x, tab_rect.min.y),
                                        egui::vec2(tab_w, active_indicator_h),
                                    );
                                    painter.rect_filled(line_rect, 0.0, th.blue);
                                }

                                if is_busy {
                                    let dot_center = egui::pos2(
                                        tab_rect.max.x - dot_pad - dot_radius,
                                        tab_rect.center().y,
                                    );
                                    let dot_color: egui::Color32 = if is_active {
                                        th.green.into()
                                    } else {
                                        // 비활성 dot: theme green + ~70% alpha.
                                        th.green.with_alpha(180).to_egui()
                                    };
                                    painter.circle_filled(dot_center, dot_radius, dot_color);
                                }

                                // Truncate tab name with ellipsis if it exceeds available width
                                let font_id = egui::FontId::proportional(label_font_size);
                                let available_w = tab_w - h_padding * 2.0;
                                let galley = painter.layout_no_wrap(
                                    name.clone(),
                                    font_id.clone(),
                                    text_color.into(),
                                );
                                if galley.size().x > available_w {
                                    // Binary-ish search: trim characters until it fits
                                    let mut truncated = name.clone();
                                    while !truncated.is_empty() {
                                        truncated.pop();
                                        let candidate = format!("{truncated}…");
                                        let g = painter.layout_no_wrap(
                                            candidate.clone(),
                                            font_id.clone(),
                                            text_color.into(),
                                        );
                                        if g.size().x <= available_w {
                                            let text_x = tab_rect.min.x + h_padding;
                                            let text_y = tab_rect.center().y - g.size().y / 2.0;
                                            painter.galley(
                                                egui::pos2(text_x, text_y),
                                                g,
                                                text_color.into(),
                                            );
                                            break;
                                        }
                                    }
                                } else {
                                    let text_pos = tab_rect.center() - galley.size() / 2.0;
                                    painter.galley(text_pos, galley, text_color.into());
                                }

                                // Click & drag detection
                                let tab_clip = tab_rect.intersect(clip_rect);
                                if !tab_clip.is_negative() {
                                    let resp = ui.interact(
                                        tab_clip,
                                        egui::Id::new(format!("tab_{}_{}", info.pane_id, i)),
                                        egui::Sense::click_and_drag(),
                                    );
                                    if resp.clicked() {
                                        actions.push((info.pane_id, PaneTabAction::SwitchTab(i)));
                                    }
                                    if resp.secondary_clicked() {
                                        actions.push((
                                            info.pane_id,
                                            PaneTabAction::OpenContextMenu(
                                                i,
                                                resp.interact_pointer_pos().unwrap_or_default(),
                                            ),
                                        ));
                                    }
                                    if resp.drag_started_by(egui::PointerButton::Primary) {
                                        actions.push((info.pane_id, PaneTabAction::DragStart(i)));
                                    }
                                    if resp.dragged_by(egui::PointerButton::Primary)
                                        && let Some(pos) = resp.interact_pointer_pos()
                                    {
                                        actions
                                            .push((info.pane_id, PaneTabAction::DragUpdate(pos.x)));
                                    }
                                    if resp.drag_stopped_by(egui::PointerButton::Primary) {
                                        actions.push((info.pane_id, PaneTabAction::DragEnd));
                                    }
                                }

                                x += tab_w;
                            }

                            // Separator before "+"
                            {
                                let sep = egui::Rect::from_min_size(
                                    egui::pos2(x, clip_rect.min.y),
                                    egui::vec2(separator_w, bar_h),
                                );
                                painter.rect_filled(sep, 0.0, th.surface1);
                                x += separator_w;
                            }

                            // "+" button
                            {
                                let plus_rect = egui::Rect::from_min_size(
                                    egui::pos2(x, clip_rect.min.y),
                                    egui::vec2(plus_w, bar_h),
                                );
                                let plus_clip = plus_rect.intersect(clip_rect);
                                if !plus_clip.is_negative() {
                                    let resp = ui.interact(
                                        plus_clip,
                                        egui::Id::new(format!("tab_plus_{}", info.pane_id)),
                                        egui::Sense::click(),
                                    );
                                    if resp.hovered() {
                                        painter.rect_filled(plus_rect, 0.0, th.surface0);
                                    }
                                    painter.text(
                                        plus_rect.center(),
                                        egui::Align2::CENTER_CENTER,
                                        "+",
                                        egui::FontId::proportional(plus_font_size),
                                        th.subtext0.into(),
                                    );
                                    if resp.clicked() {
                                        actions.push((info.pane_id, PaneTabAction::AddTab));
                                    }
                                }
                            }

                            // Right arrow
                            if needs_scroll {
                                let can_right = scroll < max_scroll;
                                let (r, resp) = ui.allocate_exact_size(
                                    egui::vec2(arrow_w, bar_h),
                                    egui::Sense::click(),
                                );
                                let arrow_color = if can_right { th.subtext0 } else { th.surface1 };
                                if resp.hovered() && can_right {
                                    ui.painter().rect_filled(r, 0.0, th.surface0);
                                }
                                ui.painter().text(
                                    r.center(),
                                    egui::Align2::CENTER_CENTER,
                                    ">",
                                    egui::FontId::proportional(arrow_font_size),
                                    arrow_color.into(),
                                );
                                if resp.clicked() && can_right {
                                    actions.push((info.pane_id, PaneTabAction::ScrollRight));
                                }
                            }
                        });
                    });
            });

        if measured_tab_bar_height.is_none() {
            let logical_h = area_response.response.rect.height();
            measured_tab_bar_height = Some(logical_h * scale_factor);
        }
    }

    if let Some(h) = measured_tab_bar_height {
        state.tab_bar_height = tasty_type_geometry::length::PhysicalPx(h);
    }

    // Apply actions
    for (pane_id, action) in actions {
        match action {
            PaneTabAction::SwitchTab(idx) => {
                let mut to_wake: Vec<u32> = Vec::new();
                if let Some(pane) = state
                    .active_workspace_mut(engine)
                    .pane_layout_mut()
                    .find_pane_mut(pane_id)
                {
                    pane.active_tab = idx;
                    if let Some(tab) = pane.tabs.get(idx) {
                        to_wake = tab.deferred_surface_ids();
                    }
                }
                for sid in to_wake {
                    engine.ensure_surface_initialized(sid);
                }
            }
            PaneTabAction::AddTab => {
                state.active_workspace_mut(engine).focused_pane = pane_id;
                if let Err(e) = state.add_tab(engine) {
                    tracing::warn!("add_tab failed: {e}");
                }
            }
            PaneTabAction::ScrollLeft => {
                if let Some(pane) = state
                    .active_workspace_mut(engine)
                    .pane_layout_mut()
                    .find_pane_mut(pane_id)
                {
                    pane.tab_scroll_offset = (pane.tab_scroll_offset - tab_w).max(0.0);
                }
            }
            PaneTabAction::ScrollRight => {
                if let Some(pane) = state
                    .active_workspace_mut(engine)
                    .pane_layout_mut()
                    .find_pane_mut(pane_id)
                {
                    pane.tab_scroll_offset += tab_w;
                }
            }
            PaneTabAction::OpenContextMenu(tab_idx, pos) => {
                state.dialogs.pending_native_menu = Some(crate::state::PendingNativeMenu::Tab {
                    pane_id,
                    tab_index: tab_idx,
                    x: pos.x,
                    y: pos.y,
                });
            }
            PaneTabAction::OpenPaneContextMenu(pos) => {
                state.dialogs.pending_native_menu = Some(crate::state::PendingNativeMenu::Pane {
                    pane_id,
                    x: pos.x,
                    y: pos.y,
                });
            }
            PaneTabAction::DragStart(tab_idx) => {
                state.dialogs.tab_drag = Some(crate::state::TabDragState {
                    pane_id,
                    tab_index: tab_idx,
                    current_x: 0.0,
                });
            }
            PaneTabAction::DragUpdate(mouse_x) => {
                if let Some(ref mut drag) = state.dialogs.tab_drag
                    && drag.pane_id == pane_id
                {
                    drag.current_x = mouse_x;
                }
            }
            PaneTabAction::DragEnd => {
                if let Some(drag) = state.dialogs.tab_drag.take()
                    && drag.pane_id == pane_id
                {
                    // Calculate insert position from mouse x
                    // Find the pane's tab info to determine target index
                    if let Some(pane_info) = infos.iter().find(|i| i.pane_id == pane_id) {
                        let target = compute_drop_index(
                            drag.current_x,
                            pane_info.logical_x,
                            pane_info.scroll_offset,
                            pane_info.tab_names.len(),
                            tab_w,
                            separator_w,
                            pane_info.logical_w,
                        );
                        if target != drag.tab_index
                            && let Some(pane) = state
                                .active_workspace_mut(engine)
                                .pane_layout_mut()
                                .find_pane_mut(pane_id)
                        {
                            pane.move_tab(drag.tab_index, target);
                        }
                    }
                }
            }
        }
    }

    // Draw drag overlay (ghost tab + insert marker)
    if let Some(ref drag) = state.dialogs.tab_drag
        && let Some(pane_info) = infos.iter().find(|i| i.pane_id == drag.pane_id)
        && let Some(pane_rect) = pane_rects.iter().find(|(pid, _)| *pid == drag.pane_id)
    {
        let pane_logical_y = (pane_rect.1.y.value() / scale_factor).round_ui();
        let needs_scroll_arrows = {
            let n = pane_info.tab_names.len();
            let content_w =
                n as f32 * tab_w + (n.max(1) - 1) as f32 * separator_w + separator_w + plus_w;
            content_w > pane_info.logical_w
        };
        let viewport_start = if needs_scroll_arrows {
            pane_info.logical_x + arrow_w
        } else {
            pane_info.logical_x
        };

        let drop_idx = compute_drop_index(
            drag.current_x,
            pane_info.logical_x,
            pane_info.scroll_offset,
            pane_info.tab_names.len(),
            tab_w,
            separator_w,
            pane_info.logical_w,
        );

        // Insert marker (blue vertical line)
        let marker_x =
            viewport_start - pane_info.scroll_offset + drop_idx as f32 * (tab_w + separator_w);
        let marker_rect = egui::Rect::from_min_size(
            egui::pos2(marker_x - 1.0, pane_logical_y),
            egui::vec2(2.0, bar_h),
        );
        let overlay_painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Tooltip,
            egui::Id::new("tab_drag_overlay"),
        ));
        overlay_painter.rect_filled(marker_rect, 0.0, th.blue);

        // Ghost tab at mouse position
        let ghost_name = pane_info
            .tab_names
            .get(drag.tab_index)
            .cloned()
            .unwrap_or_default();
        let ghost_rect = egui::Rect::from_min_size(
            egui::pos2(drag.current_x - tab_w / 2.0, pane_logical_y),
            egui::vec2(tab_w, bar_h),
        );
        // Tab drag ghost: theme 색 + ~70% alpha.
        let ghost_bg = th.base.with_alpha(180).to_egui();
        let ghost_fg = th.text.with_alpha(180).to_egui();
        overlay_painter.rect_filled(ghost_rect, 0.0, ghost_bg);
        overlay_painter.text(
            ghost_rect.center(),
            egui::Align2::CENTER_CENTER,
            &ghost_name,
            egui::FontId::proportional(label_font_size),
            ghost_fg,
        );
    }
}

/// Compute which tab index the mouse x position corresponds to for a drop.
fn compute_drop_index(
    mouse_x: f32,
    pane_logical_x: f32,
    scroll_offset: f32,
    tab_count: usize,
    tab_w: f32,
    separator_w: f32,
    _pane_w: f32,
) -> usize {
    // Convert mouse x to position within the tab content
    let content_x = mouse_x - pane_logical_x + scroll_offset;
    // Each tab occupies tab_w + separator_w (except the first which has no leading separator)
    let slot = content_x / (tab_w + separator_w);
    slot.round()
        .clamp(0.0, (tab_count.saturating_sub(1)) as f32) as usize
}

enum PaneTabAction {
    SwitchTab(usize),
    AddTab,
    ScrollLeft,
    OpenContextMenu(usize, egui::Pos2),
    OpenPaneContextMenu(egui::Pos2),
    ScrollRight,
    DragStart(usize),
    DragUpdate(f32),
    DragEnd,
}
