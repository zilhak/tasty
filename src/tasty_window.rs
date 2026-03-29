use std::sync::Arc;

use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{CursorIcon, Window};

use crate::gpu::GpuState;
use crate::model::{Rect, SplitDirection};
use crate::selection::{self, TextSelection, SelectionMode, SelectionPoint};
use crate::state::AppState;
use crate::{AppEvent, ClipboardContext, DividerDrag, DividerDragKind};

/// A single Tasty window with its own GPU state, UI state, and input state.
pub struct TastyWindow {
    pub gpu: GpuState,
    pub state: AppState,
    pub window: Arc<Window>,
    pub dirty: bool,
    pub modifiers: ModifiersState,
    pub window_focused: bool,
    pub cursor_position: Option<winit::dpi::PhysicalPosition<f64>>,
    pub dragging_divider: Option<DividerDrag>,
    pub clipboard: Option<ClipboardContext>,
    pub preedit_text: String,
    pub proxy: winit::event_loop::EventLoopProxy<AppEvent>,
    pub text_selection: Option<TextSelection>,
    pub left_mouse_down: bool,
    pub last_click_time: Option<std::time::Instant>,
    pub last_click_pos: Option<(usize, usize)>,
    pub click_count: u8,
}

impl TastyWindow {
    pub fn new(gpu: GpuState, state: AppState, window: Arc<Window>, proxy: winit::event_loop::EventLoopProxy<AppEvent>) -> Self {
        Self {
            gpu, state, window,
            dirty: true,
            modifiers: ModifiersState::empty(),
            window_focused: true,
            cursor_position: None,
            dragging_divider: None,
            clipboard: ClipboardContext::new(),
            preedit_text: String::new(),
            proxy,
            text_selection: None,
            left_mouse_down: false,
            last_click_time: None,
            last_click_pos: None,
            click_count: 0,
        }
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
        self.window.request_redraw();
    }

    pub fn compute_terminal_rect(&self) -> Rect {
        let size = self.gpu.size();
        crate::model::compute_terminal_rect(
            size.width as f32, size.height as f32,
            self.state.sidebar_width, self.gpu.scale_factor(),
        )
    }

    /// Start a new text selection from the given pixel position.
    fn start_selection(&mut self, x: f32, y: f32, terminal_rect: &Rect) {
        if let Some((point, surface_id)) = self.mouse_to_grid(x, y, terminal_rect) {
            // Detect multi-click
            let now = std::time::Instant::now();
            let same_pos = self.last_click_pos.map_or(false, |(c, r)| c == point.col && r == point.absolute_row);
            let within_time = self.last_click_time.map_or(false, |t| now.duration_since(t).as_millis() < 400);
            if same_pos && within_time {
                self.click_count = (self.click_count + 1).min(3);
            } else {
                self.click_count = 1;
            }
            self.last_click_time = Some(now);
            self.last_click_pos = Some((point.col, point.absolute_row));

            let (mode, dragging) = match self.click_count {
                2 => (SelectionMode::Word, false),
                3 => {
                    self.click_count = 0; // Reset after triple
                    (SelectionMode::Line, false)
                }
                _ => (SelectionMode::Normal, true),
            };

            // For word/line mode, expand anchor/cursor
            let (anchor, cursor) = match mode {
                SelectionMode::Word => {
                    let (start_col, end_col) = self.find_word_bounds(point.col, point.absolute_row);
                    (
                        SelectionPoint { col: start_col, absolute_row: point.absolute_row },
                        SelectionPoint { col: end_col, absolute_row: point.absolute_row },
                    )
                }
                SelectionMode::Line => {
                    let cols = self.state.focused_terminal()
                        .map(|t| t.surface().dimensions().0)
                        .unwrap_or(80);
                    (
                        SelectionPoint { col: 0, absolute_row: point.absolute_row },
                        SelectionPoint { col: cols.saturating_sub(1), absolute_row: point.absolute_row },
                    )
                }
                SelectionMode::Normal => {
                    // Clear any existing selection on single click
                    (point, point)
                }
            };

            self.text_selection = Some(TextSelection {
                anchor,
                cursor,
                mode,
                surface_id,
                dragging,
            });
            self.mark_dirty();
        } else {
            // Clicked outside terminal — clear selection

            self.text_selection = None;
        }
    }

