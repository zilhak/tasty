//! The disk line cap must drop the oldest rows like the memory limit does without disk swap:
//! retained rows keep their row IDs, every read returns only retained rows, and the file and
//! the offset table stay bounded by the cap.
use super::Scrollback;
use crate::disk_scrollback::{HEADER_LEN, MIN_COMPACTION_RECORDS, serialize_lines};
use crate::search::SearchOptions;
use crate::{ScrollbackLine, Terminal, TerminalViewport};
use termwiz::cell::CellAttributes;

fn row(i: usize) -> ScrollbackLine {
    ScrollbackLine::new(
        format!("row {i:07}")
            .chars()
            .map(|c| (c.to_string(), CellAttributes::default()))
            .collect(),
        i.is_multiple_of(3),
    )
}

fn text(line: &ScrollbackLine) -> String {
    line.cells().map(|(s, _)| s).collect()
}

fn disk_state(surface_id: u32, memory: usize, cap: Option<usize>) -> Scrollback {
    let mut history = Scrollback::new();
    history.set_limit(memory);
    history.enable_disk(surface_id);
    history.set_disk_limit(cap);
    assert!(history.disk.is_some());
    history
}

fn file_len(history: &Scrollback) -> u64 {
    let disk = history.disk.as_ref().unwrap();
    std::fs::metadata(disk.backing_file()).unwrap().len()
}

/// Every read path returns exactly rows `first..first + count` of the pushed sequence, and
/// the retained coordinate origin names the oldest of them.
fn assert_retained(history: &Scrollback, first: usize, count: usize) {
    assert_eq!(history.total_len(), count);
    assert_eq!(history.first_row, first, "first_row follows dropped rows");
    for i in 0..count {
        let want = row(first + i);
        let full = history.line_full(i).unwrap_or_else(|| panic!("row {i}"));
        assert_eq!(text(&full), text(&want), "line_full {i}");
        assert_eq!(full.wrapped, want.wrapped, "wrap flag {i}");
        assert_eq!(
            history.line_wrapped(i),
            Some(want.wrapped),
            "line_wrapped {i}"
        );
        assert_eq!(
            history.line_owned(i).unwrap(),
            want.to_cells(),
            "line_owned {i}"
        );
    }
    assert!(history.line_full(count).is_none());
    let mut bulk = Vec::new();
    history.for_each_line(0..count, |i, line| bulk.push((i, text(&line))));
    let expected: Vec<_> = (0..count).map(|i| (i, text(&row(first + i)))).collect();
    assert_eq!(bulk, expected, "for_each_line");
    let all: Vec<_> = (0..count).map(|i| row(first + i)).collect();
    assert_eq!(
        history.serialized_len(),
        serialize_lines(&all).len() as u64,
        "serialized length counts retained rows only"
    );
}

/// The file and the offset table hold at most the cap plus one compaction's worth of rows.
fn assert_bounded(history: &Scrollback, cap: usize, max_record: u64) {
    let disk = history.disk.as_ref().unwrap();
    let physical = disk.physical_records();
    let bound = cap + cap.max(MIN_COMPACTION_RECORDS);
    assert!(
        physical <= bound,
        "{physical} records on file, bound {bound}"
    );
    assert!(
        disk.offsets_capacity() <= 2 * bound,
        "offset capacity {} past {}",
        disk.offsets_capacity(),
        2 * bound
    );
    assert!(file_len(history) <= HEADER_LEN + bound as u64 * max_record);
    assert_eq!(file_len(history), disk.file_size());
}

fn record_len() -> u64 {
    crate::disk_scrollback::serialized_record_len(&row(0)) as u64
}

#[test]
fn disk_scrollback_drops_oldest_rows_past_the_disk_cap() {
    let mut history = disk_state(u32::MAX - 841, 10, Some(100));
    for i in 0..1_000 {
        history.push_lines([row(i)]);
    }
    assert_eq!(history.memory_len(), 10);
    assert_eq!(history.disk.as_ref().unwrap().line_count(), 100);
    assert_eq!(
        text(&history.line_full(history.total_len() - 1).unwrap()),
        text(&row(999))
    );
    assert_retained(&history, 1_000 - 110, 110);
}

#[test]
fn one_batch_past_the_cap_never_writes_the_rows_it_drops() {
    let mut history = disk_state(u32::MAX - 842, 10, Some(100));
    history.push_lines((0..300).map(row));
    let epoch = history.epoch;
    // Writing more than the 100 retained records would trip this append budget.
    history
        .disk
        .as_mut()
        .unwrap()
        .fail_append_after(100 * record_len() as usize);
    history.push_lines((300..50_000).map(row));
    assert_eq!(history.epoch, epoch, "no append failed");
    let disk = history.disk.as_ref().unwrap();
    assert_eq!(
        disk.physical_records(),
        100,
        "only retained rows reach the file"
    );
    assert_eq!(file_len(&history), HEADER_LEN + 100 * record_len());
    assert_retained(&history, 50_000 - 110, 110);
}

#[test]
fn file_and_offsets_stay_bounded_under_sustained_output() {
    let cap = 1_000;
    let mut history = disk_state(u32::MAX - 843, 50, Some(cap));
    for start in (0..200_000).step_by(250) {
        history.push_lines((start..start + 250).map(row));
        assert_bounded(&history, cap, record_len());
    }
    assert_retained(&history, 200_000 - cap - 50, cap + 50);
}

