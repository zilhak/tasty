//! Spilling rows to disk must write each multi-row push as one batch through the held
//! handle, keep row order and wrap flags, and leave the disk counters and file in step
//! when a write fails.
use super::Scrollback;
use crate::{ScrollbackLine, Terminal};
use termwiz::cell::CellAttributes;

fn row(i: usize) -> ScrollbackLine {
    let mut cells: Vec<(String, CellAttributes)> = format!("row {i:05} ")
        .chars()
        .map(|c| (c.to_string(), CellAttributes::default()))
        .collect();
    if i.is_multiple_of(5) {
        let mut bold = CellAttributes::default();
        bold.set_intensity(termwiz::cell::Intensity::Bold);
        cells.push(("가".into(), bold));
    }
    ScrollbackLine::new(cells, i.is_multiple_of(2))
}

fn assert_rows(history: &Scrollback, expected: &[ScrollbackLine]) {
    assert_eq!(history.total_len(), expected.len());
    for (i, want) in expected.iter().enumerate() {
        let got = history
            .line_full(i)
            .unwrap_or_else(|| panic!("row {i} missing"));
        assert_eq!(got.to_cells(), want.to_cells(), "row {i}");
        assert_eq!(got.wrapped, want.wrapped, "row {i} wrap flag");
    }
    let mut bulk = Vec::new();
    history.for_each_line(0..expected.len(), |i, line| {
        bulk.push((i, line.into_owned()))
    });
    assert_eq!(bulk.len(), expected.len());
    for (i, line) in bulk {
        assert_eq!(line.to_cells(), expected[i].to_cells(), "bulk row {i}");
        assert_eq!(line.wrapped, expected[i].wrapped, "bulk row {i} wrap flag");
    }
}

fn disk_history(surface_id: u32, limit: usize) -> Scrollback {
    let mut history = Scrollback::new();
    history.set_limit(limit);
    history.enable_disk(surface_id);
    assert!(history.disk.is_some());
    history
}

fn file_len(history: &Scrollback) -> u64 {
    let disk = history.disk.as_ref().unwrap();
    std::fs::metadata(disk.backing_file()).unwrap().len()
}

#[test]
fn injecting_a_large_history_writes_the_overflow_in_one_batch() {
    let terminal = Terminal::new_detached(20, 4);
    let lines: Vec<_> = (0..5_000).map(row).collect();
    let mut state = terminal.lock_state();
    state.enable_disk_scrollback(u32::MAX - 821);
    state.set_scrollback_limit(100);
    state.inject_scrollback(lines.clone());
    assert_eq!(state.scrollback.disk_push_batches(), 1);
    assert_eq!(state.scrollback.memory_len(), 100);
    assert_rows(&state.scrollback, &lines);
}

#[test]
fn per_row_pushes_and_one_batch_store_the_same_rows() {
    let lines: Vec<_> = (0..3_000).map(row).collect();
    let mut per_row = disk_history(u32::MAX - 822, 100);
    for line in lines.clone() {
        per_row.push_lines([line]);
    }
    let mut batched = disk_history(u32::MAX - 823, 100);
    batched.push_lines(lines.clone());
    assert_eq!(per_row.disk_push_batches(), 2_900);
    assert_eq!(batched.disk_push_batches(), 1);
    assert_rows(&per_row, &lines);
    assert_rows(&batched, &lines);
    assert_eq!(file_len(&per_row), file_len(&batched));
    assert_eq!(per_row.serialized_len(), batched.serialized_len());
}

#[test]
fn shrinking_the_limit_spills_the_excess_in_one_batch() {
    let lines: Vec<_> = (0..1_000).map(row).collect();
    let mut history = disk_history(u32::MAX - 824, 1_000);
    history.push_lines(lines.clone());
    assert_eq!(history.disk_push_batches(), 0);
    history.set_limit(10);
    assert_eq!(history.disk_push_batches(), 1);
    assert_eq!(history.memory_len(), 10);
    assert_rows(&history, &lines);
}

#[test]
fn rows_scrolled_off_together_reach_disk_in_one_batch() {
    let terminal = Terminal::new_detached(20, 4);
    let lines: Vec<_> = (0..500).map(row).collect();
    let mut state = terminal.lock_state();
    state.enable_disk_scrollback(u32::MAX - 825);
    state.set_scrollback_limit(50);
    state.push_scrolled_off(lines.clone());
    assert_eq!(state.scrollback.disk_push_batches(), 1);
    assert_rows(&state.scrollback, &lines);
}

