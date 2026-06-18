//! Resize 처리 — grid 크기 변경(`TerminalState`)과 PTY 크기 알림 throttle
//! (`Terminal` 핸들)로 분리 (ADR-0002).

use portable_pty::PtySize;
use termwiz::cell::CellAttributes;
use termwiz::surface::Change;

use crate::{Terminal, TerminalState};

impl TerminalState {
    /// Resize the grid (surfaces, scroll region, line tails). Returns true if the
    /// dimensions actually changed (the caller then schedules a PTY notify).
    pub(crate) fn resize_grid(&mut self, cols: usize, rows: usize) -> bool {
        if self.cols == cols && self.rows == rows {
            return false;
        }

        let old_cols = self.cols;
        let old_rows = self.rows;

        // Save/restore line tails on the primary surface when cols change
        if cols != old_cols && !self.use_alternate {
            self.save_or_restore_line_tails(old_cols, cols, rows);
        }

        // Handle rows shrink BEFORE resize (need to capture lines before they're lost)
        if rows < old_rows && !self.use_alternate {
            self.handle_rows_shrink(rows, old_rows);
        }

        // Save cursor position before resize for grow restoration
        let old_cursor = self.primary_surface.cursor_position();

        self.cols = cols;
        self.rows = rows;
        // Tab stops are column-indexed; rebuild the default grid when the width
        // changes (custom HTS/TBC stops are reset on resize, matching xterm's
        // default-on-resize behaviour).
        if cols != old_cols {
            self.tab_stops = crate::default_tab_stops(cols);
        }
        self.primary_surface.resize(cols, rows);
        if let Some(alt) = &mut self.alternate_surface {
            alt.resize(cols, rows);
        }

        // Handle rows grow AFTER resize (surface now has room for ScrollRegionDown)
        let mut rows_restored = 0usize;
        if rows > old_rows && !self.use_alternate {
            rows_restored = self.handle_rows_grow(rows, old_rows);
        }

        // Restore saved tails onto the surface after resize expanded cols
        if cols > old_cols && !self.use_alternate {
            self.restore_tails_to_surface(old_cols, cols);
        }

        // Always restore cursor position after all resize operations.
        if !self.use_alternate {
            use termwiz::surface::Position;
            let cursor_y = (old_cursor.1 + rows_restored).min(rows.saturating_sub(1));
            let cursor_x = old_cursor.0.min(cols.saturating_sub(1));
            self.primary_surface.add_change(Change::CursorPosition {
                x: Position::Absolute(cursor_x),
                y: Position::Absolute(cursor_y),
            });
            // The grow/tail restore paths emit `AllAttributes` per restored cell
            // directly on the surface, leaving its pen at the last cell's attrs —
            // a restoration artifact that bypasses `mirror_pen`. Re-apply the
            // logical pen so the surface pen and `current_pen` stay aligned and a
            // subsequent plain `Text` (or Overline/UnderlineColor/VerticalAlign
            // SGR) is not painted with the leftover attributes.
            self.primary_surface
                .add_change(Change::AllAttributes(self.current_pen.clone()));
        }

        // Reset scroll region on resize
        self.scroll_region = None;
        true
    }

    /// When rows shrink, capture top lines to scrollback so the cursor stays
    /// near the bottom, mimicking xterm/Alacritty behavior.
    pub(crate) fn handle_rows_shrink(&mut self, new_rows: usize, old_rows: usize) {
        let (_, cursor_y) = self.primary_surface.cursor_position();
        let rows_to_remove = old_rows - new_rows;

        // Count blank lines below the cursor
        let lines = self.primary_surface.screen_lines();
        let mut blank_below = 0;
        for i in ((cursor_y + 1)..old_rows).rev() {
            if i < lines.len() && Self::is_line_blank(&lines[i]) {
                blank_below += 1;
            } else {
                break;
            }
        }

        // How many top lines need to be pushed to scrollback
        let lines_to_scroll = rows_to_remove.saturating_sub(blank_below);
        if lines_to_scroll > 0 {
            // Capture top lines to scrollback
            let captured = self.capture_top_lines(lines_to_scroll);
            let count = captured.len();
            for line in captured {
                self.scrollback.push_line(line);
            }
            // These lines are owed back to a symmetric grow.
            self.restorable_scrollback_count += count;
            // Shift saved_line_tails
            for _ in 0..count.min(self.saved_line_tails.len()) {
                self.saved_line_tails.remove(0);
            }

            // Scroll the surface up to remove the captured lines
            self.primary_surface.add_change(Change::ScrollRegionUp {
                first_row: 0,
                region_size: old_rows,
                scroll_count: count,
            });
        }
    }