    /// Find word boundaries around the given column in the given absolute row.
    fn find_word_bounds(&self, col: usize, absolute_row: usize) -> (usize, usize) {
        let terminal = match self.state.focused_terminal() {
            Some(t) => t,
            None => return (col, col),
        };
        let scrollback_len = terminal.scrollback_len();

        // Get the text for this row
        let row_text: Vec<(String, usize)> = if absolute_row < scrollback_len {
            match terminal.scrollback_line_owned(absolute_row) {
                Some(line) => {
                    let mut result = Vec::new();
                    let mut c = 0;
                    for (text, _) in &line {
                        let ch = text.chars().next().unwrap_or(' ');
                        let w = crate::renderer::unicode_width(ch);
                        result.push((text.clone(), c));
                        c += w;
                    }
                    result
                }
                None => return (col, col),
            }
        } else {
            let screen_row = absolute_row - scrollback_len;
            let surface = terminal.surface();
            let lines = surface.screen_lines();
            match lines.get(screen_row) {
                Some(line) => {
                    line.visible_cells()
                        .map(|cell| (cell.str().to_string(), cell.cell_index()))
                        .collect()
                }
                None => return (col, col),
            }
        };

        // Find which cell the col is in
        let is_word_char = |s: &str| -> bool {
            s.chars().next().map_or(false, |c| c.is_alphanumeric() || c == '_')
        };

        // Find the cell at col
        let target_idx = row_text.iter().position(|(_, c)| *c >= col).unwrap_or(row_text.len().saturating_sub(1));
        if row_text.is_empty() {
            return (col, col);
        }
        let target_idx = target_idx.min(row_text.len() - 1);
        let word = is_word_char(&row_text[target_idx].0);

        // Expand left
        let mut start = target_idx;
        while start > 0 && is_word_char(&row_text[start - 1].0) == word {
            start -= 1;
        }
        // Expand right
        let mut end = target_idx;
        while end + 1 < row_text.len() && is_word_char(&row_text[end + 1].0) == word {
            end += 1;
        }

        let start_col = row_text[start].1;
        let end_text = &row_text[end].0;
        let end_ch = end_text.chars().next().unwrap_or(' ');
        let end_col = row_text[end].1 + crate::renderer::unicode_width(end_ch) - 1;
        (start_col, end_col)
    }

