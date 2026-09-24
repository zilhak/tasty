//! 명령줄의 편집 영역을 추정하고 클릭한 위치까지 방향키를 보낸다.

use crate::model::PhysicalRect;

/// 현재 명령줄로 추정한 연속 영역. 화면 폭을 채운 행을 줄바꿈된 명령의 일부로 본다.
#[derive(Debug, Clone)]
pub struct EditableRegion {
    /// First row of the editable region (may be above cursor if soft-wrapped).
    pub start_row: usize,
    /// Last row of the editable region (may be below cursor if cursor is mid-command).
    pub end_row: usize,
    /// Last occupied column on `end_row` (text boundary).
    pub end_col: usize,
    pub cursor_row: usize,
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
            let end = c.cell_index() + tasty_cell_width::unicode_width(ch);
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
        if terminal.scroll_offset() > 0 || terminal.is_alternate_screen() {
            return None;
        }
        if terminal.mouse_tracking() != tasty_terminal::MouseTrackingMode::None {
            return None;
        }

        // 파서 갱신 사이에 커서와 그리드를 따로 읽지 않도록 같은 잠금에서 가져온다(ADR-0013).
        let (cols, rows, cursor_col, cursor_row, screen_lines) = terminal.with_surface(|s| {
            let (cols, rows) = s.dimensions();
            let (cursor_col, cursor_row) = s.cursor_position();
            let screen_lines: Vec<_> = s
                .screen_lines()
                .into_iter()
                .map(|c| c.into_owned())
                .collect();
            (cols, rows, cursor_col, cursor_row, screen_lines)
        });

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

        let mut end_row = cursor_row;
        while end_row + 1 < rows {
            let line = match screen_lines.get(end_row) {
                Some(l) => l,
                None => break,
            };
            if last_occupied_col(line) < cols {
                break; // This row doesn't fill the terminal width — no wrap
            }
            let next_line = match screen_lines.get(end_row + 1) {
                Some(l) => l,
                None => break,
            };
            if last_occupied_col(next_line) == 0 {
                break;
            }
            end_row += 1;
        }

        let end_col = screen_lines
            .get(end_row)
            .map(last_occupied_col)
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
    let screen_lines = terminal.screen_lines();

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

/// Check if a process name is a known shell.
pub fn is_shell_process(name: &str) -> bool {
    matches!(
        name,
        "zsh" | "bash" | "fish" | "sh" | "dash" | "ksh" | "tcsh" | "csh" | "pwsh" | "powershell"
    )
}

/// Convert pixel coordinates to grid (col, row) within a terminal viewport.
pub fn pixel_to_grid(
    x: f32,
    y: f32,
    viewport: &PhysicalRect,
    cell_width: f32,
    cell_height: f32,
    cols: usize,
    rows: usize,
) -> (usize, usize) {
    let rel_x = x - viewport.x.value();
    let rel_y = y - viewport.y.value();
    let col = (rel_x / cell_width).floor() as isize;
    let col = col.clamp(0, (cols as isize) - 1) as usize;
    let row = (rel_y / cell_height).floor() as isize;
    let row = row.clamp(0, (rows as isize) - 1) as usize;
    (col, row)
}
