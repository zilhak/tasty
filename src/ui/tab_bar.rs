use egui::emath::GuiRounding as _;

use crate::model::Rect;
use crate::state::AppState;
use crate::theme;

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

    struct PaneTabInfo {
        pane_id: u32,
        tab_names: Vec<String>,
        active_tab: usize,
        is_focused: bool,
        logical_x: f32,
        logical_y: f32,
        logical_w: f32,
        scroll_offset: f32,
    }

    let mut infos = Vec::new();
    {
        let ws = state.active_workspace();
        for &(pane_id, pane_rect) in pane_rects {
            let pane = match ws.pane_layout().find_pane(pane_id) {
                Some(p) => p,
                None => continue,
            };
            infos.push(PaneTabInfo {
                pane_id,
                tab_names: pane.tabs.iter().map(|t| t.display_name()).collect(),
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

    let tab_w = th.tab_width.value();
    let bar_h = th.item_height_tab.value();
    let plus_w: f32 = 28.0;
    let arrow_w: f32 = 20.0;
    let separator_w: f32 = 1.0;

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
                    .fill(bg)
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
                                    egui::FontId::proportional(th.font_size_caption.value()),
                                    arrow_color,
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
                                let tab_bg = if is_active { th.base } else { bg };
                                let text_color = if is_active { th.text } else { th.subtext0 };

                                let tab_rect = egui::Rect::from_min_size(
                                    egui::pos2(x, clip_rect.min.y),
                                    egui::vec2(tab_w, bar_h),
                                );

                                painter.rect_filled(tab_rect, 0.0, tab_bg);

                                if is_active {
                                    let line_rect = egui::Rect::from_min_size(
                                        egui::pos2(tab_rect.min.x, tab_rect.min.y),
                                        egui::vec2(tab_w, 2.0),
                                    );
                                    painter.rect_filled(line_rect, 0.0, th.blue);
                                }

                                // Truncate tab name with ellipsis if it exceeds available width
                                let font_id =
                                    egui::FontId::proportional(th.font_size_caption.value());
                                let h_padding = 8.0;
                                let available_w = tab_w - h_padding * 2.0;
                                let galley = painter.layout_no_wrap(
                                    name.clone(),
                                    font_id.clone(),
                                    text_color,
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
                                            text_color,
                                        );
                                        if g.size().x <= available_w {
                                            let text_x = tab_rect.min.x + h_padding;
                                            let text_y = tab_rect.center().y - g.size().y / 2.0;
                                            painter.galley(
                                                egui::pos2(text_x, text_y),
                                                g,
                                                text_color,
                                            );
                                            break;
                                        }
                                    }
                                } else {
                                    let text_pos = tab_rect.center() - galley.size() / 2.0;
                                    painter.galley(text_pos, galley, text_color);
                                }

                                // Click detection
                                let tab_clip = tab_rect.intersect(clip_rect);
                                if !tab_clip.is_negative() {
                                    let resp = ui.interact(
                                        tab_clip,
                                        egui::Id::new(format!("tab_{}_{}", info.pane_id, i)),
                                        egui::Sense::click(),
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
                                        egui::FontId::proportional(th.font_size_body.value()),
                                        th.subtext0,
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
                                    egui::FontId::proportional(th.font_size_caption.value()),
                                    arrow_color,
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
        state.tab_bar_height = crate::model::length::PhysicalPx(h);
    }

    // Apply actions
    for (pane_id, action) in actions {
        match action {
            PaneTabAction::SwitchTab(idx) => {
                if let Some(pane) = state
                    .active_workspace_mut()
                    .pane_layout_mut()
                    .find_pane_mut(pane_id)
                {
                    pane.active_tab = idx;
                }
            }
            PaneTabAction::AddTab => {
                state.active_workspace_mut().focused_pane = pane_id;
                if let Err(e) = state.add_tab() {
                    tracing::warn!("add_tab failed: {e}");
                }
            }
            PaneTabAction::ScrollLeft => {
                if let Some(pane) = state
                    .active_workspace_mut()
                    .pane_layout_mut()
                    .find_pane_mut(pane_id)
                {
                    pane.tab_scroll_offset = (pane.tab_scroll_offset - tab_w).max(0.0);
                }
            }
            PaneTabAction::ScrollRight => {
                if let Some(pane) = state
                    .active_workspace_mut()
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
        }
    }

    // Tab context menu is now handled via native OS menus (see process_pending_native_menu)

    // Draw tab rename dialog if open
    draw_tab_rename_dialog(ctx, state);
}

fn draw_tab_rename_dialog(ctx: &egui::Context, state: &mut AppState) {
    let th = theme::theme();
    let dialog = match &state.dialogs.tab_rename {
        Some(d) => d.clone(),
        None => return,
    };
    let (pane_id, tab_idx, mut buf) = dialog;

    let mut close = false;

    egui::Area::new(egui::Id::new("tab_rename_dialog"))
        .fixed_pos(egui::pos2(
            ctx.screen_rect().center().x - 120.0,
            ctx.screen_rect().center().y - 40.0,
        ))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(th.surface0)
                .stroke(egui::Stroke::new(1.0, th.surface1))
                .corner_radius(th.corner_radius.value())
                .inner_margin(egui::Margin::same(12))
                .show(ui, |ui| {
                    ui.set_min_width(240.0);

                    let rename_label = crate::i18n::t("tab_context_menu.rename");
                    ui.label(egui::RichText::new(rename_label).strong().color(th.text));
                    ui.add_space(4.0);

                    let resp = ui.text_edit_singleline(&mut buf);
                    // 다이얼로그가 막 열려 포커스를 얻은 첫 프레임에 텍스트 전체를 선택해서,
                    // 사용자가 곧바로 입력하면 기존 이름이 새 입력으로 대체되도록 한다.
                    if resp.gained_focus() {
                        if let Some(mut text_state) =
                            egui::TextEdit::load_state(ctx, resp.id)
                        {
                            let len = buf.chars().count();
                            text_state.cursor.set_char_range(Some(
                                egui::text::CCursorRange::two(
                                    egui::text::CCursor::new(0),
                                    egui::text::CCursor::new(len),
                                ),
                            ));
                            text_state.store(ctx, resp.id);
                        }
                    }
                    if resp.lost_focus() {
                        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                            close = true;
                        } else {
                            // lost_focus without Escape = Enter (or clicked outside)
                            let name = buf.trim().to_string();
                            if let Some(pane) = state
                                .active_workspace_mut()
                                .pane_layout_mut()
                                .find_pane_mut(pane_id)
                            {
                                if let Some(tab) = pane.tabs.get_mut(tab_idx) {
                                    if name.is_empty() {
                                        tab.explicit_name = None;
                                    } else {
                                        tab.explicit_name = Some(name);
                                    }
                                }
                            }
                            state.engine.mark_layout_dirty();
                            close = true;
                        }
                    }

                    // Auto-focus the text field
                    resp.request_focus();

                    // Update buffer
                    state.dialogs.tab_rename = Some((pane_id, tab_idx, buf));
                });
        });

    if close {
        state.dialogs.tab_rename = None;
    }
}

enum PaneTabAction {
    SwitchTab(usize),
    AddTab,
    ScrollLeft,
    OpenContextMenu(usize, egui::Pos2),
    OpenPaneContextMenu(egui::Pos2),
    ScrollRight,
}
