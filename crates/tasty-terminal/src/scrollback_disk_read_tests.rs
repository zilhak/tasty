//! Bulk history reads must open the disk scrollback file a constant number of times,
//! independent of how many rows were spilled to disk.
use crate::{ScrollbackLine, Terminal};
use termwiz::cell::CellAttributes;

const SPILLED: usize = 2_000;
const MAX_OPENS: usize = 2;

fn row(i: usize) -> ScrollbackLine {
    ScrollbackLine::new(
        format!("row {i:05}")
            .chars()
            .map(|c| (c.to_string(), CellAttributes::default()))
            .collect(),
        i.is_multiple_of(7),
    )
}

fn row_text(line: &ScrollbackLine) -> String {
    line.cells().map(|(s, _)| s).collect()
}

/// A detached terminal whose history is mostly on disk: 8 rows stay in memory.
fn spilled_terminal(surface_id: u32) -> Terminal {
    let terminal = Terminal::new_detached(20, 4);
    {
        let mut state = terminal.lock_state();
        state.enable_disk_scrollback(surface_id);
        state.set_scrollback_limit(8);
        state.inject_scrollback((0..SPILLED).map(row).collect());
        assert_eq!(state.scrollback.memory_len(), 8);
        assert_eq!(state.scrollback_len(), SPILLED);
    }
    terminal
}

#[test]
fn reading_all_history_opens_the_disk_file_a_constant_number_of_times() {
    let terminal = spilled_terminal(u32::MAX - 811);
    let state = terminal.lock_state();
    let before = state.scrollback.disk_read_opens();
    let lines = state.scrollback_lines_all();
    let opens = state.scrollback.disk_read_opens() - before;
    assert!(
        opens <= MAX_OPENS,
        "{SPILLED} rows opened the file {opens} times"
    );
    assert_eq!(lines.len(), SPILLED);
    for (i, line) in lines.iter().enumerate() {
        assert_eq!(row_text(line), format!("row {i:05}"));
        assert_eq!(line.wrapped, i.is_multiple_of(7), "row {i} wrap flag");
    }
}

#[test]
fn bulk_history_matches_the_per_row_reads() {
    let terminal = spilled_terminal(u32::MAX - 812);
    let state = terminal.lock_state();
    let bulk = state.scrollback_lines_all();
    for (i, line) in bulk.iter().enumerate() {
        let single = state.scrollback_line_full(i).expect("per-row read");
        assert_eq!(row_text(line), row_text(&single), "row {i}");
        assert_eq!(line.wrapped, single.wrapped, "row {i}");
        assert_eq!(line.to_cells(), single.to_cells(), "row {i}");
    }
}

#[test]
fn history_search_opens_the_disk_file_a_constant_number_of_times() {
    let terminal = spilled_terminal(u32::MAX - 813);
    let state = terminal.lock_state();
    let before = state.scrollback.disk_read_opens();
    let matches = state
        .search("row 00", &crate::search::SearchOptions::default())
        .expect("search");
    let opens = state.scrollback.disk_read_opens() - before;
    assert!(
        opens <= MAX_OPENS,
        "{SPILLED} rows opened the file {opens} times"
    );
    assert_eq!(matches.len(), 1_000, "rows 00000..00999 match");
    assert_eq!(matches.first().map(|m| m.row), Some(0));
    assert_eq!(matches.last().map(|m| m.row), Some(999));
}

#[test]
fn screen_text_history_fill_opens_the_disk_file_a_constant_number_of_times() {
    let terminal = spilled_terminal(u32::MAX - 814);
    let state = terminal.lock_state();
    let before = state.scrollback.disk_read_opens();
    let text = state.screen_text_lines(SPILLED, false);
    let opens = state.scrollback.disk_read_opens() - before;
    assert!(
        opens <= MAX_OPENS,
        "{SPILLED} rows opened the file {opens} times"
    );
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.first(), Some(&"row 00000"));
    assert_eq!(lines.get(SPILLED - 1), Some(&"row 01999"));
}

#[test]
fn serialized_len_matches_the_serialized_history_without_reading_disk() {
    let terminal = spilled_terminal(u32::MAX - 815);
    let mut state = terminal.lock_state();
    let mut colored = CellAttributes::default();
    colored.set_foreground(termwiz::color::ColorAttribute::PaletteIndex(3));
    colored.set_background(
        termwiz::color::ColorAttribute::TrueColorWithDefaultFallback(termwiz::color::SrgbaTuple(
            0.5, 0.25, 1.0, 1.0,
        )),
    );
    state.inject_scrollback(vec![ScrollbackLine::new(
        vec![
            ("가".into(), colored),
            ("x".into(), CellAttributes::default()),
        ],
        true,
    )]);
    let before = state.scrollback.disk_read_opens();
    let estimated = state.scrollback_serialized_len();
    assert_eq!(state.scrollback.disk_read_opens(), before, "no disk read");
    let actual = crate::disk_scrollback::serialize_lines(&state.scrollback_lines_all()).len();
    assert_eq!(estimated, actual as u64);
}