    /// Move the terminal cursor to the clicked position by sending arrow key sequences.
    /// Supports multi-row movement for soft-wrapped command lines.
    fn move_cursor_to_click(&mut self, x: f32, y: f32, terminal_rect: &Rect) {
        let terminal = match self.state.focused_terminal() {
            Some(t) => t,
            None => return,
        };

        // Don't move cursor if scrolled back or in alternate screen (TUI apps handle their own mouse)
        if terminal.scroll_offset > 0 || terminal.is_alternate_screen() {
            return;
        }

        // Don't move cursor if mouse tracking is active (app handles mouse)
        if terminal.mouse_tracking() != tasty_terminal::MouseTrackingMode::None {
            return;
        }

        let (cols, rows) = terminal.surface().dimensions();
        let padding = 4.0;
        let cell_w = self.gpu.cell_width();
        let cell_h = self.gpu.cell_height();

        // Convert click position to grid column/row
        let rel_x = x - terminal_rect.x - padding;
        let rel_y = y - terminal_rect.y - padding;
        let click_col = (rel_x / cell_w).floor() as isize;
        let click_col = click_col.clamp(0, (cols as isize) - 1) as usize;
        let click_row = (rel_y / cell_h).floor() as isize;
        let click_row = click_row.clamp(0, (rows as isize) - 1) as usize;

        // Get current cursor position
        let (cursor_col, cursor_row) = terminal.surface().cursor_position();
        let cursor_row = cursor_row as usize;

        if click_row == cursor_row && click_col == cursor_col {
            return;
        }

        // Don't allow clicking below the cursor row (empty/non-editable area)
        if click_row > cursor_row {
            return;
        }

        let surface = terminal.surface();
        let screen_lines = surface.screen_lines();

        // For clicks above cursor row, only allow if every row from click_row
        // to cursor_row-1 is fully filled (soft-wrapped continuation).
        // Lines that don't fill the terminal width are hard line breaks (previous output).
        if click_row < cursor_row {
            for row in click_row..cursor_row {
                let line = match screen_lines.get(row) {
                    Some(l) => l,
                    None => return,
                };
                let last_col = line.visible_cells()
                    .map(|c| {
                        let ch = c.str().chars().next().unwrap_or(' ');
                        c.cell_index() + crate::renderer::unicode_width(ch)
                    })
                    .max()
                    .unwrap_or(0);
                if last_col < cols {
                    return; // Not a soft wrap — previous output line
                }
            }
        }

        // On the cursor row, don't allow clicking past the cursor (into empty space)
        let click_col = if click_row == cursor_row && click_col > cursor_col {
            cursor_col
        } else {
            click_col
        };

        if click_row == cursor_row && click_col == cursor_col {
            return;
        }

        // Count arrow presses across rows, accounting for wide characters.
        let going_right = (click_row, click_col) > (cursor_row, cursor_col);
        let (start_row, start_col, end_row, end_col) = if going_right {
            (cursor_row, cursor_col, click_row, click_col)
        } else {
            (click_row, click_col, cursor_row, cursor_col)
        };

        let mut arrow_count = 0usize;
        for row in start_row..=end_row {
            let line = match screen_lines.get(row) {
                Some(l) => l,
                None => break,
            };

            let row_start = if row == start_row { start_col } else { 0 };
            let row_end = if row == end_row { end_col } else { cols };

            for cell_ref in line.visible_cells() {
                let col = cell_ref.cell_index();
                if col >= row_start && col < row_end {
                    arrow_count += 1;
                }
            }
        }

        if arrow_count == 0 {
            return;
        }

        // Send arrow keys
        let terminal = self.state.focused_terminal_mut().unwrap();
        let app_cursor = terminal.application_cursor_keys();
        let arrow: &[u8] = if going_right {
            if app_cursor { b"\x1bOC" } else { b"\x1b[C" }
        } else {
            if app_cursor { b"\x1bOD" } else { b"\x1b[D" }
        };
        for _ in 0..arrow_count {
            terminal.send_bytes(arrow);
        }
    }

    /// Convert mouse physical coordinates to a grid SelectionPoint for the focused terminal.
    fn mouse_to_grid(&self, x: f32, y: f32, viewport: &Rect) -> Option<(SelectionPoint, u32)> {
        let terminal = self.state.focused_terminal()?;
        let surface_id = self.state.focused_surface_id()?;
        let (cols, rows) = terminal.surface().dimensions();
        let point = selection::pixel_to_grid(
            x, y, viewport,
            self.gpu.cell_width(), self.gpu.cell_height(),
            cols, rows,
            terminal.scroll_offset,
            terminal.scrollback_len(),
        );
        Some((point, surface_id))
    }

    /// Copy the current selection to clipboard and clear selection.
    pub fn copy_selection_to_clipboard(&mut self) -> bool {
        let sel = match &self.text_selection {
            Some(s) if !s.is_empty() => s.clone(),
            _ => return false,
        };
        let text = if let Some(terminal) = self.state.find_terminal_by_id(sel.surface_id) {
            selection::extract_selected_text(terminal, &sel)
        } else {
            return false;
        };
        if text.is_empty() {
            return false;
        }
        if let Some(cb) = &mut self.clipboard {
            cb.set_text(&text);
        }

        self.text_selection = None;
        true
    }

