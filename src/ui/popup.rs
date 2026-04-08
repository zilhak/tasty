use crate::theme;

/// Unique identifier for a popup instance.
pub type PopupId = &'static str;

/// State for a single popup instance.
#[derive(Debug, Clone)]
pub struct PopupState {
    /// Unique identifier.
    pub id: PopupId,
    /// Title text displayed in the title bar.
    pub title: String,
    /// Whether the popup is currently visible.
    pub open: bool,
    /// Position in logical pixels (top-left corner).
    pub pos: egui::Pos2,
    /// Size in logical pixels.
    pub size: egui::Vec2,
    /// Whether the popup is currently being dragged.
    dragging: bool,
    /// Drag offset from popup top-left to mouse position.
    drag_offset: egui::Vec2,
}

const TITLE_BAR_HEIGHT: f32 = 28.0;
const CONTENT_MARGIN: f32 = 4.0;

impl PopupState {
    pub fn new(id: PopupId, title: impl Into<String>, default_size: egui::Vec2) -> Self {
        Self {
            id,
            title: title.into(),
            open: false,
            pos: egui::pos2(100.0, 100.0),
            size: default_size,
            dragging: false,
            drag_offset: egui::Vec2::ZERO,
        }
    }

    fn popup_rect(&self) -> egui::Rect {
        egui::Rect::from_min_size(self.pos, self.size)
    }

    fn title_rect(&self) -> egui::Rect {
        egui::Rect::from_min_size(self.pos, egui::vec2(self.size.x, TITLE_BAR_HEIGHT))
    }

    fn content_rect(&self) -> egui::Rect {
        let popup = self.popup_rect();
        egui::Rect::from_min_max(
            egui::pos2(popup.min.x + CONTENT_MARGIN, popup.min.y + TITLE_BAR_HEIGHT + CONTENT_MARGIN),
            egui::pos2(popup.max.x - CONTENT_MARGIN, popup.max.y - CONTENT_MARGIN),
        )
    }

    fn close_btn_rect(&self) -> egui::Rect {
        let title = self.title_rect();
        let size = 20.0;
        let center = egui::pos2(title.max.x - size * 0.5 - 4.0, title.center().y);
        egui::Rect::from_center_size(center, egui::vec2(size, size))
    }

    /// Clamp position so popup stays within the given screen rect.
    fn clamp_to_screen(&mut self, screen: egui::Rect) {
        self.size.x = self.size.x.min(screen.width());
        self.size.y = self.size.y.min(screen.height());
        self.pos.x = self.pos.x.clamp(screen.min.x, (screen.max.x - self.size.x).max(screen.min.x));
        self.pos.y = self.pos.y.clamp(screen.min.y, (screen.max.y - self.size.y).max(screen.min.y));
    }
}

/// Manager for all internal popups. Handles z-ordering, dragging, and window clamping.
pub struct PopupManager {
    /// Popups in z-order (last = topmost).
    popups: Vec<PopupState>,
}

impl PopupManager {
    pub fn new() -> Self {
        Self {
            popups: Vec::new(),
        }
    }

    /// Register a popup. Call once during init. Does nothing if already registered.
    pub fn register(&mut self, popup: PopupState) {
        if !self.popups.iter().any(|p| p.id == popup.id) {
            self.popups.push(popup);
        }
    }

    /// Open a popup by id, bringing it to the front.
    pub fn open(&mut self, id: PopupId) {
        if let Some(i) = self.popups.iter().position(|p| p.id == id) {
            self.popups[i].open = true;
            let popup = self.popups.remove(i);
            self.popups.push(popup);
        }
    }

    /// Close a popup by id.
    pub fn close(&mut self, id: PopupId) {
        if let Some(p) = self.popups.iter_mut().find(|p| p.id == id) {
            p.open = false;
            p.dragging = false;
        }
    }

    /// Toggle a popup open/closed.
    pub fn toggle(&mut self, id: PopupId) {
        if self.is_open(id) {
            self.close(id);
        } else {
            self.open(id);
        }
    }

    /// Check if a popup is open.
    pub fn is_open(&self, id: PopupId) -> bool {
        self.popups.iter().any(|p| p.id == id && p.open)
    }

    /// Bring a popup to the front (topmost z-order).
    fn bring_to_front(&mut self, id: PopupId) {
        if let Some(i) = self.popups.iter().position(|p| p.id == id) {
            let popup = self.popups.remove(i);
            self.popups.push(popup);
        }
    }

    /// Get mutable access to a popup's state.
    pub fn get_mut(&mut self, id: PopupId) -> Option<&mut PopupState> {
        self.popups.iter_mut().find(|p| p.id == id)
    }

