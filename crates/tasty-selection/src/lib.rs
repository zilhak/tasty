//! 터미널 선택 좌표·모드·범위와 텍스트 추출. 렌더러와 뷰가 같은 모델을 사용한다.

use tasty_type_geometry::rect::PhysicalRect;

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
    /// Rectangular block selection (vi visual block / Ctrl+v).
    Block,
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
#[allow(clippy::too_many_arguments)] // reason: 마우스→grid 좌표 변환 컨텍스트
pub fn pixel_to_grid(
    mouse_x: f32,
    mouse_y: f32,
    viewport: &PhysicalRect,
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
        SelectionMode::Block => {
            let c0 = sel.start.col.min(sel.end.col);
            let c1 = sel.start.col.max(sel.end.col);
            col >= c0 && col <= c1
        }
        SelectionMode::Normal | SelectionMode::Word => {
            if sel.start.absolute_row == sel.end.absolute_row {
                col >= sel.start.col && col <= sel.end.col
            } else if absolute_row == sel.start.absolute_row {
                col >= sel.start.col
            } else if absolute_row == sel.end.absolute_row {
                col <= sel.end.col
            } else {
                true
            }
        }
    }
}

/// Extract selected text. Soft-wrapped rows are joined without a separator;
/// hard line breaks remain. Rectangular selection keeps visual row boundaries.
pub fn extract_selected_text(
    terminal: &tasty_terminal::Terminal,
    selection: &TextSelection,
) -> String {
    let norm = selection.normalized();
    let scrollback_len = terminal.scrollback_len();
    let (cols, _) = terminal.dimensions();
    let screen_lines = terminal.screen_lines();

    // Block (visual block) — soft-wrap join 무관, 각 row 의 [c0..=c1] 사각형을
    // join("\n") 으로 추출. 본질적으로 visual selection.
    if norm.mode == SelectionMode::Block {
        let c0 = norm.start.col.min(norm.end.col);
        let c1 = norm.start.col.max(norm.end.col);
        let mut lines: Vec<String> = Vec::new();
        for abs_row in norm.start.absolute_row..=norm.end.absolute_row {
            let mut row_text = String::new();
            if abs_row < scrollback_len {
                if let Some(cells) = terminal.scrollback_line_owned(abs_row) {
                    let mut col_idx: usize = 0;
                    for (cell_text, _) in &cells {
                        let ch = cell_text.chars().next().unwrap_or(' ');
                        let width = tasty_cell_width::unicode_width(ch);
                        if col_idx >= c0 && col_idx <= c1 {
                            row_text.push_str(cell_text);
                        }
                        col_idx += width.max(1);
                    }
                }
            } else {
                let screen_row = abs_row - scrollback_len;
                if let Some(line) = screen_lines.get(screen_row) {
                    for cell_ref in line.visible_cells() {
                        let col_idx = cell_ref.cell_index();
                        if col_idx >= c0 && col_idx <= c1 {
                            row_text.push_str(cell_ref.str());
                        }
                    }
                }
            }
            lines.push(row_text);
        }
        return lines.join("\n");
    }

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
        let width = tasty_cell_width::unicode_width(ch);
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
                extra_env: &[],
            },
            waker,
        )
        .expect("terminal creation")
    }

    fn select_all(terminal: &Terminal) -> TextSelection {
        let scrollback_len = terminal.scrollback_len();
        let (cols, rows) = terminal.dimensions();
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
        // Push wrapped rows into scrollback to check that joining still works after capture.
        let mut t = term(10, 3);
        let payload: Vec<u8> = (b'a'..=b'y').collect(); // 25 chars → 3 wrapped rows
        t.process_bytes(&payload);
        // Force enough hard newlines to push every wrapped row into scrollback.
        t.process_bytes(b"\r\nA\r\nB\r\nC\r\nD");

        assert!(t.scrollback_len() >= 3);
        let sel = select_all(&t);
        let text = extract_selected_text(&t, &sel);
        let head: String = (b'a'..=b'y').map(|b| b as char).collect();
        let expected = format!("{head}\nA\nB\nC\nD");
        assert_eq!(text, expected);
    }
}
