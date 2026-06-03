//! `PopupManager::draw` 거대 fn + scope helper 들 분리.

use crate::adapters::ui::LayoutContext;
use crate::theme;

use super::{PopupDrawResult, PopupId, PopupManager, PopupScope};

impl PopupManager {
    /// Draw all open popups. The `content_fn` callback is invoked for each popup with its id.
    /// `draw_ctx` provides scope context for visibility and boundary clamping.
    /// Returns draw result including closed popup IDs and hover state for input layer.
    pub fn draw(
        &mut self,
        ctx: &egui::Context,
        content_fn: &mut dyn FnMut(&str, &mut egui::Ui),
        draw_ctx: Option<&LayoutContext>,
    ) -> PopupDrawResult {
        let th = theme::theme();
        let screen_rect = ctx.screen_rect();
        let mut closed: Vec<PopupId> = Vec::new();
        let mut bring_front: Option<PopupId> = None;

        // Read pointer state once
        let pointer_pos = ctx.input(|i| i.pointer.interact_pos());
        let primary_pressed = ctx.input(|i| i.pointer.primary_pressed());
        let primary_down = ctx.input(|i| i.pointer.primary_down());
        let primary_released = ctx.input(|i| i.pointer.any_released());

        // Collect open popup indices, filtered by scope visibility
        let open_indices: Vec<usize> = self
            .popups
            .iter()
            .enumerate()
            .filter(|(_, p)| p.open && Self::is_scope_visible(&p.scope, draw_ctx))
            .map(|(i, _)| i)
            .collect();

        // Determine which popup (topmost) the pointer is over
        let mut hovered_popup: Option<PopupId> = None;
        let mut hovered_title: Option<PopupId> = None;
        let mut hovered_close: Option<PopupId> = None;
        if let Some(pos) = pointer_pos {
            // Check in reverse z-order (topmost first) for correct hit-testing
            for &idx in open_indices.iter().rev() {
                let popup = &self.popups[idx];
                if popup.popup_rect().contains(pos) {
                    hovered_popup = Some(popup.id);
                    if !popup.headless {
                        if popup.close_btn_rect().contains(pos) {
                            hovered_close = Some(popup.id);
                        } else if popup.title_rect().contains(pos) {
                            hovered_title = Some(popup.id);
                        }
                    }
                    break; // topmost popup wins
                }
            }
        }

        // Handle close button click and focus
        if primary_pressed {
            if let Some(id) = hovered_close {
                closed.push(id);
            } else if let Some(id) = hovered_popup {
                bring_front = Some(id);
                // Focus this popup, unfocus all others
                for popup in &mut self.popups {
                    popup.focused = popup.id == id;
                }
            } else {
                // Clicked outside all popups
                for popup in &mut self.popups {
                    if popup.open && popup.close_on_outside_click {
                        closed.push(popup.id);
                    }
                    // sticky_focus popups keep keyboard focus when clicking outside.
                    if !popup.sticky_focus {
                        popup.focused = false;
                    }
                }
            }
        }

        // Handle drag start
        if primary_pressed
            && let Some(id) = hovered_title
            && hovered_close.is_none()
        {
            if let Some(popup) = self.popups.iter_mut().find(|p| p.id == id) {
                popup.dragging = true;
                if let Some(pos) = pointer_pos {
                    popup.drag_offset = pos - popup.pos;
                }
            }
            bring_front = Some(id);
        }

        // Handle drag move / release
        for popup in &mut self.popups {
            if !popup.dragging {
                continue;
            }
            if primary_released {
                popup.dragging = false;
            } else if primary_down && let Some(pos) = pointer_pos {
                let bounds = Self::scope_rect(&popup.scope, draw_ctx).unwrap_or(screen_rect);
                let new_pos = pos - popup.drag_offset;
                popup.pos = egui::pos2(
                    new_pos.x.clamp(
                        bounds.min.x,
                        (bounds.max.x - popup.size.x).max(bounds.min.x),
                    ),
                    new_pos.y.clamp(
                        bounds.min.y,
                        (bounds.max.y - popup.size.y).max(bounds.min.y),
                    ),
                );
            }
        }

        // Handle request_center (use scope rect if available, else screen rect)
        for popup in &mut self.popups {
            if popup.request_center && popup.open {
                let center_rect = Self::scope_rect(&popup.scope, draw_ctx).unwrap_or(screen_rect);
                popup.pos = egui::pos2(
                    center_rect.center().x - popup.size.x / 2.0,
                    center_rect.center().y - popup.size.y / 2.0,
                );
                popup.request_center = false;
            }
        }

        // Handle request_top — scope rect 상단 가로 중앙 정렬 (margin 8px).
        for popup in &mut self.popups {
            if popup.request_top && popup.open {
                let anchor_rect = Self::scope_rect(&popup.scope, draw_ctx).unwrap_or(screen_rect);
                popup.pos = egui::pos2(
                    anchor_rect.center().x - popup.size.x / 2.0,
                    anchor_rect.min.y + 8.0,
                );
                popup.request_top = false;
            }
        }

        // Set cursor for popup hover
        if hovered_title.is_some() && hovered_close.is_none() {
            ctx.set_cursor_icon(egui::CursorIcon::Grab);
        } else if hovered_popup.is_some() && hovered_title.is_none() {
            // Content area: set default cursor (arrow) to override terminal cursor
            ctx.set_cursor_icon(egui::CursorIcon::Default);
        }

        // --- Render all open popups ---
        for (z_idx, &popup_idx) in open_indices.iter().enumerate() {
            let popup = &mut self.popups[popup_idx];
            if closed.contains(&popup.id) {
                continue;
            }

            let clamp_rect = Self::scope_rect(&popup.scope, draw_ctx).unwrap_or(screen_rect);
            popup.clamp_to_screen(clamp_rect);

            let popup_id = popup.id;
            let is_headless = popup.headless;
            let popup_rect = popup.popup_rect();
            let content_rect = popup.content_rect();

            let layer_id = egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("popup").with(popup_id).with(z_idx),
            );

            let painter = ctx.layer_painter(layer_id);

            // Popup background
            painter.rect_filled(popup_rect, th.corner_radius.value(), th.surface0);
            painter.rect_stroke(
                popup_rect,
                th.corner_radius.value(),
                egui::Stroke::new(th.border_width.value(), th.surface1),
                egui::StrokeKind::Outside,
            );

            if !is_headless {
                let title_rect = popup.title_rect();
                let close_btn_rect = popup.close_btn_rect();

                // Title bar
                let cr = th.corner_radius.value() as u8;
                painter.rect_filled(
                    title_rect,
                    egui::CornerRadius {
                        nw: cr,
                        ne: cr,
                        sw: 0,
                        se: 0,
                    },
                    th.mantle,
                );
                painter.line_segment(
                    [
                        egui::pos2(title_rect.min.x, title_rect.max.y),
                        egui::pos2(title_rect.max.x, title_rect.max.y),
                    ],
                    egui::Stroke::new(th.border_width.value(), th.surface1),
                );

                // Title text (centered)
                painter.text(
                    title_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    &popup.title,
                    egui::FontId::proportional(th.font_size_body.value()),
                    th.text.into(),
                );

                // Close button
                let is_close_hovered = hovered_close == Some(popup_id);
                if is_close_hovered {
                    painter.rect_filled(
                        close_btn_rect,
                        2.0,
                        th.hover_overlay.to_egui_premultiplied(),
                    );
                }
                let x_size = 5.0;
                let x_color = if is_close_hovered {
                    th.red
                } else {
                    th.subtext0
                };
                let center = close_btn_rect.center();
                painter.line_segment(
                    [
                        center - egui::vec2(x_size, x_size),
                        center + egui::vec2(x_size, x_size),
                    ],
                    egui::Stroke::new(1.5, x_color),
                );
                painter.line_segment(
                    [
                        center + egui::vec2(-x_size, x_size),
                        center + egui::vec2(x_size, -x_size),
                    ],
                    egui::Stroke::new(1.5, x_color),
                );
            }

            // Content
            {
                let mut child_ui = egui::Ui::new(
                    ctx.clone(),
                    egui::Id::new("popup_content").with(popup_id),
                    egui::UiBuilder::new()
                        .layer_id(layer_id)
                        .max_rect(content_rect),
                );
                content_fn(popup_id, &mut child_ui);
            }
        }

