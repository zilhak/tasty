use crate::model::Rect;

/// A point in the terminal grid using absolute row coordinates.
/// absolute_row 0 = oldest scrollback line, scrollback_len = first screen row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectionPoint {
    pub col: usize,
    pub absolute_row: usize,
}

impl SelectionPoint {
    /// Returns true if self comes before other in reading order.
    pub fn before(&self, other: &SelectionPoint) -> bool {
        self.absolute_row < other.absolute_row
            || (self.absolute_row == other.absolute_row && self.col < other.col)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionMode {
    /// Character-level selection (normal drag).
    Normal,
    /// Word-level selection (double-click).
    Word,
    /// Line-level selection (triple-click).
    Line,
}

/// Active text selection state.
#[derive(Debug, Clone)]
pub struct TextSelection {
    /// Drag start point (fixed).
    pub anchor: SelectionPoint,
    /// Current mouse point (moves with drag).
    pub cursor: SelectionPoint,
    pub mode: SelectionMode,
    pub surface_id: u32,
    /// Whether a drag is in progress.
    pub dragging: bool,
}

/// Normalized (start <= end) selection range for rendering/extraction.
pub struct NormalizedSelection {
    pub start: SelectionPoint,
    pub end: SelectionPoint,
    pub mode: SelectionMode,
}

impl TextSelection {
    /// Normalize anchor/cursor so start is always before end.
    pub fn normalized(&self) -> NormalizedSelection {
        if self.anchor.before(&self.cursor) {
            NormalizedSelection {
                start: self.anchor,
                end: self.cursor,
                mode: self.mode,
            }
        } else {
            NormalizedSelection {
                start: self.cursor,
                end: self.anchor,
                mode: self.mode,
            }
        }
    }

    /// Returns true if anchor and cursor point to the same cell.
    pub fn is_empty(&self) -> bool {
        self.anchor == self.cursor
    }
}

/// Convert mouse physical pixel coordinates to a terminal grid SelectionPoint.
pub fn pixel_to_grid(
    mouse_x: f32,
    mouse_y: f32,
    viewport: &Rect,
    cell_width: f32,
    cell_height: f32,
    cols: usize,
    rows: usize,
    scroll_offset: usize,
    scrollback_len: usize,
) -> SelectionPoint {
    let rel_x = mouse_x - viewport.x;
    let rel_y = mouse_y - viewport.y;

    let col = (rel_x / cell_width).floor() as isize;
    let col = col.clamp(0, (cols as isize) - 1) as usize;

    let display_row = (rel_y / cell_height).floor() as isize;
    let display_row = display_row.clamp(0, (rows as isize) - 1) as usize;

    // Convert display row to absolute row:
    // display_row 0 shows: scrollback_len - scroll_offset
    let absolute_row = scrollback_len.saturating_sub(scroll_offset) + display_row;

    SelectionPoint { col, absolute_row }
}

/// Check if a cell at (col, absolute_row) is within the normalized selection range.
pub fn is_selected(
    col: usize,
    absolute_row: usize,
    sel: &NormalizedSelection,
) -> bool {
    if absolute_row < sel.start.absolute_row || absolute_row > sel.end.absolute_row {
        return false;
    }

    match sel.mode {
        SelectionMode::Line => true,
        SelectionMode::Normal | SelectionMode::Word => {
            if sel.start.absolute_row == sel.end.absolute_row {
                // Single row selection
                col >= sel.start.col && col <= sel.end.col
            } else if absolute_row == sel.start.absolute_row {
                // First row: from start.col to end of line
                col >= sel.start.col
            } else if absolute_row == sel.end.absolute_row {
                // Last row: from start of line to end.col
                col <= sel.end.col
            } else {
                // Middle rows: entire line
                true
            }
        }
    }
}

/// Extract selected text from the terminal.
pub fn extract_selected_text(
    terminal: &tasty_terminal::Terminal,
    selection: &TextSelection,
) -> String {
    let norm = selection.normalized();
    let scrollback_len = terminal.scrollback_len();
    let surface = terminal.surface();
    let screen_lines = surface.screen_lines();
    let mut result = Vec::new();

    for abs_row in norm.start.absolute_row..=norm.end.absolute_row {
        let line_text = if abs_row < scrollback_len {
            // Scrollback line
            extract_scrollback_line(terminal, abs_row, &norm, abs_row)
        } else {
            // Screen line
            let screen_row = abs_row - scrollback_len;
            if screen_row < screen_lines.len() {
                extract_surface_line(&screen_lines[screen_row], &norm, abs_row)
            } else {
                String::new()
            }
        };
        result.push(line_text);
    }

    // Join with newline, trim trailing empty lines
    let text = result.join("\n");
    text.trim_end_matches('\n').to_string()
}

fn extract_scrollback_line(
    terminal: &tasty_terminal::Terminal,
    index: usize,
    sel: &NormalizedSelection,
    abs_row: usize,
) -> String {
    let line = match terminal.scrollback_line_owned(index) {
        Some(l) => l,
        None => return String::new(),
    };

    let mut text = String::new();
    let mut col_idx: usize = 0;
    for (cell_text, _attrs) in &line {
        let ch = cell_text.chars().next().unwrap_or(' ');
        let width = crate::renderer::unicode_width(ch);
        let selected = match sel.mode {
            SelectionMode::Line => true,
            _ => is_col_in_range(col_idx, abs_row, sel),
        };
        if selected {
            text.push_str(cell_text);
        }
        col_idx += width;
    }
    text.trim_end().to_string()
}

fn extract_surface_line(
    line: &termwiz::surface::line::Line,
    sel: &NormalizedSelection,
    abs_row: usize,
) -> String {
    let mut text = String::new();
    for cell_ref in line.visible_cells() {
        let col_idx = cell_ref.cell_index();
        let selected = match sel.mode {
            SelectionMode::Line => true,
            _ => is_col_in_range(col_idx, abs_row, sel),
        };
        if selected {
            text.push_str(cell_ref.str());
        }
    }
    text.trim_end().to_string()
}

fn is_col_in_range(col: usize, abs_row: usize, sel: &NormalizedSelection) -> bool {
    if sel.start.absolute_row == sel.end.absolute_row {
        col >= sel.start.col && col <= sel.end.col
    } else if abs_row == sel.start.absolute_row {
        col >= sel.start.col
    } else if abs_row == sel.end.absolute_row {
        col <= sel.end.col
    } else {
        true
    }
}
