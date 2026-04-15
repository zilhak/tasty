use crate::state::AppState;
use crate::theme;

/// Unique identifier for a popup instance.
pub type PopupId = &'static str;

/// Result of a popup's draw call.
pub enum PopupAction {
    /// No action needed.
    None,
    /// The popup requests to be closed.
    Close,
}

/// Trait for popup content. Each popup type implements this to define
/// its own size, scope, and rendering logic.
pub trait PopupContent {
    /// Unique popup ID (must match the registered PopupState id).
    fn id(&self) -> PopupId;
    /// Title text for the title bar.
    fn title(&self) -> String;
    /// Default size of the popup.
    fn default_size(&self) -> egui::Vec2;
    /// Scope for visibility and boundary clamping. Default: Window.
    fn scope(&self) -> PopupScope { PopupScope::Window }
    /// Whether clicking outside closes this popup. Default: false.
    fn close_on_outside_click(&self) -> bool { false }
    /// Draw the popup content. Called each frame while the popup is open.
    fn draw(&mut self, ui: &mut egui::Ui, state: &mut AppState) -> PopupAction;
}

/// Scope determines where a popup is anchored and when it's visible.
#[derive(Debug, Clone, PartialEq)]
pub enum PopupScope {
    /// Always visible, clamped to window bounds.
    Window,
    /// Visible only when the specified workspace is active.
    Workspace(usize),
    /// Visible only when the specified pane is visible, clamped to pane bounds.
    Pane(u32),
    /// Visible only when the specified tab is active, clamped to pane bounds.
    Tab(u32, usize),
    /// Visible only when the specified surface is visible, clamped to surface bounds.
    Surface(u32),
}

/// Context passed to PopupManager::draw() for scope-based visibility/clamping.
pub struct PopupDrawContext {
    pub active_workspace: usize,
    /// (pane_id, rect) for all visible panes.
    pub pane_rects: Vec<(u32, egui::Rect)>,
    /// (surface_id, rect) for all visible surfaces.
    pub surface_rects: Vec<(u32, egui::Rect)>,
    /// (pane_id, active_tab_index) for each pane.
    pub active_tabs: Vec<(u32, usize)>,
}

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
    /// Scope determines visibility and boundary clamping.
    pub scope: PopupScope,
    /// Whether this popup currently has keyboard focus.
    /// When focused, keyboard input should NOT be forwarded to the terminal.
    pub focused: bool,
    /// If true, clicking outside this popup will close it (not just unfocus).
    pub close_on_outside_click: bool,
    /// If true, PopupManager will center this popup on the next draw and clear the flag.
    pub request_center: bool,
}

