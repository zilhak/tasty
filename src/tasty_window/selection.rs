use crate::model::Rect;
use crate::selection::{self, SelectionMode, SelectionPoint, TextSelection};

use super::TastyWindow;

impl TastyWindow {
    /// Start a new text selection from the given pixel position.
    pub(super) fn start_selection(&mut self, x: f32, y: f32, terminal_rect: &Rect) {
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

    /// Move the terminal cursor to the clicked position using the click_cursor module.
    pub(super) fn move_cursor_to_click(&mut self, x: f32, y: f32, terminal_rect: &Rect) {
        if !self.state.engine.settings.general.click_to_move_cursor {
            return;
        }

        // Commit any in-progress IME composition before moving cursor
        if let Some(preedit) = self.ime_preedit.take() {
            if let Some(terminal) = self.state.focused_terminal_mut() {
                terminal.send_key(&preedit.text);
            }
        }

        let terminal = match self.state.focused_terminal() {
            Some(t) => t,
            None => return,
        };

        let region = match crate::click_cursor::EditableRegion::from_terminal(terminal) {
            Some(r) => r,
            None => return,
        };

        let (cols, rows) = terminal.surface().dimensions();
        // Use the actual content rect (after tab bar) instead of the raw pane rect
        let surface_rect = match self.state.focused_surface_rect(*terminal_rect) {
            Some(r) => r,
            None => return,
        };
        let (click_col, click_row) = crate::click_cursor::pixel_to_grid(
            x, y, &surface_rect,
            self.gpu.cell_width(), self.gpu.cell_height(),
            cols, rows,
        );

        // Clamp to editable region
        let (click_row, click_col) = match region.clamp(click_row, click_col) {
            Some(pos) => pos,
            None => return,
        };

        if click_row == region.cursor_row && click_col == region.cursor_col {
            return;
        }

        let going_right = (click_row, click_col) > (region.cursor_row, region.cursor_col);
        let arrow_count = crate::click_cursor::count_arrows(
            terminal,
            region.cursor_row, region.cursor_col,
            click_row, click_col,
            cols,
        );

        if arrow_count == 0 {
            return;
        }

        // Check if this is a Claude Code surface before mut-borrowing terminal
        let surface_id = self.state.focused_surface_id().unwrap_or(0);
        let is_claude = self.state.engine.claude_parent_children.contains_key(&surface_id)
            || self.state.engine.claude_child_parent.contains_key(&surface_id);

        // Determine arrow escape sequence
        let terminal = self.state.focused_terminal_mut().unwrap();
        let app_cursor = terminal.application_cursor_keys();
        let arrow: &'static [u8] = if going_right {
            if app_cursor { b"\x1bOC" } else { b"\x1b[C" }
        } else {
            if app_cursor { b"\x1bOD" } else { b"\x1b[D" }
        };

        if is_claude {
            // Queue arrows to send one per frame
            terminal.send_bytes(arrow); // Send first one immediately
            if arrow_count > 1 {
                self.arrow_queue = Some(crate::click_cursor::ArrowQueue::new(arrow, arrow_count - 1, surface_id));
                self.window.request_redraw();
            }
        } else {
            // Shell: send all at once
            for _ in 0..arrow_count {
                terminal.send_bytes(arrow);
            }
        }
    }

    /// Convert mouse physical coordinates to a grid SelectionPoint for the focused terminal.
    pub(super) fn mouse_to_grid(&self, x: f32, y: f32, terminal_rect: &Rect) -> Option<(SelectionPoint, u32)> {
        let terminal = self.state.focused_terminal()?;
        let surface_id = self.state.focused_surface_id()?;
        // Use the actual content rect (after tab bar) instead of the raw pane rect
        let surface_rect = self.state.focused_surface_rect(*terminal_rect)?;
        let (cols, rows) = terminal.surface().dimensions();
        let point = selection::pixel_to_grid(
            x, y, &surface_rect,
            self.gpu.cell_width(), self.gpu.cell_height(),
            cols, rows,
            terminal.scroll_offset,
            terminal.scrollback_len(),
        );
        Some((point, surface_id))
    }

    /// Copy the current selection to clipboard. Selection is preserved.
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

        true
    }
}