    pub fn paste_to_terminal(&mut self) {
        let text = match &mut self.clipboard {
            Some(cb) => cb.get_text(),
            None => None,
        };
        if let Some(text) = text {
            if text.is_empty() { return; }
            if let Some(terminal) = self.state.focused_terminal_mut() {
                if terminal.bracketed_paste() {
                    terminal.send_bytes(b"\x1b[200~");
                    terminal.send_key(&text);
                    terminal.send_bytes(b"\x1b[201~");
                } else {
                    terminal.send_key(&text);
                }
            }
        }
    }

    /// Handle a window event. `modal_active` indicates if a modal is blocking input.
    pub fn handle_window_event(&mut self, event: WindowEvent, _event_loop: &ActiveEventLoop, modal_active: bool) -> bool {
        // Let egui handle the event first
        let (egui_consumed, egui_repaint) = self.gpu.handle_egui_event(&self.window, &event);
        if egui_repaint {
            self.mark_dirty();
        }

        // If a modal is active, only allow Resized/RedrawRequested/ScaleFactorChanged
        if modal_active {
            match &event {
                WindowEvent::Resized(_) | WindowEvent::RedrawRequested | WindowEvent::ScaleFactorChanged { .. } => {}
                _ => return false,
            }
        }

        let was_dirty = self.dirty;

        match event {
            WindowEvent::Resized(new_size) => {
                self.gpu.resize(new_size);
                let terminal_rect = self.compute_terminal_rect();
                let (cols, rows) = self.gpu.grid_size_for_rect(&terminal_rect);
                self.state.update_grid_size(cols, rows);
                self.state.resize_all(terminal_rect, self.gpu.cell_width(), self.gpu.cell_height());
                self.mark_dirty();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.gpu.update_scale_factor(scale_factor as f32);
                self.mark_dirty();
            }
            WindowEvent::Focused(focused) => {
                self.window_focused = focused;
                if !focused {
                    self.modifiers = ModifiersState::empty();
                }
                self.mark_dirty();
            }
            WindowEvent::Occluded(false) => {
                self.mark_dirty();
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                self.handle_keyboard_input(&event, egui_consumed);
            }
            WindowEvent::Ime(ime_event) => {
                self.handle_ime(ime_event, egui_consumed);
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.handle_cursor_moved(position, egui_consumed);
            }
            WindowEvent::CursorLeft { .. } => {
                self.cursor_position = None;
                self.window.set_cursor(CursorIcon::Default);
            }
            WindowEvent::MouseInput { state: button_state, button, .. } => {
                self.handle_mouse_input(button_state, button, egui_consumed);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                self.handle_mouse_wheel(delta, egui_consumed);
            }
            WindowEvent::RedrawRequested => {
                self.handle_redraw(_event_loop);
            }
            _ => {}
        }

        // If this event made us dirty, request a redraw.
        if self.dirty && !was_dirty {
            self.window.request_redraw();
        }

        false // don't exit
    }

    fn handle_keyboard_input(&mut self, event: &winit::event::KeyEvent, egui_consumed: bool) {
        if event.state != ElementState::Pressed {
            return;
        }

        if event.logical_key == Key::Named(NamedKey::Escape) {
            if self.state.settings_open {
                self.state.settings_open = false;
                self.state.settings_ui_state = crate::settings_ui::SettingsUiState::new();
                self.mark_dirty();
                return;
            }
            if self.state.notification_panel_open {
                self.state.notification_panel_open = false;
                self.mark_dirty();
                return;
            }
        }

        let settings_open = self.state.settings_open;
        let notification_open = self.state.notification_panel_open;
        let overlay_open = settings_open || notification_open;

        if !overlay_open || (notification_open && !settings_open) {
            if self.handle_shortcut(&event.logical_key, self.modifiers) {
                self.mark_dirty();
                return;
            }
        }
        if egui_consumed || overlay_open {
            if egui_consumed { self.mark_dirty(); }
            return;
        }

        // Forward to terminal
        let typing_surface_id = self.state.focused_surface_id();
        if let Some(terminal) = self.state.focused_terminal_mut() {
            let (dirty, sent) = Self::send_key_to_terminal(terminal, &event.logical_key, &event.text, self.modifiers);
            if dirty { self.dirty = true; }

            // Clear selection only when actual content was sent to the terminal PTY
            if sent && self.text_selection.is_some() {

                self.text_selection = None;
                self.dirty = true;
            }
        }
        if let Some(sid) = typing_surface_id {
            self.state.record_typing(sid);
        }
    }