    /// When rows grow, restore lines from scrollback to the top of the screen.
    /// Called AFTER primary_surface.resize() so the surface already has room.
    /// Returns the number of lines restored (for cursor offset calculation).
    pub(crate) fn handle_rows_grow(&mut self, new_rows: usize, old_rows: usize) -> usize {
        use termwiz::surface::Position;

        let rows_added = new_rows - old_rows;
        // Only restore lines that were pushed by a prior shrink.
        let restore_count = rows_added
            .min(self.scrollback.memory_len())
            .min(self.restorable_scrollback_count);

        if restore_count == 0 {
            return 0;
        }

        // Pop from scrollback (most recent first = back of deque)
        let mut to_restore: Vec<crate::scrollback::ScrollbackLine> = Vec::new();
        for _ in 0..restore_count {
            if let Some(line) = self.scrollback.pop_back() {
                to_restore.push(line);
            }
        }
        let actual_restored = to_restore.len();
        self.restorable_scrollback_count -= actual_restored;
        to_restore.reverse(); // oldest first

        // Surface is already resized to new_rows.
        // Scroll current content down to make room at top.
        self.primary_surface.add_change(Change::ScrollRegionDown {
            first_row: 0,
            region_size: new_rows,
            scroll_count: actual_restored,
        });

        // Write restored lines at the top.
        for (row, line) in to_restore.iter().enumerate() {
            self.primary_surface.add_change(Change::CursorPosition {
                x: Position::Absolute(0),
                y: Position::Absolute(row),
            });
            for (text, attrs) in line.cells() {
                self.primary_surface
                    .add_change(Change::AllAttributes(attrs.clone()));
                self.primary_surface
                    .add_change(Change::Text(text.to_string()));
            }
        }

        // Shift saved_line_tails to match shifted content positions
        if !self.saved_line_tails.is_empty() {
            let mut shifted = vec![Vec::new(); actual_restored];
            shifted.append(&mut self.saved_line_tails);
            self.saved_line_tails = shifted;
        }

        actual_restored
    }

    /// Check if a line is visually blank (all spaces or empty).
    pub(crate) fn is_line_blank(line: &termwiz::surface::line::Line) -> bool {
        for cell in line.visible_cells() {
            let s = cell.str();
            if !s.is_empty() && s.trim() != "" {
                return false;
            }
        }
        true
    }

    /// Heuristic: returns true when the line's rightmost cell is occupied by a
    /// non-space grapheme — the signature of a soft-wrap.
    pub(crate) fn line_was_soft_wrapped(line: &termwiz::surface::line::Line, cols: usize) -> bool {
        if cols == 0 {
            return false;
        }
        for cell in line.visible_cells() {
            let idx = cell.cell_index();
            let width = cell.width().max(1);
            // A cell that occupies the rightmost column has its right edge at `cols`.
            if idx + width == cols {
                let s = cell.str();
                return !s.is_empty() && s.trim() != "";
            }
        }
        false
    }

    /// Before termwiz truncates lines, capture cells that would be lost (cols
    /// shrinking) or merge saved tails back when cols grow.
    pub(crate) fn save_or_restore_line_tails(
        &mut self,
        old_cols: usize,
        new_cols: usize,
        new_rows: usize,
    ) {
        let lines = self.primary_surface.screen_lines();
        let line_count = lines.len();

        // Ensure saved_line_tails has enough entries
        if self.saved_line_tails.len() < line_count {
            self.saved_line_tails.resize(line_count, Vec::new());
        }

        if new_cols < old_cols {
            // Cols shrinking: capture cells at indices [new_cols..] before termwiz truncates
            for (i, line) in lines.iter().enumerate() {
                let mut tail_cells: Vec<(String, CellAttributes)> = line
                    .visible_cells()
                    .filter(|cell| cell.cell_index() >= new_cols)
                    .map(|cell| (cell.str().to_string(), cell.attrs().clone()))
                    .collect();
                // Prepend to any previously saved tail for this line
                if !self.saved_line_tails[i].is_empty() {
                    tail_cells.append(&mut self.saved_line_tails[i]);
                }
                self.saved_line_tails[i] = tail_cells;
            }
        }

        // Trim saved_line_tails to match new row count
        self.saved_line_tails.truncate(new_rows);
    }