pub const TITLE_BAR_HEIGHT: f32 = 28.0;
pub const CONTENT_MARGIN: f32 = 4.0;

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
            scope: PopupScope::Window,
            focused: false,
            close_on_outside_click: false,
            request_center: false,
        }
    }

    /// Create a popup with a specific scope.
    pub fn with_scope(mut self, scope: PopupScope) -> Self {
        self.scope = scope;
        self
    }

    /// Set whether clicking outside this popup should close it.
    pub fn with_close_on_outside_click(mut self, v: bool) -> Self {
        self.close_on_outside_click = v;
        self
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

    /// Register a popup from a PopupContent trait object.
    /// Creates the PopupState automatically from the content's properties.
    pub fn register_content(&mut self, content: &dyn PopupContent) {
        let id = content.id();
        if !self.popups.iter().any(|p| p.id == id) {
            let popup = PopupState::new(id, content.title(), content.default_size())
                .with_scope(content.scope())
                .with_close_on_outside_click(content.close_on_outside_click());
            self.popups.push(popup);
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

    /// Open a popup centered on screen, with focus.
    pub fn open_centered_focused(&mut self, id: PopupId) {
        if let Some(i) = self.popups.iter().position(|p| p.id == id) {
            self.popups[i].open = true;
            self.popups[i].focused = true;
            self.popups[i].request_center = true;
            let popup = self.popups.remove(i);
            self.popups.push(popup);
        }
    }

    /// Open a popup centered within a specific scope, with focus.
    pub fn open_with_scope(&mut self, id: PopupId, scope: PopupScope) {
        if let Some(i) = self.popups.iter().position(|p| p.id == id) {
            self.popups[i].open = true;
            self.popups[i].focused = true;
            self.popups[i].request_center = true;
            self.popups[i].scope = scope;
            let popup = self.popups.remove(i);
            self.popups.push(popup);
        }
    }

    /// Close a popup by id.
    pub fn close(&mut self, id: PopupId) {
        if let Some(p) = self.popups.iter_mut().find(|p| p.id == id) {
            p.open = false;
            p.dragging = false;
            p.focused = false;
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

    /// Check if any popup currently has keyboard focus.
    pub fn has_focused(&self) -> bool {
        self.popups.iter().any(|p| p.open && p.focused)
    }

    /// Check if any popup is currently open.
    pub fn has_any_open(&self) -> bool {
        self.popups.iter().any(|p| p.open)
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

    /// Draw all open popups. The `content_fn` callback is invoked for each popup with its id.
    /// `draw_ctx` provides scope context for visibility and boundary clamping.
    /// Returns a list of popup ids that were closed via the X button or outside click.
    pub fn draw(
        &mut self,
        ctx: &egui::Context,
        content_fn: &mut dyn FnMut(&str, &mut egui::Ui),
        draw_ctx: Option<&PopupDrawContext>,
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
                    if popup.close_btn_rect().contains(pos) {
                        hovered_close = Some(popup.id);
                    } else if popup.title_rect().contains(pos) {
                        hovered_title = Some(popup.id);
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
                    popup.focused = false;
                }
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
                    let bounds = Self::scope_rect(&popup.scope, draw_ctx).unwrap_or(screen_rect);
                    let new_pos = pos - popup.drag_offset;
                    popup.pos = egui::pos2(
                        new_pos.x.clamp(bounds.min.x, (bounds.max.x - popup.size.x).max(bounds.min.x)),
                        new_pos.y.clamp(bounds.min.y, (bounds.max.y - popup.size.y).max(bounds.min.y)),
                    );
                }
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

        closed
    }

    /// Check if a popup's scope is currently visible.
    fn is_scope_visible(scope: &PopupScope, ctx: Option<&PopupDrawContext>) -> bool {
        let Some(ctx) = ctx else { return true };
        match scope {
            PopupScope::Window => true,
            PopupScope::Workspace(ws_idx) => *ws_idx == ctx.active_workspace,
            PopupScope::Pane(pane_id) => ctx.pane_rects.iter().any(|(id, _)| *id == *pane_id),
            PopupScope::Tab(pane_id, tab_idx) => {
                ctx.active_tabs.iter().any(|(pid, tidx)| *pid == *pane_id && *tidx == *tab_idx)
            }
            PopupScope::Surface(surface_id) => {
                ctx.surface_rects.iter().any(|(id, _)| *id == *surface_id)
            }
        }
    }

    /// Get the bounding rect for a popup's scope.
    fn scope_rect(scope: &PopupScope, ctx: Option<&PopupDrawContext>) -> Option<egui::Rect> {
        let ctx = ctx?;
        match scope {
            PopupScope::Window => None, // use screen_rect (caller default)
            PopupScope::Workspace(_) => None, // workspace fills window
            PopupScope::Pane(pane_id) => {
                ctx.pane_rects.iter().find(|(id, _)| *id == *pane_id).map(|(_, r)| *r)
            }
            PopupScope::Tab(pane_id, _) => {
                ctx.pane_rects.iter().find(|(id, _)| *id == *pane_id).map(|(_, r)| *r)
            }
            PopupScope::Surface(surface_id) => {
                ctx.surface_rects.iter().find(|(id, _)| *id == *surface_id).map(|(_, r)| *r)
            }
        }
    }
}