    /// Send a key to the terminal. Returns (dirty, sent) where `sent` indicates
    /// whether any bytes were actually written to the terminal PTY.
    fn send_key_to_terminal(
        terminal: &mut tasty_terminal::Terminal,
        key: &Key,
        text: &Option<winit::keyboard::SmolStr>,
        modifiers: ModifiersState,
    ) -> (bool, bool) {
        let app_cursor = terminal.application_cursor_keys();
        let is_alt_screen = terminal.is_alternate_screen();
        let mut dirty = false;
        let mut sent = false;

        let is_scrollback_key = !is_alt_screen && matches!(
            key.as_ref(),
            Key::Named(NamedKey::PageUp) | Key::Named(NamedKey::PageDown)
        );

        if !is_scrollback_key && terminal.scroll_offset > 0 {
            terminal.scroll_to_bottom();
            dirty = true;
        }

        match key.as_ref() {
            Key::Named(NamedKey::Enter) => { terminal.send_bytes(b"\r"); sent = true; }
            Key::Named(NamedKey::Backspace) => { terminal.send_bytes(b"\x7f"); sent = true; }
            Key::Named(NamedKey::Tab) => {
                if modifiers.shift_key() { terminal.send_bytes(b"\x1b[Z"); }
                else { terminal.send_bytes(b"\t"); }
                sent = true;
            }
            Key::Named(NamedKey::Escape) => { terminal.send_bytes(b"\x1b"); sent = true; }
            Key::Named(NamedKey::ArrowUp) => {
                if app_cursor { terminal.send_bytes(b"\x1bOA") } else { terminal.send_bytes(b"\x1b[A") }
                sent = true;
            }
            Key::Named(NamedKey::ArrowDown) => {
                if app_cursor { terminal.send_bytes(b"\x1bOB") } else { terminal.send_bytes(b"\x1b[B") }
                sent = true;
            }
            Key::Named(NamedKey::ArrowRight) => {
                if app_cursor { terminal.send_bytes(b"\x1bOC") } else { terminal.send_bytes(b"\x1b[C") }
                sent = true;
            }
            Key::Named(NamedKey::ArrowLeft) => {
                if app_cursor { terminal.send_bytes(b"\x1bOD") } else { terminal.send_bytes(b"\x1b[D") }
                sent = true;
            }
            Key::Named(NamedKey::Home) => { terminal.send_bytes(b"\x1b[H"); sent = true; }
            Key::Named(NamedKey::End) => { terminal.send_bytes(b"\x1b[F"); sent = true; }
            Key::Named(NamedKey::PageUp) => {
                if is_alt_screen { terminal.send_bytes(b"\x1b[5~"); sent = true; }
                else { terminal.scroll_up(terminal.rows()); dirty = true; }
            }
            Key::Named(NamedKey::PageDown) => {
                if is_alt_screen { terminal.send_bytes(b"\x1b[6~"); sent = true; }
                else { terminal.scroll_down(terminal.rows()); dirty = true; }
            }
            Key::Named(NamedKey::Insert) => { terminal.send_bytes(b"\x1b[2~"); sent = true; }
            Key::Named(NamedKey::Delete) => { terminal.send_bytes(b"\x1b[3~"); sent = true; }
            Key::Named(NamedKey::F1) => { terminal.send_bytes(b"\x1bOP"); sent = true; }
            Key::Named(NamedKey::F2) => { terminal.send_bytes(b"\x1bOQ"); sent = true; }
            Key::Named(NamedKey::F3) => { terminal.send_bytes(b"\x1bOR"); sent = true; }
            Key::Named(NamedKey::F4) => { terminal.send_bytes(b"\x1bOS"); sent = true; }
            Key::Named(NamedKey::F5) => { terminal.send_bytes(b"\x1b[15~"); sent = true; }
            Key::Named(NamedKey::F6) => { terminal.send_bytes(b"\x1b[17~"); sent = true; }
            Key::Named(NamedKey::F7) => { terminal.send_bytes(b"\x1b[18~"); sent = true; }
            Key::Named(NamedKey::F8) => { terminal.send_bytes(b"\x1b[19~"); sent = true; }
            Key::Named(NamedKey::F9) => { terminal.send_bytes(b"\x1b[20~"); sent = true; }
            Key::Named(NamedKey::F10) => { terminal.send_bytes(b"\x1b[21~"); sent = true; }
            Key::Named(NamedKey::F11) => { terminal.send_bytes(b"\x1b[23~"); sent = true; }
            Key::Named(NamedKey::F12) => { terminal.send_bytes(b"\x1b[24~"); sent = true; }
            _ => {
                if let Some(text) = text {
                    let s = text.as_str();
                    if !s.is_empty() { terminal.send_key(s); sent = true; }
                }
            }
        }
        (dirty, sent)
    }