    /// After termwiz Surface::resize expanded cols, write back saved tail cells.
    pub(crate) fn restore_tails_to_surface(&mut self, old_cols: usize, new_cols: usize) {
        use termwiz::surface::Position;

        let restore_count = new_cols - old_cols;

        for (row, tail) in self.saved_line_tails.iter_mut().enumerate() {
            if tail.is_empty() {
                continue;
            }
            let cells_to_restore = restore_count.min(tail.len());
            let restored: Vec<(String, CellAttributes)> = tail.drain(..cells_to_restore).collect();

            // Position cursor at (old_cols, row) and write each cell
            self.primary_surface.add_change(Change::CursorPosition {
                x: Position::Absolute(old_cols),
                y: Position::Absolute(row),
            });
            for (text, attrs) in restored {
                self.primary_surface
                    .add_change(Change::AllAttributes(attrs));
                self.primary_surface.add_change(Change::Text(text));
            }
        }
    }
}

impl Terminal {
    /// Throttle interval for PTY resize notifications.
    const PTY_RESIZE_THROTTLE: std::time::Duration = std::time::Duration::from_millis(100);

    /// Resize the terminal grid and (for PTY-backed terminals) schedule a
    /// throttled SIGWINCH notification. Lock-free no-op when the dimensions are
    /// unchanged — the per-frame `resize_all` sweep calls this on every terminal,
    /// so the common case must not lock a busy background terminal's state.
    pub fn resize(&mut self, cols: usize, rows: usize) {
        if self.cached_dims == (cols, rows) {
            return;
        }
        let changed = self.lock_state().resize_grid(cols, rows);
        self.cached_dims = (cols, rows);
        // Defer PTY resize notification to avoid SIGWINCH storms during drag.
        if changed && self.pty.is_some() {
            self.pending_pty_resize = Some((cols, rows));
        }
    }

    /// Try to flush pending PTY resize. Returns true if flushed/cleared, false if
    /// throttled (the pending resize is kept and the caller should retry later).
    pub fn flush_pty_resize(&mut self) -> bool {
        if self.pending_pty_resize.is_none() {
            return false;
        }

        if self.last_pty_flush.elapsed() < Self::PTY_RESIZE_THROTTLE {
            return false; // throttled — caller should retry later
        }

        if let Some((cols, rows)) = self.pending_pty_resize.take() {
            if let Some(pty) = self.pty.as_ref()
                && let Err(e) = pty.pty_master.resize(PtySize {
                    rows: rows as u16,
                    cols: cols as u16,
                    pixel_width: 0,
                    pixel_height: 0,
                })
            {
                tracing::warn!("PTY resize failed: {e}");
            }
            self.last_pty_flush = std::time::Instant::now();
        }
        true
    }

    /// Force flush pending PTY resize regardless of throttle.
    pub fn force_flush_pty_resize(&mut self) {
        if let Some((cols, rows)) = self.pending_pty_resize.take() {
            if let Some(pty) = self.pty.as_ref()
                && let Err(e) = pty.pty_master.resize(PtySize {
                    rows: rows as u16,
                    cols: cols as u16,
                    pixel_width: 0,
                    pixel_height: 0,
                })
            {
                tracing::warn!("PTY resize failed: {e}");
            }
            self.last_pty_flush = std::time::Instant::now();
        }
    }

    /// Check if there is a pending PTY resize.
    pub fn has_pending_pty_resize(&self) -> bool {
        self.pending_pty_resize.is_some()
    }
}