    /// Draw all open popups. The `content_fns` provides drawing callbacks per popup id.
    /// Returns a list of popup ids that were closed via the X button.
    pub fn draw(
        &mut self,
        ctx: &egui::Context,
        content_fns: &mut [(&'static str, &mut dyn FnMut(&mut egui::Ui))],
    ) -> Vec<PopupId> {
        let th = theme::theme();
        let screen_rect = ctx.screen_rect();
        let mut closed: Vec<PopupId> = Vec::new();
        let mut bring_front: Option<PopupId> = None;

        // Read pointer state once
        let pointer_pos = ctx.input(|i| i.pointer.interact_pos());
        let primary_pressed = ctx.input(|i| i.pointer.primary_pressed());
        let primary_down = ctx.input(|i| i.pointer.primary_down());
        let primary_released = ctx.input(|i| i.pointer.any_released());

        // Collect open popup indices (z-order: first = back, last = front)
        let open_indices: Vec<usize> = self
            .popups
            .iter()
            .enumerate()
            .filter(|(_, p)| p.open)
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
                    if popup.close_btn_rect().contains(pos) {
                        hovered_close = Some(popup.id);
                    } else if popup.title_rect().contains(pos) {
                        hovered_title = Some(popup.id);
                    }
                    break; // topmost popup wins
                }
            }
        }

        // Handle close button click
        if primary_pressed {
            if let Some(id) = hovered_close {
                closed.push(id);
            } else if let Some(id) = hovered_popup {
                bring_front = Some(id);
            }
        }

        // Handle drag start
        if primary_pressed {
            if let Some(id) = hovered_title {
                if hovered_close.is_none() {
                    if let Some(popup) = self.popups.iter_mut().find(|p| p.id == id) {
                        popup.dragging = true;
                        if let Some(pos) = pointer_pos {
                            popup.drag_offset = pos - popup.pos;
                        }
                    }
                    bring_front = Some(id);
                }
            }
        }

        // Handle drag move / release
        for popup in &mut self.popups {
            if !popup.dragging {
                continue;
            }
            if primary_released {
                popup.dragging = false;
            } else if primary_down {
                if let Some(pos) = pointer_pos {
                    let new_pos = pos - popup.drag_offset;
                    popup.pos = egui::pos2(
                        new_pos.x.clamp(screen_rect.min.x, (screen_rect.max.x - popup.size.x).max(screen_rect.min.x)),
                        new_pos.y.clamp(screen_rect.min.y, (screen_rect.max.y - popup.size.y).max(screen_rect.min.y)),
                    );
                }
            }
        }

        // Set cursor for title bar hover
        if hovered_title.is_some() && hovered_close.is_none() {
            ctx.set_cursor_icon(egui::CursorIcon::Grab);
        }

        // --- Render all open popups ---
        for (z_idx, &popup_idx) in open_indices.iter().enumerate() {
            let popup = &mut self.popups[popup_idx];
            if closed.contains(&popup.id) {
                continue;
            }

            popup.clamp_to_screen(screen_rect);

            let popup_id = popup.id;
            let popup_rect = popup.popup_rect();
            let title_rect = popup.title_rect();
            let content_rect = popup.content_rect();
            let close_btn_rect = popup.close_btn_rect();

            let layer_id = egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("popup").with(popup_id).with(z_idx),
            );

            let painter = ctx.layer_painter(layer_id);

            // Popup background
            painter.rect_filled(popup_rect, th.corner_radius, th.surface0);
            painter.rect_stroke(
                popup_rect,
                th.corner_radius,
                egui::Stroke::new(th.border_width, th.surface1),
                egui::StrokeKind::Outside,
            );

            // Title bar
            let cr = th.corner_radius as u8;
            painter.rect_filled(
                title_rect,
                egui::CornerRadius { nw: cr, ne: cr, sw: 0, se: 0 },
                th.mantle,
            );
            painter.line_segment(
                [
                    egui::pos2(title_rect.min.x, title_rect.max.y),
                    egui::pos2(title_rect.max.x, title_rect.max.y),
                ],
                egui::Stroke::new(th.border_width, th.surface1),
            );

            // Title text (centered)
            painter.text(
                title_rect.center(),
                egui::Align2::CENTER_CENTER,
                &popup.title,
                egui::FontId::proportional(th.font_size_body),
                th.text,
            );

            // Close button
            let is_close_hovered = hovered_close == Some(popup_id);
            if is_close_hovered {
                painter.rect_filled(close_btn_rect, 2.0, th.hover_overlay);
            }
            let x_size = 5.0;
            let x_color = if is_close_hovered { th.red } else { th.subtext0 };
            let center = close_btn_rect.center();
            painter.line_segment(
                [center - egui::vec2(x_size, x_size), center + egui::vec2(x_size, x_size)],
                egui::Stroke::new(1.5, x_color),
            );
            painter.line_segment(
                [center + egui::vec2(-x_size, x_size), center + egui::vec2(x_size, -x_size)],
                egui::Stroke::new(1.5, x_color),
            );

            // Content
            if let Some((_id, content_fn)) = content_fns.iter_mut().find(|(id, _)| *id == popup_id) {
                let mut child_ui = egui::Ui::new(
                    ctx.clone(),
                    egui::Id::new("popup_content").with(popup_id),
                    egui::UiBuilder::new()
                        .layer_id(layer_id)
                        .max_rect(content_rect),
                );
                content_fn(&mut child_ui);
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

        closed
    }
}