#[test]
fn a_failed_batch_leaves_disk_counters_and_file_in_step() {
    let lines: Vec<_> = (0..300).map(row).collect();
    let mut history = disk_history(u32::MAX - 826, 10);
    history.push_lines(lines[..100].to_vec());
    let kept_disk = history.disk.as_ref().unwrap().line_count();
    let kept_size = history.disk.as_ref().unwrap().file_size();
    assert_eq!(kept_disk, 90);
    assert_eq!(file_len(&history), kept_size);
    let epoch = history.epoch;

    // Fail in the middle of a record of the next batch.
    history.disk.as_mut().unwrap().fail_append_after(37);
    history.push_lines(lines[100..200].to_vec());
    let disk = history.disk.as_ref().unwrap();
    assert_eq!(disk.line_count(), kept_disk);
    assert_eq!(disk.file_size(), kept_size);
    assert_eq!(file_len(&history), kept_size, "partial record cut off");
    assert_ne!(history.epoch, epoch, "lost rows start a new timeline");
    assert_eq!(history.first_row, 0);
    assert_eq!(history.memory_len(), 10);

    // The next batch lands right after the rows kept before the failure.
    history.push_lines(lines[200..].to_vec());
    let mut expected = lines[..kept_disk].to_vec();
    expected.extend_from_slice(&lines[190..]);
    assert_rows(&history, &expected);
    assert_eq!(
        file_len(&history),
        history.disk.as_ref().unwrap().file_size()
    );
}

#[test]
fn a_failure_after_an_earlier_chunk_rolls_back_the_whole_batch() {
    // Rows big enough that one batch needs several chunked writes.
    let wide = |i: usize| {
        ScrollbackLine::new(
            vec![(format!("{i:0>4096}"), CellAttributes::default())],
            false,
        )
    };
    let rows = 2 * crate::disk_scrollback::WRITE_CHUNK / 4096 + 8;
    let lines: Vec<_> = (0..rows).map(wide).collect();
    let mut history = disk_history(u32::MAX - 827, 0);
    history.push_lines([wide(usize::MAX % 10_000)]);
    let kept_size = history.disk.as_ref().unwrap().file_size();
    history
        .disk
        .as_mut()
        .unwrap()
        .fail_append_after(crate::disk_scrollback::WRITE_CHUNK + 100);
    history.push_lines(lines.clone());
    assert_eq!(history.disk.as_ref().unwrap().line_count(), 1);
    assert_eq!(history.disk.as_ref().unwrap().file_size(), kept_size);
    assert_eq!(file_len(&history), kept_size);
    history.push_lines(lines.clone());
    let mut expected = vec![wide(usize::MAX % 10_000)];
    expected.extend(lines);
    assert_rows(&history, &expected);
}

#[test]
fn clearing_keeps_the_header_and_later_rows_start_after_it() {
    let lines: Vec<_> = (0..200).map(row).collect();
    let mut history = disk_history(u32::MAX - 828, 10);
    history.push_lines(lines.clone());
    history.clear();
    assert_eq!(history.total_len(), 0);
    assert_eq!(file_len(&history), crate::disk_scrollback::HEADER_LEN);
    history.push_lines(lines.clone());
    assert_rows(&history, &lines);
    let bytes = std::fs::read(history.disk.as_ref().unwrap().backing_file()).unwrap();
    let mut on_disk = crate::disk_scrollback::deserialize_lines(&bytes).expect("valid file");
    assert_eq!(on_disk.len(), 190);
    on_disk.truncate(3);
    for (i, line) in on_disk.iter().enumerate() {
        assert_eq!(line.to_cells(), lines[i].to_cells());
    }
}

#[test]
fn dropping_the_owner_closes_and_removes_the_backing_file() {
    let mut history = disk_history(u32::MAX - 829, 0);
    history.push_lines((0..20).map(row));
    let path = history.disk.as_ref().unwrap().backing_file().to_path_buf();
    assert!(path.is_file());
    drop(history);
    assert!(!path.exists());
}
