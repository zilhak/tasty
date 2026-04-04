//! Click-to-move-cursor feature.
//!
//! Computes the "editable region" of a terminal surface (the area where the
//! shell prompt / command line is), and moves the cursor to a clicked position
//! within that region by sending arrow key sequences.

use crate::model::Rect;

/// The editable region of a terminal surface — the contiguous area of the
/// current (possibly soft-wrapped) command line, from the first wrapped row
/// to the last row with text content.
#[derive(Debug, Clone)]
pub struct EditableRegion {
    /// First row of the editable region (may be above cursor if soft-wrapped).
    pub start_row: usize,
    /// Last row of the editable region (may be below cursor if cursor is mid-command).
    pub end_row: usize,
    /// Last occupied column on `end_row` (text boundary).
    pub end_col: usize,
    /// Cursor row.
    pub cursor_row: usize,
    /// Cursor column.
    pub cursor_col: usize,
}

/// Get the last occupied column in a terminal line (exclusive).
/// Only counts cells with actual content (non-space, non-empty).
fn last_occupied_col(line: &termwiz::surface::line::Line) -> usize {
    let mut last = 0usize;
    for c in line.visible_cells() {
        let text = c.str();
        if !text.is_empty() && text != " " {
            let ch = text.chars().next().unwrap_or(' ');
            let end = c.cell_index() + crate::renderer::unicode_width(ch);
            if end > last {
                last = end;
            }
        }
    }
    last
}

impl EditableRegion {
    /// Compute the editable region from the current terminal state.
    /// Returns `None` if the terminal is in a state where click-to-move
    /// should be disabled (scrollback, alternate screen, mouse tracking).
    pub fn from_terminal(terminal: &tasty_terminal::Terminal) -> Option<Self> {
        if terminal.scroll_offset > 0 || terminal.is_alternate_screen() {
            return None;
        }
        if terminal.mouse_tracking() != tasty_terminal::MouseTrackingMode::None {
            return None;
        }

        let (cols, rows) = terminal.surface().dimensions();
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
            if last_occupied_col(line) < cols {
                break;
            }
            start_row = prev_row;
        }

        // Walk downward from cursor_row to find the last row of the editable region.
        // If the cursor row fills all columns, the next row is a continuation.
        let mut end_row = cursor_row;
        while end_row + 1 < rows {
            let line = match screen_lines.get(end_row) {
                Some(l) => l,
                None => break,
            };
            if last_occupied_col(line) < cols {
                break; // This row doesn't fill the terminal width — no wrap
            }
            // Next row is a continuation if current row is fully filled
            let next_line = match screen_lines.get(end_row + 1) {
                Some(l) => l,
                None => break,
            };
            // Only include next row if it has content
            if last_occupied_col(next_line) == 0 {
                break;
            }
            end_row += 1;
        }

        // End column: last visible character on end_row
        let end_col = screen_lines
            .get(end_row)
            .map(|l| last_occupied_col(l))
            .unwrap_or(0);

        Some(Self {
            start_row,
            end_row,
            end_col,
            cursor_row,
            cursor_col,
        })
    }

    /// Clamp a grid position to be within this editable region.
    /// Clicks up to 1 row above/below the region are clamped to the boundary.
    /// Clicks further away are rejected (returns None).
    pub fn clamp(&self, row: usize, col: usize) -> Option<(usize, usize)> {
        let margin = 1; // allow 1 row outside

        if row + margin < self.start_row || row > self.end_row + margin {
            return None; // Too far away
        }

        // Clamp row into the region
        let row = row.clamp(self.start_row, self.end_row);

        let col = if row == self.end_row {
            col.min(self.end_col)
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

/// Pending arrow key queue for delayed sending (used for Claude Code surfaces).
#[derive(Debug, Clone)]
pub struct ArrowQueue {
    /// The arrow escape sequence to send.
    pub arrow: &'static [u8],
    /// Remaining count of arrows to send.
    pub remaining: usize,
    /// Surface ID to send to.
    pub surface_id: u32,
}

impl ArrowQueue {
    pub fn new(arrow: &'static [u8], count: usize, surface_id: u32) -> Self {
        Self { arrow, remaining: count, surface_id }
    }

    /// Send one arrow key. Returns true if there are more to send.
    pub fn tick(&mut self, terminal: &mut tasty_terminal::Terminal) -> bool {
        if self.remaining > 0 {
            terminal.send_bytes(self.arrow);
            self.remaining -= 1;
        }
        self.remaining > 0
    }
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
    let rel_x = x - viewport.x;
    let rel_y = y - viewport.y;
    let col = (rel_x / cell_width).floor() as isize;
    let col = col.clamp(0, (cols as isize) - 1) as usize;
    let row = (rel_y / cell_height).floor() as isize;
    let row = row.clamp(0, (rows as isize) - 1) as usize;
    (col, row)
}