    fn handle_ime(&mut self, ime_event: winit::event::Ime, egui_consumed: bool) {
        if egui_consumed { self.mark_dirty(); return; }
        match ime_event {
            winit::event::Ime::Preedit(text, _cursor) => {
                self.preedit_text = text;
                self.mark_dirty();
            }
            winit::event::Ime::Commit(text) => {
                self.preedit_text.clear();
                let sid = self.state.focused_surface_id();
                if let Some(terminal) = self.state.focused_terminal_mut() {
                    terminal.send_key(&text);
                }
                if let Some(sid) = sid {
                    self.state.record_typing(sid);
                }
                self.mark_dirty();
            }
            _ => {}
        }
    }

    fn handle_cursor_moved(&mut self, position: winit::dpi::PhysicalPosition<f64>, egui_consumed: bool) {
        self.cursor_position = Some(position);
        let overlay_open = self.state.settings_open || self.state.notification_panel_open;
        if egui_consumed || overlay_open {
            if egui_consumed { self.mark_dirty(); }
            return;
        }

        let terminal_rect = self.compute_terminal_rect();
        let x = position.x as f32;
        let y = position.y as f32;

        // Handle selection drag
        if self.left_mouse_down && self.dragging_divider.is_none() {
            let is_dragging = self.text_selection.as_ref().map_or(false, |s| s.dragging);
            if is_dragging {
                if let Some((point, _)) = self.mouse_to_grid(x, y, &terminal_rect) {
                    if let Some(sel) = &mut self.text_selection {
                        sel.cursor = point;
                    }
                    self.mark_dirty();
                }
            }
        }

        if let Some(drag) = self.dragging_divider {
            let changed = match drag.kind {
                DividerDragKind::Pane => self.state.update_pane_divider(&drag.info, x, y, terminal_rect),
                DividerDragKind::Surface => self.state.update_surface_divider(&drag.info, x, y, terminal_rect),
            };
            if changed {
                self.state.resize_all(terminal_rect, self.gpu.cell_width(), self.gpu.cell_height());
                self.mark_dirty();
            }
        } else {
            let threshold = 4.0;
            let divider = self.state.find_pane_divider_at(x, y, terminal_rect, threshold)
                .or_else(|| self.state.find_surface_divider_at(x, y, terminal_rect, threshold));
            match divider {
                Some(info) => {
                    let cursor = match info.direction {
                        SplitDirection::Vertical => CursorIcon::ColResize,
                        SplitDirection::Horizontal => CursorIcon::RowResize,
                    };
                    self.window.set_cursor(cursor);
                }
                None => {
                    if self.state.is_over_terminal(x, y, terminal_rect) {
                        self.window.set_cursor(CursorIcon::Text);
                    } else {
                        self.window.set_cursor(CursorIcon::Default);
                    }
                }
            }
        }
    }