#[test]
fn lowering_the_cap_returns_offset_capacity_and_file_space() {
    let mut history = disk_state(u32::MAX - 844, 10, None);
    history.push_lines((0..30_000).map(row));
    let grown = history.disk.as_ref().unwrap().offsets_capacity();
    assert!(grown >= 29_990);
    history.set_disk_limit(Some(10));
    let disk = history.disk.as_ref().unwrap();
    assert_eq!(disk.physical_records(), 10);
    assert!(
        disk.offsets_capacity() < 100,
        "capacity {}",
        disk.offsets_capacity()
    );
    assert_eq!(file_len(&history), HEADER_LEN + 10 * record_len());
    assert_retained(&history, 30_000 - 20, 20);
}

#[test]
fn changing_the_cap_keeps_row_ids_and_unlimited_keeps_every_row() {
    let mut history = disk_state(u32::MAX - 845, 10, Some(100));
    history.push_lines((0..500).map(row));
    assert_retained(&history, 390, 110);
    let epoch = history.epoch;
    history.set_disk_limit(Some(1_000));
    assert_retained(&history, 390, 110);
    history.set_disk_limit(Some(40));
    assert_retained(&history, 450, 50);
    history.set_disk_limit(None);
    history.push_lines((500..20_000).map(row));
    assert_retained(&history, 450, 20_000 - 450);
    assert_eq!(
        history.epoch, epoch,
        "dropping rows keeps the content timeline"
    );
    // Shrinking the memory limit spills into the cap like any other push.
    history.set_disk_limit(Some(30));
    history.set_limit(5);
    assert_retained(&history, 20_000 - 35, 35);
}

#[test]
fn zero_cap_keeps_only_memory_rows() {
    let mut history = disk_state(u32::MAX - 846, 10, Some(0));
    history.push_lines((0..100).map(row));
    assert_eq!(history.disk.as_ref().unwrap().physical_records(), 0);
    assert_eq!(file_len(&history), HEADER_LEN);
    assert_retained(&history, 90, 10);
}

#[test]
fn erasing_scrollback_after_drops_starts_a_new_timeline() {
    let mut history = disk_state(u32::MAX - 847, 10, Some(100));
    history.push_lines((0..5_000).map(row));
    let epoch = history.epoch;
    history.clear();
    assert_ne!(history.epoch, epoch);
    assert_eq!(history.total_len(), 0);
    assert_eq!(file_len(&history), HEADER_LEN);
    history.push_lines((0..200).map(row));
    assert_retained(&history, 90, 110);
}

#[test]
fn a_failed_compaction_keeps_reading_the_retained_rows() {
    let mut history = disk_state(u32::MAX - 848, 10, Some(100));
    history.disk.as_mut().unwrap().fail_compaction(true);
    history.push_lines((0..300).map(row));
    for start in (300..10_000).step_by(100) {
        history.push_lines((start..start + 100).map(row));
    }
    let disk = history.disk.as_ref().unwrap();
    assert!(disk.physical_records() > 100 + MIN_COMPACTION_RECORDS);
    let stuck = file_len(&history);
    assert_retained(&history, 10_000 - 110, 110);
    history.disk.as_mut().unwrap().fail_compaction(false);
    history.push_lines((10_000..10_100).map(row));
    assert_eq!(history.disk.as_ref().unwrap().physical_records(), 100);
    assert!(file_len(&history) < stuck);
    assert_retained(&history, 10_100 - 110, 110);
}

#[test]
fn a_failed_append_after_dropping_rows_starts_a_new_timeline() {
    let mut history = disk_state(u32::MAX - 849, 10, Some(100));
    history.push_lines((0..500).map(row));
    let epoch = history.epoch;
    history.disk.as_mut().unwrap().fail_append_after(17);
    history.push_lines((500..1_000).map(row));
    assert_ne!(history.epoch, epoch);
    assert_eq!(history.first_row, 0);
    assert_eq!(
        file_len(&history),
        history.disk.as_ref().unwrap().file_size()
    );
    history.push_lines((1_000..1_200).map(row));
    let total = history.total_len();
    assert!(total <= 110);
    assert_eq!(
        text(&history.line_full(total - 1).unwrap()),
        text(&row(1_199))
    );
}

#[test]
fn search_anchor_and_row_reads_agree_after_disk_drops() {
    let mut terminal = Terminal::new_detached(20, 3);
    {
        let mut state = terminal.lock_state();
        state.enable_disk_scrollback(u32::MAX - 850);
        state.set_scrollback_limit(5);
        state.set_disk_scrollback_limit(Some(20));
    }
    let mut out = Vec::new();
    for i in 0..100 {
        out.extend_from_slice(format!("line{i:03}\r\n").as_bytes());
    }
    terminal.feed_bytes(&out);
    let mut reader = TerminalViewport::default();
    let cut = terminal.content_cut();
    reader.scroll_up(cut, cut.screen_start - cut.first_row);
    let (first, oldest) = terminal.with_view(&reader, |view| {
        let top = view.viewport().top_row;
        assert_eq!(top, view.first_row());
        (
            view.first_row(),
            text(&view.scrollback_line_full(top).unwrap())
                .trim_end()
                .to_string(),
        )
    });
    assert_eq!(oldest, format!("line{:03}", first));
    terminal.with_view(&TerminalViewport::LIVE, |view| {
        let options = SearchOptions::default();
        assert!(view.search("line000", &options).unwrap().is_empty());
        let found = view.search(&oldest, &options).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].row, first);
        assert!(view.scrollback_line_full(first - 1).is_none());
    });
    // More output drops the anchored row; the anchor clamps to the new oldest row.
    terminal.feed_bytes(b"more\r\nmore\r\nmore\r\n");
    terminal.with_view(&reader, |view| {
        assert_eq!(view.first_row(), first + 3);
        assert_eq!(view.viewport().top_row, first + 3);
        let top = text(&view.scrollback_line_full(first + 3).unwrap());
        assert_eq!(top.trim_end(), format!("line{:03}", first + 3));
    });
}
