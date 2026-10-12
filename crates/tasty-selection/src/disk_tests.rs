//! Copying rows that live in disk scrollback must give the same text as reading each row
//! on its own, in every selection mode and across the disk, memory and screen boundaries.
use super::*;
use tasty_terminal::Terminal;

const ROWS: usize = 5_000;

/// Limit 100 with disk swap: almost all of the 5,000 rows are on disk. Every seventh row
/// overflows the 20 columns and soft-wraps; rows carry wide characters.
fn spilled(surface_id: u32) -> Terminal {
    let mut t = Terminal::new_detached(20, 4);
    t.enable_disk_scrollback(surface_id);
    t.set_scrollback_limit(100);
    for i in 0..ROWS {
        if i.is_multiple_of(7) {
            t.feed_bytes(format!("w{i:04}-abcdefghijklmnopqrstuvwxyz\r\n").as_bytes());
        } else {
            t.feed_bytes(format!("r{i:04} 가나 x  \r\n").as_bytes());
        }
    }
    assert!(t.scrollback_len() > ROWS);
    t
}

fn selection(
    t: &Terminal,
    from: (usize, usize),
    to: (usize, usize),
    mode: SelectionMode,
) -> TextSelection {
    let epoch = t.content_cut().epoch;
    TextSelection {
        anchor: SelectionPoint {
            epoch,
            absolute_row: from.0,
            col: from.1,
        },
        cursor: SelectionPoint {
            epoch,
            absolute_row: to.0,
            col: to.1,
        },
        mode,
        surface_id: 0,
        dragging: false,
    }
}

/// The extraction rules applied to one row read at a time through the per-row API.
fn per_row_text(view: &tasty_terminal::TerminalReadView<'_>, selection: &TextSelection) -> String {
    let norm = selection.normalized();
    let scrollback_len = view.screen_start();
    let (cols, _) = view.dimensions();
    let screen_lines = view.screen_lines();
    let rows = norm.start.absolute_row.max(view.first_row())
        ..=norm.end.absolute_row.min(view.end_row().saturating_sub(1));
    if norm.mode == SelectionMode::Block {
        let c0 = norm.start.col.min(norm.end.col);
        let c1 = norm.start.col.max(norm.end.col);
        let mut lines = Vec::new();
        for abs_row in rows {
            let mut row_text = String::new();
            if abs_row < scrollback_len {
                let mut col = 0;
                for (cell, _) in view.scrollback_line_owned(abs_row).unwrap_or_default() {
                    if col >= c0 && col <= c1 {
                        row_text.push_str(&cell);
                    }
                    col +=
                        tasty_cell_width::unicode_width(cell.chars().next().unwrap_or(' ')).max(1);
                }
            } else if let Some(line) = screen_lines.get(abs_row - scrollback_len) {
                for cell in line.visible_cells() {
                    if cell.cell_index() >= c0 && cell.cell_index() <= c1 {
                        row_text.push_str(cell.str());
                    }
                }
            }
            lines.push(row_text);
        }
        return lines.join("\n");
    }
    let mut out = Vec::new();
    for abs_row in rows {
        if abs_row < scrollback_len {
            let mut text = String::new();
            let mut col = 0;
            for (cell, _) in view.scrollback_line_owned(abs_row).unwrap_or_default() {
                let width = tasty_cell_width::unicode_width(cell.chars().next().unwrap_or(' '));
                if norm.mode == SelectionMode::Line || is_col_in_range(col, abs_row, &norm) {
                    text.push_str(&cell);
                }
                col += width;
            }
            out.push((text, view.scrollback_line_wrapped(abs_row).unwrap_or(false)));
        } else if let Some(line) = screen_lines.get(abs_row - scrollback_len) {
            out.push((
                extract_surface_line(line, &norm, abs_row),
                screen_line_soft_wrapped(line, cols),
            ));
        } else {
            out.push((String::new(), false));
        }
    }
    let mut text = String::new();
    let last = out.len().saturating_sub(1);
    for (i, (row, wrapped)) in out.iter().enumerate() {
        if i == last {
            text.push_str(row.trim_end());
        } else if *wrapped {
            text.push_str(row);
        } else {
            text.push_str(row.trim_end());
            text.push('\n');
        }
    }
    while text.ends_with('\n') {
        text.pop();
    }
    text
}

fn assert_same(t: &Terminal, selection: &TextSelection) -> String {
    t.with_content(|view| {
        let got = extract_selected_text_from_view(&view, selection);
        assert_eq!(got, per_row_text(&view, selection), "{:?}", selection.mode);
        got
    })
}

#[test]
fn copying_a_disk_backed_selection_matches_the_per_row_result() {
    let t = spilled(u32::MAX - 901);
    let text = assert_same(
        &t,
        &selection(&t, (0, 3), (4_000, 6), SelectionMode::Normal),
    );
    assert!(text.starts_with("00-abcdefghijklmnopqrstuvwxyz\nr0001 가나 x\n"));
    assert!(text.contains("w0007-abcdefghijklmnopqrstuvwxyz\nr0008 가나 x\n"));
    assert_same(&t, &selection(&t, (0, 0), (4_000, 19), SelectionMode::Line));
}

#[test]
fn block_selection_over_disk_rows_matches_the_per_row_result() {
    let t = spilled(u32::MAX - 902);
    let text = assert_same(
        &t,
        &selection(&t, (10, 2), (3_000, 8), SelectionMode::Block),
    );
    // Absolute row 10 is r0008: rows 0-1 and 8-9 hold the soft-wrapped w0000 and w0007.
    assert!(
        text.starts_with("008 가나\n009 가나\n"),
        "wide cells keep columns"
    );
    assert!(text.contains("\n014-abc\nqrstuvw\n015 가나\n"));
}

#[test]
fn selections_across_the_disk_memory_and_screen_boundaries_match() {
    let t = spilled(u32::MAX - 903);
    let screen_start = t.content_cut().screen_start;
    let memory_start = screen_start - 100;
    for mode in [
        SelectionMode::Normal,
        SelectionMode::Line,
        SelectionMode::Block,
    ] {
        assert_same(
            &t,
            &selection(&t, (memory_start - 30, 4), (screen_start + 2, 5), mode),
        );
        assert_same(
            &t,
            &selection(&t, (memory_start - 1, 0), (memory_start, 19), mode),
        );
    }
}