    fn handle_mouse_input(&mut self, button_state: ElementState, button: MouseButton, egui_consumed: bool) {
        let overlay_open = self.state.settings_open || self.state.notification_panel_open;
        if egui_consumed || overlay_open {
            if button_state == ElementState::Released {
                self.dragging_divider = None;
                self.left_mouse_down = false;
            }
            if egui_consumed { self.mark_dirty(); }
            return;
        }
        if button == MouseButton::Left {
            if button_state == ElementState::Pressed {
                self.left_mouse_down = true;
            } else {
                self.left_mouse_down = false;
            }

            if self.state.pane_context_menu.is_some() {
                self.state.pane_context_menu = None;
                self.dirty = true;
            }
            let terminal_rect = self.compute_terminal_rect();
            if let Some(pos) = self.cursor_position {
                let (x, y) = (pos.x as f32, pos.y as f32);
                if button_state == ElementState::Pressed {
                    let threshold = 4.0;
                    let pane_div = self.state.find_pane_divider_at(x, y, terminal_rect, threshold);
                    let surf_div = self.state.find_surface_divider_at(x, y, terminal_rect, threshold);
                    if let Some(info) = pane_div {
                        self.dragging_divider = Some(DividerDrag { info, kind: DividerDragKind::Pane });
                    } else if let Some(info) = surf_div {
                        self.dragging_divider = Some(DividerDrag { info, kind: DividerDragKind::Surface });
                    } else {
                        if self.state.focus_pane_at_position(x, y, terminal_rect) { self.dirty = true; }
                        if self.state.focus_surface_at_position(x, y, terminal_rect) { self.dirty = true; }

                        // Start text selection (only if not mouse-tracking or Shift held)
                        let mouse_tracking = self.state.focused_terminal()
                            .map(|t| t.mouse_tracking())
                            .unwrap_or(tasty_terminal::MouseTrackingMode::None);
                        let shift = self.modifiers.shift_key();
                        if mouse_tracking == tasty_terminal::MouseTrackingMode::None || shift {
                            self.start_selection(x, y, &terminal_rect);
                        }
                    }
                } else if button_state == ElementState::Released {
                    if self.dragging_divider.is_some() {
                        self.dragging_divider = None;
                        self.state.resize_all(terminal_rect, self.gpu.cell_width(), self.gpu.cell_height());
                        self.dirty = true;
                    }
                    // Finish selection drag
                    if let Some(sel) = &mut self.text_selection {
                        sel.dragging = false;
                        if sel.is_empty() {
                            // Single click (no drag) — move cursor to clicked position
                            self.move_cursor_to_click(x, y, &terminal_rect);
                            self.text_selection = None;
                        }
                    }
                    self.mark_dirty();
                }
            }
        } else if button == MouseButton::Right && button_state == ElementState::Pressed {
            let terminal_rect = self.compute_terminal_rect();
            if let Some(pos) = self.cursor_position {
                let (x, y) = (pos.x as f32, pos.y as f32);
                if terminal_rect.contains(x, y) {
                    let ws = self.state.active_workspace();
                    let pane_rects = ws.pane_layout().compute_rects(terminal_rect);
                    let scale = self.gpu.scale_factor();
                    for (pane_id, rect) in pane_rects {
                        if rect.contains(x, y) {
                            self.state.pane_context_menu = Some(crate::state::PaneContextMenu {
                                pane_id, x: x / scale, y: y / scale,
                            });
                            self.dirty = true;
                            break;
                        }
                    }
                }
            }
        }
    }

