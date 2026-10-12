//! A hit test over rows that live in disk scrollback must find the same link as over the
//! same rows kept in memory.
use super::*;
use tasty_terminal::Terminal;

const COLS: usize = 24;

fn fill(t: &mut Terminal) {
    for i in 0..300 {
        // Longer than the 24 columns, so every line soft-wraps into a second row.
        t.feed_bytes(format!("r{i:03} see https://example.com/p{i} x\r\n").as_bytes());
    }
    // An OSC 8 link wrapped over three rows near the bottom, in memory and screen rows.
    let mut osc = b"\x1b]8;;https://osc.example/long\x1b\\".to_vec();
    osc.extend(std::iter::repeat_n(b'L', 2 * COLS + 5));
    osc.extend_from_slice(b"\x1b]8;;\x1b\\ end\r\n");
    t.feed_bytes(&osc);
    t.feed_bytes(b"last");
}

fn links(t: &Terminal) -> Vec<Option<(String, Vec<LinkSegment>)>> {
    t.with_content(|view| {
        let mut out = Vec::new();
        for row in 0..view.end_row() {
            for col in 0..COLS {
                out.push(link_at_view(&view, col, row, None, false).map(|s| (s.uri, s.segments)));
            }
        }
        out
    })
}

#[test]
fn links_over_disk_rows_match_links_over_memory_rows() {
    let mut memory = Terminal::new_detached(COLS, 4);
    memory.set_scrollback_limit(100_000);
    fill(&mut memory);
    let mut disk = Terminal::new_detached(COLS, 4);
    disk.enable_disk_scrollback(u32::MAX - 941);
    disk.set_scrollback_limit(20);
    fill(&mut disk);
    assert_eq!(disk.content_cut().end_row(), memory.content_cut().end_row());

    let expected = links(&memory);
    assert_eq!(links(&disk), expected);
    // Both terminals go through the same row reader, so also check history rows against
    // one-row detection through the per-row API. Plain text URLs here never merge rows.
    disk.with_content(|view| {
        for row in 0..view.screen_start() {
            let cells = view.scrollback_line_owned(row).expect("history row");
            let spans = detect_scrollback_line(&cells, row, None, false);
            for col in 0..COLS {
                let want = spans
                    .iter()
                    .find(|s| s.contains(col, row))
                    .map(|s| (s.uri.clone(), s.segments.clone()));
                assert_eq!(expected[row * COLS + col], want, "row {row} col {col}");
            }
        }
    });
    let found = expected.iter().flatten().count();
    assert!(found > 300 * 10, "{found} link cells");
    let chain = expected
        .iter()
        .flatten()
        .find(|(uri, _)| uri == "https://osc.example/long")
        .expect("OSC 8 link");
    assert_eq!(chain.1.len(), 3, "wrapped OSC 8 link spans three rows");
}
