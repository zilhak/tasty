//! Click-to-move-cursor feature.
//!
//! Computes the "editable region" of a terminal surface (the area where the
//! shell prompt / command line is), and moves the cursor to a clicked position
//! within that region by sending arrow key sequences.

use crate::model::Rect;

/// The editable region of a terminal surface — the contiguous area from the
/// start of the current (possibly soft-wrapped) command line to the cursor.
#[derive(Debug, Clone)]
pub struct EditableRegion {
    /// First row of the editable region (may be above cursor if soft-wrapped).
    pub start_row: usize,
    /// Starting column on `start_row`.
    pub start_col: usize,
    /// Cursor row (last row of the editable region).
    pub cursor_row: usize,
    /// Cursor column.
    pub cursor_col: usize,
    /// Terminal column count.
    pub cols: usize,
}

impl EditableRegion {
    /// Compute the editable region from the current terminal state.
    /// Returns `None` if the terminal is in a state where click-to-move
    /// should be disabled (scrollback, alternate screen, mouse tracking).
    pub fn from_terminal(terminal: &tasty_terminal::Terminal) -> Option<Self> {
        // Disabled in scrollback, alternate screen, or mouse tracking modes
        if terminal.scroll_offset > 0 || terminal.is_alternate_screen() {
            return None;
        }
        if terminal.mouse_tracking() != tasty_terminal::MouseTrackingMode::None {
            return None;
        }

        let (cols, _rows) = terminal.surface().dimensions();
        let (cursor_col, cursor_row) = terminal.surface().cursor_position();
        let cursor_row = cursor_row as usize;
        let screen_lines = terminal.surface().screen_lines();

        // Walk upward from cursor_row to find the first row of the editable region.
        // A row is part of the same soft-wrapped line if the row above it fills
        // all terminal columns (no hard line break).
        let mut start_row = cursor_row;
        while start_row > 0 {
            let prev_row = start_row - 1;
            let line = match screen_lines.get(prev_row) {
                Some(l) => l,
                None => break,
            };
            let last_occupied = line
                .visible_cells()
                .map(|c| {
                    let ch = c.str().chars().next().unwrap_or(' ');
                    c.cell_index() + crate::renderer::unicode_width(ch)
                })
                .max()
                .unwrap_or(0);
            if last_occupied < cols {
                break; // Hard line break — previous output, not part of this command
            }
            start_row = prev_row;
        }

        Some(Self {
            start_row,
            start_col: 0, // Soft-wrapped lines start at col 0
            cursor_row,
            cursor_col,
            cols,
        })
    }

    /// Check if a grid position (row, col) is within this editable region.
    pub fn contains(&self, row: usize, col: usize) -> bool {
        if row < self.start_row || row > self.cursor_row {
            return false;
        }
        if row == self.cursor_row && col > self.cursor_col {
            return false;
        }
        true
    }

    /// Clamp a grid position to be within this editable region.
    /// Returns the clamped (row, col), or None if the position is entirely outside.
    pub fn clamp(&self, row: usize, col: usize) -> Option<(usize, usize)> {
        if row < self.start_row || row > self.cursor_row {
            return None;
        }
        let col = if row == self.cursor_row {
            col.min(self.cursor_col)
        } else {
            col
        };
        Some((row, col))
    }
}

/// Count the number of arrow key presses needed to move from one position to
/// another, accounting for wide (2-cell) characters.
pub fn count_arrows(
    terminal: &tasty_terminal::Terminal,
    from_row: usize,
    from_col: usize,
    to_row: usize,
    to_col: usize,
    cols: usize,
) -> usize {
    let screen_lines = terminal.surface().screen_lines();

    let (start_row, start_col, end_row, end_col) = if (to_row, to_col) > (from_row, from_col) {
        (from_row, from_col, to_row, to_col)
    } else {
        (to_row, to_col, from_row, from_col)
    };

    let mut count = 0usize;
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
                count += 1;
            }
        }
    }
    count
}

/// Convert pixel coordinates to grid (col, row) within a terminal viewport.
pub fn pixel_to_grid(
    x: f32,
    y: f32,
    viewport: &Rect,
    cell_width: f32,
    cell_height: f32,
    cols: usize,
    rows: usize,
) -> (usize, usize) {
    let padding = 4.0;
    let rel_x = x - viewport.x - padding;
    let rel_y = y - viewport.y - padding;
    let col = (rel_x / cell_width).floor() as isize;
    let col = col.clamp(0, (cols as isize) - 1) as usize;
    let row = (rel_y / cell_height).floor() as isize;
    let row = row.clamp(0, (rows as isize) - 1) as usize;
    (col, row)
}