    fn handle_mouse_wheel(&mut self, delta: MouseScrollDelta, egui_consumed: bool) {
        let overlay_open = self.state.settings_open || self.state.notification_panel_open;
        if egui_consumed { self.mark_dirty(); }
        if !egui_consumed && !overlay_open {
            if let Some(terminal) = self.state.focused_terminal_mut() {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y as i32,
                    MouseScrollDelta::PixelDelta(pos) => (pos.y / 20.0) as i32,
                };
                if terminal.is_alternate_screen() {
                    if lines > 0 {
                        for _ in 0..lines.unsigned_abs() { terminal.send_bytes(b"\x1b[A"); }
                    } else if lines < 0 {
                        for _ in 0..lines.unsigned_abs() { terminal.send_bytes(b"\x1b[B"); }
                    }
                } else {
                    if lines > 0 { terminal.scroll_up(lines as usize); }
                    else if lines < 0 { terminal.scroll_down((-lines) as usize); }
                    self.dirty = true;
                }
            }
        }
    }

    fn handle_redraw(&mut self, event_loop: &ActiveEventLoop) {
        // Check if settings button was clicked (ui.rs sets state.settings_open = true)
        if self.state.settings_open {
            self.state.settings_open = false;
            let _ = self.proxy.send_event(crate::AppEvent::OpenSettings);
        }

        // When targeted_pty_polling is off, process all terminals every frame.
        // When on, individual terminals are processed via TerminalOutput(Some(id)) events,
        // but we still call process_all() as a safety net (it's a no-op if channels are empty).
        if self.state.process_all() {
            self.dirty = true;
        }

        // Collect terminal events
        let events = self.state.collect_events();
        for event in &events {
            let surface_id = event.surface_id;
            match &event.kind {
                crate::terminal::TerminalEventKind::Notification { title, body } => {
                    if self.state.engine.settings.notification.enabled
                        && self.state.engine.settings.notification.system_notification
                        && !self.window_focused
                        && self.state.engine.notifications.should_send_system_notification()
                    {
                        crate::notification::send_system_notification(title, body);
                    }
                    if self.state.engine.settings.notification.enabled {
                        let ws_id = self.state.active_workspace().id;
                        self.state.engine.notifications.add(ws_id, surface_id, title.clone(), body.clone());
                    }
                    let hook_events = vec![tasty_hooks::HookEvent::Notification];
                    self.state.engine.hook_manager.check_and_fire(surface_id, &hook_events);
                    self.dirty = true;
                }
                crate::terminal::TerminalEventKind::BellRing => {
                    if self.state.engine.settings.notification.enabled {
                        let ws_id = self.state.active_workspace().id;
                        self.state.engine.notifications.add(ws_id, surface_id, "Bell".to_string(), String::new());
                    }
                    if self.state.engine.settings.notification.enabled
                        && self.state.engine.settings.notification.system_notification
                        && !self.window_focused
                        && self.state.engine.notifications.should_send_system_notification()
                    {
                        crate::notification::send_system_notification("Tasty", "Bell");
                    }
                    let hook_events = vec![tasty_hooks::HookEvent::Bell];
                    self.state.engine.hook_manager.check_and_fire(surface_id, &hook_events);
                    self.dirty = true;
                }
                crate::terminal::TerminalEventKind::TitleChanged(_) => { self.dirty = true; }
                crate::terminal::TerminalEventKind::CwdChanged(_) => { self.dirty = true; }
                crate::terminal::TerminalEventKind::ClipboardSet(data) => {
                    if let Some(cb) = &mut self.clipboard {
                        cb.set_text(data);
                    }
                }
                crate::terminal::TerminalEventKind::ProcessExited => {
                    let hook_events = vec![tasty_hooks::HookEvent::ProcessExit];
                    self.state.engine.hook_manager.check_and_fire(surface_id, &hook_events);
                    self.dirty = true;
                }
            }
        }

        // Render
        if self.dirty {
            self.dirty = false;
            match self.gpu.render(&mut self.state, &self.window, &self.preedit_text, self.text_selection.as_ref()) {
                Ok(_) => {}
                Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                    self.gpu.resize(self.window.inner_size());
                }
                Err(wgpu::SurfaceError::OutOfMemory) => {
                    tracing::error!("GPU out of memory");
                    // Can't exit from here — App will handle
                }
                Err(e) => {
                    tracing::warn!("surface error: {e}");
                }
            }
        }

        if self.dirty {
            self.window.request_redraw();
        }
    }
}
