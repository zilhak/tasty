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
    let rel_x = mouse_x - viewport.x.value();
    let rel_y = mouse_y - viewport.y.value();

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
pub fn is_selected(col: usize, absolute_row: usize, sel: &NormalizedSelection) -> bool {
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
///
/// Soft-wrapped lines (lines that the terminal auto-wrapped because content
/// reached the right edge) are rejoined into a single logical line on copy:
/// the wrap point is treated as no separator at all, while real `\n` line
/// breaks are preserved. This matches WezTerm/Alacritty behavior for shell
/// prompts that wrap a long command across multiple visual rows.
pub fn extract_selected_text(
    terminal: &tasty_terminal::Terminal,
    selection: &TextSelection,
) -> String {
    let norm = selection.normalized();
    let scrollback_len = terminal.scrollback_len();
    let surface = terminal.surface();
    let (cols, _) = surface.dimensions();
    let screen_lines = surface.screen_lines();

    // Collect (raw_text_without_trim, wrapped) per row in selection.
    let mut rows: Vec<(String, bool)> = Vec::new();
    for abs_row in norm.start.absolute_row..=norm.end.absolute_row {
        let (text, wrapped) = if abs_row < scrollback_len {
            let raw = extract_scrollback_line(terminal, abs_row, &norm, abs_row);
            let wrapped = terminal.scrollback_line_wrapped(abs_row).unwrap_or(false);
            (raw, wrapped)
        } else {
            let screen_row = abs_row - scrollback_len;
            if let Some(line) = screen_lines.get(screen_row) {
                let raw = extract_surface_line(line, &norm, abs_row);
                (raw, screen_line_soft_wrapped(line, cols))
            } else {
                (String::new(), false)
            }
        };
        rows.push((text, wrapped));
    }

    // Join: a wrapped row glues directly to the next; a non-wrapped row gets
    // its trailing whitespace trimmed and a `\n` appended. The final row only
    // gets trim_end (wrap flag irrelevant — there is no next row to join).
    // Whole-screen selections drag in trailing blank rows; strip them so we
    // don't tack a sea of `\n`s onto the clipboard.
    let mut out = String::new();
    let last = rows.len().saturating_sub(1);
    for (i, (text, wrapped)) in rows.iter().enumerate() {
        if i == last {
            out.push_str(text.trim_end());
        } else if *wrapped {
            // Soft wrap: keep raw text (no trim), no separator.
            out.push_str(text);
        } else {
            out.push_str(text.trim_end());
            out.push('\n');
        }
    }
    while out.ends_with('\n') {
        out.pop();
    }
    out
}

/// Same heuristic as `Terminal::line_was_soft_wrapped`, applied to the
/// currently-visible screen surface (where termwiz also fails to set the
/// `last_cell_was_wrapped` bit because `Surface::print_text` skips it).
fn screen_line_soft_wrapped(line: &termwiz::surface::line::Line, cols: usize) -> bool {
    if cols == 0 {
        return false;
    }
    for cell in line.visible_cells() {
        let idx = cell.cell_index();
        let width = cell.width().max(1);
        if idx + width == cols {
            let s = cell.str();
            return !s.is_empty() && s.trim() != "";
        }
    }
    false
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
    text
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
    text
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tasty_terminal::{Terminal, TerminalConfig};

    fn term(cols: usize, rows: usize) -> Terminal {
        let waker: tasty_terminal::Waker = Arc::new(|| {});
        Terminal::new(
            TerminalConfig {
                cols,
                rows,
                shell: None,
                args: &[],
                surface_id: 0,
                working_dir: None,
                initial_input: None,
            },
            waker,
        )
        .expect("terminal creation")
    }

    fn select_all(terminal: &Terminal) -> TextSelection {
        let scrollback_len = terminal.scrollback_len();
        let (cols, rows) = terminal.surface().dimensions();
        TextSelection {
            anchor: SelectionPoint {
                col: 0,
                absolute_row: 0,
            },
            cursor: SelectionPoint {
                col: cols.saturating_sub(1),
                absolute_row: scrollback_len + rows.saturating_sub(1),
            },
            mode: SelectionMode::Normal,
            surface_id: 0,
            dragging: false,
        }
    }

    #[test]
    fn soft_wrapped_screen_lines_are_joined_into_one_line() {
        // 10-col, 4-row terminal. Write 25 chars on a single logical line —
        // termwiz auto-wraps into rows 0..2. Selecting all should produce one
        // contiguous string, not three lines separated by `\n`.
        let mut t = term(10, 4);
        let payload: Vec<u8> = (b'a'..=b'y').collect(); // 25 chars
        t.process_bytes(&payload);

        let sel = select_all(&t);
        let text = extract_selected_text(&t, &sel);
        let expected: String = (b'a'..=b'y').map(|b| b as char).collect();
        assert_eq!(
            text, expected,
            "soft-wrapped lines must be rejoined into a single string"
        );
    }

    #[test]
    fn hard_newline_lines_keep_their_separator() {
        let mut t = term(20, 4);
        t.process_bytes(b"hello\r\nworld");
        let sel = select_all(&t);
        let text = extract_selected_text(&t, &sel);
        assert_eq!(text, "hello\nworld");
    }

    #[test]
    fn soft_wrap_in_scrollback_still_joins() {
        // Wrap a long command, then push the wrapped lines into scrollback by
        // emitting more rows than the screen can hold. The wrap flag must
        // survive scrollback capture so the selection rejoins into one line.
        let mut t = term(10, 3);
        let payload: Vec<u8> = (b'a'..=b'y').collect(); // 25 chars → 3 wrapped rows
        t.process_bytes(&payload);
        // Force enough hard newlines to push every wrapped row into scrollback.
        t.process_bytes(b"\r\nA\r\nB\r\nC\r\nD");

        assert!(t.scrollback_len() >= 3);
        let sel = select_all(&t);
        let text = extract_selected_text(&t, &sel);
        // Expect: the wrapped command rejoined as one line, then the trailing
        // hard-newline-separated rows.
        let head: String = (b'a'..=b'y').map(|b| b as char).collect();
        let expected = format!("{head}\nA\nB\nC\nD");
        assert_eq!(text, expected);
    }
}