        // Apply close
        for id in &closed {
            self.close(id);
        }

        // Bring clicked popup to front
        if let Some(id) = bring_front {
            self.bring_to_front(id);
        }

        PopupDrawResult {
            closed,
            hovered: hovered_popup.is_some(),
        }
    }

    /// Check if a popup's scope is currently visible.
    fn is_scope_visible(scope: &PopupScope, ctx: Option<&LayoutContext>) -> bool {
        let Some(ctx) = ctx else { return true };
        match scope {
            PopupScope::Window => true,
            PopupScope::Workspace(ws_idx) => *ws_idx == ctx.active_workspace,
            PopupScope::Pane(pane_id) => ctx.pane_rects.iter().any(|(id, _)| *id == *pane_id),
            PopupScope::Tab(pane_id, tab_idx) => ctx
                .active_tabs
                .iter()
                .any(|(pid, tidx)| *pid == *pane_id && *tidx == *tab_idx),
            PopupScope::Surface(surface_id) => {
                ctx.surface_rects.iter().any(|(id, _)| *id == *surface_id)
            }
        }
    }

    /// Get the bounding rect for a popup's scope.
    fn scope_rect(scope: &PopupScope, ctx: Option<&LayoutContext>) -> Option<egui::Rect> {
        let ctx = ctx?;
        match scope {
            PopupScope::Window => None,       // use screen_rect (caller default)
            PopupScope::Workspace(_) => None, // workspace fills window
            PopupScope::Pane(pane_id) => ctx
                .pane_rects
                .iter()
                .find(|(id, _)| *id == *pane_id)
                .map(|(_, r)| *r),
            PopupScope::Tab(pane_id, _) => ctx
                .pane_rects
                .iter()
                .find(|(id, _)| *id == *pane_id)
                .map(|(_, r)| *r),
            PopupScope::Surface(surface_id) => ctx
                .surface_rects
                .iter()
                .find(|(id, _)| *id == *surface_id)
                .map(|(_, r)| *r),
        }
    }
}
