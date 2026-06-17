//! `Terminal` 단위 테스트.

use super::*;
use std::sync::Arc;
use termwiz::escape::csi::CSI;
use termwiz::escape::parser::Parser;

fn noop_waker() -> Waker {
    Arc::new(|| {})
}

fn test_terminal(cols: usize, rows: usize) -> Terminal {
    let waker = noop_waker();
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

// ---- DECSET/DECRST mode toggling tests ----

#[test]
fn decset_application_cursor_keys() {
    let mut terminal = test_terminal(80, 24);
    assert!(!terminal.application_cursor_keys());

    let mut parser = Parser::new();
    let actions = parser.parse_as_vec(b"\x1b[?1h");
    for action in actions {
        if let Action::CSI(CSI::Mode(ref mode)) = action {
            terminal.lock_state().handle_mode(mode);
        }
    }
    assert!(terminal.application_cursor_keys());

    let actions = parser.parse_as_vec(b"\x1b[?1l");
    for action in actions {
        if let Action::CSI(CSI::Mode(ref mode)) = action {
            terminal.lock_state().handle_mode(mode);
        }
    }
    assert!(!terminal.application_cursor_keys());
}

#[test]
fn decset_cursor_visibility() {
    let mut terminal = test_terminal(80, 24);
    assert!(terminal.cursor_visible());

    let mut parser = Parser::new();
    let actions = parser.parse_as_vec(b"\x1b[?25l");
    for action in actions {
        if let Action::CSI(CSI::Mode(ref mode)) = action {
            terminal.lock_state().handle_mode(mode);
        }
    }
    assert!(!terminal.cursor_visible());

    let actions = parser.parse_as_vec(b"\x1b[?25h");
    for action in actions {
        if let Action::CSI(CSI::Mode(ref mode)) = action {
            terminal.lock_state().handle_mode(mode);
        }
    }
    assert!(terminal.cursor_visible());
}

#[test]
fn decset_bracketed_paste() {
    let mut terminal = test_terminal(80, 24);
    assert!(!terminal.bracketed_paste());

    let mut parser = Parser::new();
    let actions = parser.parse_as_vec(b"\x1b[?2004h");
    for action in actions {
        if let Action::CSI(CSI::Mode(ref mode)) = action {
            terminal.lock_state().handle_mode(mode);
        }
    }
    assert!(terminal.bracketed_paste());

    let actions = parser.parse_as_vec(b"\x1b[?2004l");
    for action in actions {
        if let Action::CSI(CSI::Mode(ref mode)) = action {
            terminal.lock_state().handle_mode(mode);
        }
    }
    assert!(!terminal.bracketed_paste());
}

#[test]
fn decset_mouse_tracking() {
    let mut terminal = test_terminal(80, 24);
    assert_eq!(terminal.mouse_tracking(), MouseTrackingMode::None);

    let mut parser = Parser::new();
    let actions = parser.parse_as_vec(b"\x1b[?1000h");
    for action in actions {
        if let Action::CSI(CSI::Mode(ref mode)) = action {
            terminal.lock_state().handle_mode(mode);
        }
    }
    assert_eq!(terminal.mouse_tracking(), MouseTrackingMode::Click);

    let actions = parser.parse_as_vec(b"\x1b[?1003h");
    for action in actions {
        if let Action::CSI(CSI::Mode(ref mode)) = action {
            terminal.lock_state().handle_mode(mode);
        }
    }
    assert_eq!(terminal.mouse_tracking(), MouseTrackingMode::AllMotion);

    let actions = parser.parse_as_vec(b"\x1b[?1003l");
    for action in actions {
        if let Action::CSI(CSI::Mode(ref mode)) = action {
            terminal.lock_state().handle_mode(mode);
        }
    }
    assert_eq!(terminal.mouse_tracking(), MouseTrackingMode::None);
}

// ---- Alternate screen tests ----

#[test]
fn alternate_screen_switching() {
    let mut terminal = test_terminal(80, 24);
    assert!(!terminal.is_alternate_screen());

    let mut parser = Parser::new();
    let actions = parser.parse_as_vec(b"\x1b[?1049h");
    for action in actions {
        if let Action::CSI(CSI::Mode(ref mode)) = action {
            terminal.lock_state().handle_mode(mode);
        }
    }
    assert!(terminal.is_alternate_screen());
    assert!(terminal.lock_state().alternate_surface.is_some());

    let actions = parser.parse_as_vec(b"\x1b[?1049l");
    for action in actions {
        if let Action::CSI(CSI::Mode(ref mode)) = action {
            terminal.lock_state().handle_mode(mode);
        }
    }
    assert!(!terminal.is_alternate_screen());
}

#[test]
fn alternate_screen_mode_47() {
    let mut terminal = test_terminal(80, 24);

    let mut parser = Parser::new();
    let actions = parser.parse_as_vec(b"\x1b[?47h");
    for action in actions {
        if let Action::CSI(CSI::Mode(ref mode)) = action {
            terminal.lock_state().handle_mode(mode);
        }
    }
    assert!(terminal.is_alternate_screen());

    let actions = parser.parse_as_vec(b"\x1b[?47l");
    for action in actions {
        if let Action::CSI(CSI::Mode(ref mode)) = action {
            terminal.lock_state().handle_mode(mode);
        }
    }
    assert!(!terminal.is_alternate_screen());
}

#[test]
fn alternate_screen_resize() {
    let mut terminal = test_terminal(80, 24);

    let mut parser = Parser::new();
    let actions = parser.parse_as_vec(b"\x1b[?1049h");
    for action in actions {
        if let Action::CSI(CSI::Mode(ref mode)) = action {
            terminal.lock_state().handle_mode(mode);
        }
    }

    terminal.resize(120, 40);
    assert_eq!(terminal.cols(), 120);
    assert_eq!(terminal.rows(), 40);
    let (cols, rows) = terminal.dimensions();
    assert_eq!(cols, 120);
    assert_eq!(rows, 40);
}

// ---- Arrow key mode switching ----

#[test]
fn arrow_key_sequences_normal_vs_application() {
    let mut terminal = test_terminal(80, 24);

    assert!(!terminal.application_cursor_keys());
    let mut parser = Parser::new();
    let actions = parser.parse_as_vec(b"\x1b[?1h");
    for action in actions {
        if let Action::CSI(CSI::Mode(ref mode)) = action {
            terminal.lock_state().handle_mode(mode);
        }
    }
    assert!(terminal.application_cursor_keys());
}

// ---- Full reset test ----

#[test]
fn full_reset_clears_modes() {
    let mut terminal = test_terminal(80, 24);

    let mut parser = Parser::new();
    let data = b"\x1b[?1h\x1b[?25l\x1b[?2004h\x1b[?1049h";
    let actions = parser.parse_as_vec(data);
    for action in actions {
        if let Action::CSI(CSI::Mode(ref mode)) = action {
            terminal.lock_state().handle_mode(mode);
        }
    }
    assert!(terminal.application_cursor_keys());
    assert!(!terminal.cursor_visible());
    assert!(terminal.bracketed_paste());
    assert!(terminal.is_alternate_screen());

    let actions = parser.parse_as_vec(b"\x1bc");
    for action in actions {
        let _changes = terminal.lock_state().action_to_changes(action);
    }
    assert!(!terminal.application_cursor_keys());
    assert!(terminal.cursor_visible());
    assert!(!terminal.bracketed_paste());
    assert!(!terminal.is_alternate_screen());
}

// ---- Scrollback capture: implicit scroll via line wrap ----

fn first_scrollback_text(terminal: &Terminal, index: usize) -> String {
    terminal
        .lock_state()
        .scrollback_line(index)
        .map(|l| {
            l.iter()
                .map(|(s, _)| s.clone())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .unwrap_or_default()
}

#[test]
fn scrollback_captured_on_lf_at_bottom_row() {
    let mut terminal = test_terminal(10, 4);
    terminal.process_bytes(b"row0\r\nrow1\r\nrow2\r\nrow3");
    assert_eq!(terminal.scrollback_len(), 0);
    terminal.process_bytes(b"\r\nrow4");
    assert_eq!(
        terminal.scrollback_len(),
        1,
        "newline at bottom row must push row0 to scrollback"
    );
    assert_eq!(first_scrollback_text(&terminal, 0), "row0");
}

// ---- Soft-wrap flag is captured into scrollback ----

#[test]
fn soft_wrapped_line_records_wrap_flag_in_scrollback() {
    // 10-col terminal, write 25 chars on one logical line then a real LF.
    // termwiz auto-wraps at col 10, producing 3 visual rows. With only
    // 4 screen rows, hitting LF after the wrap pushes the first wrapped
    // row into scrollback — and that scrollback line must keep wrapped=true.
    let mut terminal = test_terminal(10, 4);
    let payload: Vec<u8> = (b'a'..=b'y').collect(); // 25 chars
    terminal.process_bytes(&payload);
    // Force two extra LFs at the bottom to flush wrapped lines into scrollback.
    terminal.process_bytes(b"\r\nnext\r\nmore\r\nfinal");

    assert!(
        terminal.scrollback_len() >= 1,
        "expected wrapped lines to be pushed into scrollback"
    );
    // The first scrollback line is the head of the wrapped command, which
    // continues on the next line — it must be marked wrapped.
    assert_eq!(
        terminal.scrollback_line_wrapped(0),
        Some(true),
        "first scrollback line was a soft-wrap continuation point"
    );
}

#[test]
fn hard_newline_line_is_not_wrapped() {
    let mut terminal = test_terminal(10, 4);
    // "row0\nrow1\nrow2\nrow3\nrow4" — row0 ends with a real LF, no wrap.
    terminal.process_bytes(b"row0\r\nrow1\r\nrow2\r\nrow3\r\nrow4");
    assert!(terminal.scrollback_len() >= 1);
    assert_eq!(
        terminal.scrollback_line_wrapped(0),
        Some(false),
        "row0 ended in a hard newline, not a wrap"
    );
}

// ---- Regression: wrap-induced scroll at the bottom row must be captured ----

/// Join every scrollback line (oldest→newest) plus every visible row into a
/// single string, so a marker can be counted across the whole buffer.
fn all_buffer_text(terminal: &Terminal) -> String {
    let state = terminal.lock_state();
    let mut out = String::new();
    for i in 0..state.scrollback_len() {
        if let Some(cells) = state.scrollback_line_owned(i) {
            for (s, _) in &cells {
                out.push_str(s);
            }
            out.push('\n');
        }
    }
    for line in state.surface().screen_lines().iter() {
        for cell in line.visible_cells() {
            out.push_str(cell.str());
        }
        out.push('\n');
    }
    out
}

/// When a long line auto-wraps at the right edge while the cursor is already on
/// the bottom row, termwiz scrolls the grid internally (no `ScrollRegionUp`
/// Change is emitted). The evicted top row MUST still land in scrollback;
/// otherwise scrolling back loses content. This reproduces the loss bug.
#[test]
fn wrap_induced_scroll_at_bottom_row_reaches_scrollback() {
    let mut terminal = test_terminal(10, 4);
    // 8 visual rows of 10 identical chars each (no LF): "0000000000".."7777777777".
    // The screen holds 4; the first 4 rows are scrolled off by wrap-scroll.
    let mut payload = Vec::new();
    for d in b'0'..=b'7' {
        payload.extend(std::iter::repeat(d).take(10));
    }
    terminal.process_bytes(&payload);

    assert_eq!(
        terminal.scrollback_len(),
        4,
        "wrap-induced scroll at the bottom row must push the 4 evicted rows to scrollback"
    );
    assert_eq!(first_scrollback_text(&terminal, 0), "0000000000");
    assert_eq!(first_scrollback_text(&terminal, 3), "3333333333");
}

/// Conservation property: when wrap (not LF) drives the scrolling, every unique
/// marker must remain exactly once across (scrollback ∪ visible screen). A
/// missing marker = loss; a marker appearing twice = duplication. Catches both
/// faces of the bug in one assertion. Uses a continuous no-newline payload so
/// the scroll path is purely auto-wrap (the uncaptured path).
#[test]
fn wrapping_lines_are_preserved_exactly_once() {
    let mut terminal = test_terminal(10, 4);
    // N visual rows, each exactly 10 chars beginning with a unique marker,
    // concatenated with NO newlines so wrap alone drives the scrolling.
    const N: usize = 8;
    let mut payload = String::new();
    for i in 0..N {
        payload.push_str(&format!("MK{i:02}aaaaaa")); // "MKnn" + 6 = 10 cols
    }
    terminal.process_bytes(payload.as_bytes());

    let buffer = all_buffer_text(&terminal);
    for i in 0..N {
        let marker = format!("MK{i:02}");
        let count = buffer.matches(&marker).count();
        assert_eq!(
            count, 1,
            "marker {marker} must appear exactly once (count={count}); \
             0 = lost on wrap-scroll, >1 = duplicated"
        );
    }
}

/// Reproduces the on-screen duplication symptom: after content has wrap-scrolled
/// into history, a TUI repaints the visible region in place (absolute cursor
/// moves, no re-emission of committed lines). The committed markers must not end
/// up both in scrollback AND on screen — i.e. still exactly once each.
#[test]
fn repaint_after_wrap_scroll_does_not_duplicate_committed_lines() {
    let mut terminal = test_terminal(10, 4);
    // 6 wrap-driven rows: MK00..MK05. 4 fit; MK00/MK01 scroll into history.
    let mut payload = String::new();
    for i in 0..6 {
        payload.push_str(&format!("MK{i:02}aaaaaa"));
    }
    terminal.process_bytes(payload.as_bytes());

    // TUI frame repaint: rewrite each of the 4 visible rows in place via
    // absolute positioning + erase-line. No newline, so nothing scrolls.
    for row in 0..4 {
        terminal.process_bytes(format!("\x1b[{};1H\x1b[2K", row + 1).as_bytes());
        terminal.process_bytes(format!("RP{row:02}bbbbbb").as_bytes());
    }

    let buffer = all_buffer_text(&terminal);
    // Committed (scrolled-off) markers must survive exactly once.
    for i in 0..2 {
        let marker = format!("MK{i:02}");
        let count = buffer.matches(&marker).count();
        assert_eq!(
            count, 1,
            "committed marker {marker} must appear exactly once (count={count})"
        );
    }
}

fn join_line(line: &crate::ScrollbackLine) -> String {
    line.cells.iter().map(|(s, _)| s.as_str()).collect()
}

#[test]
fn screen_snapshot_captures_visible_content() {
    let mut terminal = test_terminal(10, 4);
    terminal.process_bytes(b"alpha\r\nbeta\r\ngamma");
    let snap = terminal.screen_snapshot_lines();
    // 화면에 3 줄 출력. 마지막 빈 줄들은 trim 되어 3 줄만 남는다.
    assert_eq!(snap.len(), 3);
    assert_eq!(join_line(&snap[0]).trim_end(), "alpha");
    assert_eq!(join_line(&snap[1]).trim_end(), "beta");
    assert_eq!(join_line(&snap[2]).trim_end(), "gamma");
}

#[test]
fn screen_snapshot_trims_trailing_blank_rows() {
    let mut terminal = test_terminal(10, 6);
    terminal.process_bytes(b"hello");
    let snap = terminal.screen_snapshot_lines();
    // 한 줄만 출력했으니 뒤따르는 5 줄 빈 row 는 모두 잘려야 한다.
    assert_eq!(snap.len(), 1);
    assert_eq!(join_line(&snap[0]).trim_end(), "hello");
}

#[test]
fn screen_snapshot_empty_terminal_returns_empty_vec() {
    let terminal = test_terminal(10, 4);
    assert!(terminal.screen_snapshot_lines().is_empty());
}

#[test]
fn prefill_visible_from_scrollback_draws_lines_and_parks_cursor() {
    use crate::ScrollbackLine;
    use termwiz::cell::CellAttributes;

    let mut terminal = test_terminal(20, 10);

    // Inject 15 historical lines.
    let mut injected = Vec::new();
    for i in 0..15 {
        let label = format!("OLD_{i:02}");
        let cells: Vec<(String, CellAttributes)> = label
            .chars()
            .map(|c| (c.to_string(), CellAttributes::default()))
            .collect();
        injected.push(ScrollbackLine::new(cells, false));
    }
    terminal.inject_scrollback(injected);

    // Prefill half the visible rows (5 of 10).
    let drawn = terminal.prefill_visible_from_scrollback(5);
    assert_eq!(drawn, 5);
    assert_eq!(terminal.scrollback_len(), 10); // 5 popped

    // Visible rows 0..4 should now hold the popped lines (oldest first).
    let lines = terminal.screen_lines();
    for (row, expected_idx) in (10..15).enumerate() {
        let text: String = lines[row]
            .visible_cells()
            .map(|c| c.str().to_string())
            .collect();
        let expected = format!("OLD_{expected_idx:02}");
        assert!(
            text.starts_with(&expected),
            "row {row} expected to start with {expected:?}, got {text:?}"
        );
    }

    // Cursor parked on row right after prefilled block.
    assert_eq!(terminal.cursor_position(), (0, 5));
}

#[test]
fn prefill_saturates_when_scrollback_is_shorter_than_requested() {
    use crate::ScrollbackLine;
    use termwiz::cell::CellAttributes;

    let mut terminal = test_terminal(20, 10);

    let mut injected = Vec::new();
    for i in 0..3 {
        let label = format!("OLD_{i}");
        let cells: Vec<(String, CellAttributes)> = label
            .chars()
            .map(|c| (c.to_string(), CellAttributes::default()))
            .collect();
        injected.push(ScrollbackLine::new(cells, false));
    }
    terminal.inject_scrollback(injected);

    // Request 5, but only 3 are available.
    let drawn = terminal.prefill_visible_from_scrollback(5);
    assert_eq!(drawn, 3);
    assert_eq!(terminal.scrollback_len(), 0);
    assert_eq!(terminal.cursor_position(), (0, 3));
}

/// Regression: when scrollback contains injected/historical lines (e.g. from
/// a previous session restore) and the cursor sits high with blank rows
/// below, repeated shrink→grow cycles must NOT keep pulling fresh lines
/// from scrollback into the visible area. Each cycle should be idempotent.
#[test]
fn resize_ping_pong_does_not_accumulate_visible_lines_when_cursor_is_high() {
    use crate::ScrollbackLine;
    use termwiz::cell::CellAttributes;

    let mut terminal = test_terminal(20, 10);

    // Inject 30 historical scrollback lines (simulates previous-session restore).
    let mut injected = Vec::new();
    for i in 0..30 {
        let label = format!("OLD_{i:02}");
        let cells: Vec<(String, CellAttributes)> = label
            .chars()
            .map(|c| (c.to_string(), CellAttributes::default()))
            .collect();
        injected.push(ScrollbackLine::new(cells, false));
    }
    terminal.inject_scrollback(injected);
    let initial_scrollback_len = terminal.scrollback_len();
    assert_eq!(initial_scrollback_len, 30);

    // Move cursor to (0,0) and write a short prompt — cursor stays high,
    // bottom rows remain blank.
    terminal.process_bytes(b"\x1b[H$ ");

    // 3 ping-pong cycles: grow → shrink → grow → shrink → grow → shrink.
    for _ in 0..3 {
        terminal.resize(20, 14); // grow by 4 rows
        terminal.resize(20, 10); // shrink back
    }

    // Scrollback should not have shrunk: handle_rows_grow should not have
    // popped lines that no corresponding shrink had pushed.
    assert_eq!(
        terminal.scrollback_len(),
        initial_scrollback_len,
        "scrollback was consumed by grow operations that had no matching shrink push"
    );

    // The visible screen should still show only the prompt at row 0 plus
    // blank rows below — no historical OLD_NN lines should have leaked
    // into the visible area.
    let lines = terminal.screen_lines();
    for (row, line) in lines.iter().enumerate() {
        let text: String = line.visible_cells().map(|c| c.str().to_string()).collect();
        assert!(
            !text.contains("OLD_"),
            "row {row} contains historical scrollback content: {text:?}"
        );
    }
}

// ---- Mirror foundation: new_detached + feed_bytes ----

/// A representative byte sequence exercising text, CR/LF, SGR color/intensity,
/// absolute cursor moves, EraseLine, a DECSET mode toggle, and a clear.
const MIRROR_SEQ: &[u8] = b"hello\r\n\x1b[31mred\x1b[0m world\r\n\x1b[2J\x1b[H\x1b[1mbold\x1b[0m\x1b[?1h\x1b[3;5Hxy\x1b[K";

fn assert_grid_eq(a: &Terminal, b: &Terminal, ctx: &str) {
    assert_eq!(a.screen_text(), b.screen_text(), "{ctx}: screen_text");
    assert_eq!(a.cursor_position(), b.cursor_position(), "{ctx}: cursor");
    assert_eq!(
        a.application_cursor_keys(),
        b.application_cursor_keys(),
        "{ctx}: DECCKM"
    );
    assert_eq!(
        a.is_alternate_screen(),
        b.is_alternate_screen(),
        "{ctx}: alt-screen"
    );
    // Compare per-cell text + key attributes on the populated rows.
    for row in 0..4 {
        let ac = a.row_cells(row);
        let bc = b.row_cells(row);
        assert_eq!(ac.len(), bc.len(), "{ctx}: row {row} cell count");
        for ((aidx, ai), (bidx, bi)) in ac.iter().zip(bc.iter()) {
            assert_eq!(aidx, bidx, "{ctx}: row {row} cell index");
            assert_eq!(ai.text, bi.text, "{ctx}: row {row} text");
            assert_eq!(
                (ai.fg.as_str(), ai.intensity, ai.inverse),
                (bi.fg.as_str(), bi.intensity, bi.inverse),
                "{ctx}: row {row} attrs"
            );
        }
    }
}

#[test]
fn detached_feed_matches_pty_process_path() {
    let mut real = test_terminal(40, 12); // PTY-backed; shell output unused
    let mut mirror = Terminal::new_detached(40, 12);
    real.process_bytes(MIRROR_SEQ); // shared ingest path (= process() parsing)
    mirror.feed_bytes(MIRROR_SEQ);
    assert_grid_eq(&real, &mirror, "single-shot");
}

#[test]
fn detached_feed_is_chunk_boundary_invariant() {
    // Feeding the same stream split at arbitrary byte boundaries (including
    // mid-escape) must reconstruct an identical grid — the parser carries
    // state across feed_bytes calls.
    let whole = {
        let mut t = Terminal::new_detached(40, 12);
        t.feed_bytes(MIRROR_SEQ);
        t
    };
    // Split points: mid-escape "\x1b[3" | ";5Hxy..." and a few others.
    for split in [1usize, 7, 18, 30, MIRROR_SEQ.len() - 3] {
        let mut t = Terminal::new_detached(40, 12);
        t.feed_bytes(&MIRROR_SEQ[..split]);
        t.feed_bytes(&MIRROR_SEQ[split..]);
        assert_grid_eq(&whole, &t, &format!("split@{split}"));
    }
}

#[test]
fn detached_alt_screen_parity() {
    let seq = b"main\x1b[?1049h\x1b[Halt-content\x1b[?1049lback";
    let mut real = test_terminal(40, 12);
    let mut mirror = Terminal::new_detached(40, 12);
    real.process_bytes(b"main\x1b[?1049h\x1b[Halt-content");
    mirror.feed_bytes(b"main\x1b[?1049h\x1b[Halt-content");
    assert_grid_eq(&real, &mirror, "in-alt");
    real.process_bytes(b"\x1b[?1049lback");
    mirror.feed_bytes(b"\x1b[?1049lback");
    assert_grid_eq(&real, &mirror, "after-exit");
    let _ = seq;
}

#[test]
fn detached_terminal_has_no_pty_state() {
    let mut t = Terminal::new_detached(40, 12);
    assert_eq!(t.process_id(), None);
    assert!(!t.is_busy());
    assert!(t.is_alive(), "detached mirror is considered alive");
    // resize touches only the surface; no PTY notification is queued.
    t.resize(80, 24);
    assert_eq!(t.cols(), 80);
    assert_eq!(t.rows(), 24);
    assert!(!t.has_pending_pty_resize());
    let (cols, rows) = t.dimensions();
    assert_eq!((cols, rows), (80, 24));
    // process() on a detached terminal is a harmless no-op (no child exit event).
    assert!(!t.process());
    assert!(t.take_events().is_empty());
}

#[test]
fn feed_bytes_reports_change() {
    let mut t = Terminal::new_detached(40, 12);
    assert!(!t.feed_bytes(b""), "empty feed does not change the surface");
    assert!(t.feed_bytes(b"x"), "text feed changes the surface");
}

#[test]
fn detached_input_forwards_to_sink() {
    use std::sync::mpsc;
    let mut t = Terminal::new_detached(40, 12);
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    t.set_input_sink(tx);
    t.send_bytes(b"abc");
    t.send_key("Z");
    assert_eq!(rx.recv().unwrap(), b"abc".to_vec());
    assert_eq!(rx.recv().unwrap(), b"Z".to_vec());
}

// OSC 52 read query (`OSC 52 ; c ; ? ST`) must surface a single ClipboardQuery
// event through the production ingest path — the host gates the actual reply.
#[test]
fn osc52_read_query_emits_clipboard_query_event() {
    let mut t = Terminal::new_detached(80, 24);
    t.feed_bytes(b"\x1b]52;c;?\x07");
    let queries = t
        .take_events()
        .into_iter()
        .filter(|e| matches!(e.kind, TerminalEventKind::ClipboardQuery))
        .count();
    assert_eq!(queries, 1, "exactly one ClipboardQuery expected");
}

// Regression: OSC 52 write (set) still produces a ClipboardSet event, not a
// ClipboardQuery. `aGk=` is base64 for "hi".
#[test]
fn osc52_write_still_emits_clipboard_set() {
    let mut t = Terminal::new_detached(80, 24);
    t.feed_bytes(b"\x1b]52;c;aGk=\x07");
    let events = t.take_events();
    assert!(
        events
            .iter()
            .any(|e| matches!(&e.kind, TerminalEventKind::ClipboardSet(s) if s == "hi")),
        "OSC 52 write should emit ClipboardSet(\"hi\")"
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e.kind, TerminalEventKind::ClipboardQuery)),
        "a write must not emit a ClipboardQuery"
    );
}

#[test]
fn da2_query_emits_secondary_attributes_response() {
    use std::sync::mpsc;
    let mut t = Terminal::new_detached(80, 24);
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    t.set_input_sink(tx);
    t.feed_bytes(b"\x1b[>c"); // DA2
    let resp = rx.try_recv().expect("no DA2 response");
    assert!(
        resp.starts_with(b"\x1b[>") && resp.ends_with(b"c"),
        "unexpected DA2 response: {resp:?}"
    );
}

#[test]
fn xtversion_query_emits_name_and_version_response() {
    use std::sync::mpsc;
    let mut t = Terminal::new_detached(80, 24);
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    t.set_input_sink(tx);
    t.feed_bytes(b"\x1b[>0q"); // XTVERSION
    let resp = rx.try_recv().expect("no XTVERSION response");
    assert!(
        resp.starts_with(b"\x1bP") && resp.ends_with(b"\x1b\\"),
        "XTVERSION must be a DCS string terminated by ST: {resp:?}"
    );
    assert!(
        resp.windows(5).any(|w| w == b"tasty"),
        "XTVERSION must contain the terminal name: {resp:?}"
    );
}

#[test]
fn xtgettcap_query_emits_unsupported_reply() {
    use std::sync::mpsc;
    let mut t = Terminal::new_detached(80, 24);
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    t.set_input_sink(tx);
    // XtGetTcap for "Co" (436f). Currently unsupported → DCS 0 + r 436f ST.
    t.feed_bytes(b"\x1bP+q436f\x1b\\");
    let resp = rx.try_recv().expect("no XtGetTcap response");
    assert_eq!(
        resp, b"\x1bP0+r436f\x1b\\",
        "XtGetTcap must answer status 0 (currently unsupported): {resp:?}"
    );
}

#[test]
fn xtgettcap_multi_cap_query_emits_one_reply_each() {
    use std::sync::mpsc;
    let mut t = Terminal::new_detached(80, 24);
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    t.set_input_sink(tx);
    // Two caps in one query: "Co" (436f) ; "TN" (544e).
    t.feed_bytes(b"\x1bP+q436f;544e\x1b\\");
    let first = rx.try_recv().expect("no reply for first cap");
    assert_eq!(first, b"\x1bP0+r436f\x1b\\", "unexpected first reply: {first:?}");
    let second = rx.try_recv().expect("no reply for second cap");
    assert_eq!(second, b"\x1bP0+r544e\x1b\\", "unexpected second reply: {second:?}");
}

#[test]
fn da1_query_still_emits_primary_attributes_response() {
    use std::sync::mpsc;
    let mut t = Terminal::new_detached(80, 24);
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    t.set_input_sink(tx);
    t.feed_bytes(b"\x1b[c"); // DA1 — must be unchanged
    assert_eq!(rx.try_recv().expect("no DA1 response"), b"\x1b[?1;2c".to_vec());
}

#[test]
fn detached_input_without_sink_is_dropped() {
    // No sink wired: must not panic or hang, just drop.
    let mut t = Terminal::new_detached(40, 12);
    t.send_bytes(b"abc");
    t.send_key("Z");
}

// ---- Server-side output tap (fan-out) ----

#[test]
fn output_tap_receives_raw_bytes_and_replays_to_mirror() {
    let mut t = test_terminal(40, 12);
    let rx = t.add_output_tap();
    t.process_bytes(b"\x1b[31mX");
    assert_eq!(rx.try_recv().unwrap(), b"\x1b[31mX".to_vec());

    // Replaying the tapped bytes into a mirror yields an identical grid.
    let mut mirror = Terminal::new_detached(40, 12);
    mirror.feed_bytes(b"\x1b[31mX");
    assert_grid_eq(&t, &mirror, "tap-replay");
}

#[test]
fn output_tap_is_non_destructive() {
    // The grid produced with a tap attached must match the grid without one.
    let mut tapped = test_terminal(40, 12);
    let _rx = tapped.add_output_tap();
    tapped.process_bytes(MIRROR_SEQ);

    let mut untapped = test_terminal(40, 12);
    untapped.process_bytes(MIRROR_SEQ);
    assert_grid_eq(&tapped, &untapped, "tap-nondestructive");
}

#[test]
fn output_tap_disconnected_is_pruned() {
    let mut t = test_terminal(40, 12);
    let rx = t.add_output_tap();
    drop(rx); // subscriber gone
    // Next ingest detects the disconnect, prunes the tap, and applies normally.
    t.process_bytes(b"hello");
    assert!(t.screen_text().contains("hello"));
    // A fresh tap still works after pruning.
    let rx2 = t.add_output_tap();
    t.process_bytes(b"!");
    assert_eq!(rx2.try_recv().unwrap(), b"!".to_vec());
}

// ---- Initial bulk snapshot (snapshot_as_vt, attach step 4) ----

/// Snapshot-specific grid comparison. Unlike [`assert_grid_eq`], this does NOT
/// compare raw `visible_cells()` counts: termwiz back-fills a row to full width
/// when reached via absolute cursor addressing (CUP), while `EraseLine` shrinks
/// it — so the raw cell count depends on the write/erase *history*, not the final
/// visible content. The snapshot reproduces visible content, not history. We
/// therefore compare the rendered text, cursor, modes, and the attributes of each
/// non-blank cell (which is what the renderer actually draws).
fn assert_snapshot_eq(server: &Terminal, mirror: &Terminal, ctx: &str) {
    assert_eq!(
        server.screen_text(),
        mirror.screen_text(),
        "{ctx}: screen_text"
    );
    assert_eq!(
        server.cursor_position(),
        mirror.cursor_position(),
        "{ctx}: cursor"
    );
    assert_eq!(
        server.application_cursor_keys(),
        mirror.application_cursor_keys(),
        "{ctx}: DECCKM"
    );
    assert_eq!(
        server.is_alternate_screen(),
        mirror.is_alternate_screen(),
        "{ctx}: alt-screen"
    );
    // Per non-blank cell: text + key attributes must match at the same position.
    let nonblank = |t: &Terminal, row: usize| -> Vec<(usize, String, String, &'static str, bool)> {
        t.row_cells(row)
            .into_iter()
            .filter(|(_, ci)| ci.text != " " && !ci.text.is_empty())
            .map(|(idx, ci)| (idx, ci.text, ci.fg, ci.intensity, ci.inverse))
            .collect()
    };
    for row in 0..server.rows() {
        assert_eq!(
            nonblank(server, row),
            nonblank(mirror, row),
            "{ctx}: row {row} non-blank cells"
        );
    }
}

#[test]
fn snapshot_as_vt_reconstructs_grid_in_mirror() {
    // Build a populated server-side screen (text + SGR + cursor move + DECCKM).
    let mut server = test_terminal(40, 12);
    server.process_bytes(MIRROR_SEQ);

    // Serialize the current screen and feed it into a fresh mirror: the mirror's
    // visible content + cursor + modes must match the server's.
    let snapshot = server.snapshot_as_vt();
    let mut mirror = Terminal::new_detached(40, 12);
    mirror.feed_bytes(&snapshot);
    assert_snapshot_eq(&server, &mirror, "snapshot-replay");
}

#[test]
fn snapshot_as_vt_preserves_colors_and_intensity() {
    // 24-bit fg, palette bg, bold — all must round-trip through the snapshot.
    let mut server = test_terminal(20, 4);
    server.process_bytes(b"\x1b[38;2;10;200;30m\x1b[44m\x1b[1mAB\x1b[0mC");
    let snapshot = server.snapshot_as_vt();
    let mut mirror = Terminal::new_detached(20, 4);
    mirror.feed_bytes(&snapshot);

    let a = server.row_cells(0);
    let b = mirror.row_cells(0);
    assert_eq!(a.len(), b.len(), "cell count");
    for ((_, ai), (_, bi)) in a.iter().zip(b.iter()) {
        assert_eq!(
            (
                ai.text.as_str(),
                ai.fg.as_str(),
                ai.bg.as_str(),
                ai.intensity
            ),
            (
                bi.text.as_str(),
                bi.fg.as_str(),
                bi.bg.as_str(),
                bi.intensity
            ),
            "cell attrs round-trip"
        );
    }
}

#[test]
fn snapshot_as_vt_preserves_alt_screen() {
    let mut server = test_terminal(20, 4);
    server.process_bytes(b"primary\x1b[?1049h\x1b[Halt-only");
    assert!(server.is_alternate_screen());
    let snapshot = server.snapshot_as_vt();
    let mut mirror = Terminal::new_detached(20, 4);
    mirror.feed_bytes(&snapshot);
    assert!(mirror.is_alternate_screen(), "mirror enters alt-screen");
    assert_eq!(server.screen_text(), mirror.screen_text(), "alt content");
}

// ---- check_process_alive throttle tests ----

#[test]
fn alive_check_throttled_within_window() {
    let mut t = test_terminal(80, 24);
    // In-the-past init: the first process() must check immediately.
    assert!(t.last_alive_check.elapsed() >= ALIVE_CHECK_INTERVAL);

    t.process();
    let first = t.last_alive_check;
    assert!(
        first.elapsed() < ALIVE_CHECK_INTERVAL,
        "first check stamped now"
    );

    // Immediate second call falls inside the throttle window — no re-check.
    t.process();
    assert_eq!(first, t.last_alive_check, "check skipped within window");

    // After the window elapses the check runs (and re-stamps) again.
    std::thread::sleep(ALIVE_CHECK_INTERVAL + std::time::Duration::from_millis(100));
    t.process();
    assert!(t.last_alive_check > first, "check re-ran after window");
}

#[test]
fn process_exited_eventually_emitted() {
    let waker = noop_waker();
    let mut t = Terminal::new(
        TerminalConfig {
            cols: 80,
            rows: 24,
            shell: None,
            args: &[],
            surface_id: 0,
            working_dir: None,
            initial_input: Some("exit\r"),
        },
        waker,
    )
    .expect("terminal creation");

    // The shell exits on its own; the reader thread's final EOF wake plus the
    // Disconnected fast path must surface ProcessExited well within the
    // deadline regardless of the throttle window.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut seen = false;
    while std::time::Instant::now() < deadline {
        t.process();
        if t.lock_state()
            .events
            .iter()
            .any(|e| matches!(e.kind, TerminalEventKind::ProcessExited))
        {
            seen = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(seen, "ProcessExited not emitted before deadline");
}

// ---- OutputAppended observer gate tests ----

/// Concat of all OutputAppended texts — termwiz may deliver "hello" as one
/// PrintString or a chain of Print(c), so individual events are not stable.
fn appended_text(events: &[TerminalEvent]) -> String {
    events
        .iter()
        .filter_map(|e| match &e.kind {
            TerminalEventKind::OutputAppended { text } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn no_output_appended_when_gate_off() {
    let mut t = Terminal::new_detached(80, 24);
    // The gate defaults to off — no setter call needed.
    t.feed_bytes(b"hello world");
    assert_eq!(appended_text(&t.take_events()), "");

    t.set_output_events_enabled(false);
    t.feed_bytes(b"more");
    assert_eq!(appended_text(&t.take_events()), "");
}

#[test]
fn output_appended_emitted_when_gate_on() {
    let mut t = Terminal::new_detached(80, 24);
    t.set_output_events_enabled(true);
    t.feed_bytes(b"hello");
    assert_eq!(appended_text(&t.take_events()), "hello");
}

#[test]
fn output_appended_stops_after_gate_turned_off() {
    let mut t = Terminal::new_detached(80, 24);
    t.set_output_events_enabled(true);
    t.feed_bytes(b"on");
    assert_eq!(appended_text(&t.take_events()), "on");

    t.set_output_events_enabled(false);
    t.feed_bytes(b"off");
    assert_eq!(appended_text(&t.take_events()), "");
}

#[test]
fn detached_terminal_never_emits_process_exited() {
    let mut t = Terminal::new_detached(40, 12);
    t.process();
    std::thread::sleep(ALIVE_CHECK_INTERVAL + std::time::Duration::from_millis(50));
    t.process();
    assert!(
        !t.lock_state()
            .events
            .iter()
            .any(|e| matches!(e.kind, TerminalEventKind::ProcessExited)),
        "detached mirror has no child to exit"
    );
}

// ---- H4: SGR Overline(53) / UnderlineColor(58) / VerticalAlign(73-75) ----
// These SGRs have no termwiz `AttributeChange` variant, so they are applied via
// the mirrored pen + `Change::AllAttributes`. `cell_info` must report the real
// cell state instead of the default values. Production path: new_detached +
// feed_bytes (the `TestTerminal` map_sgr clone drops these, so it cannot be used
// here without a false pass).

#[test]
fn sgr_overline_reflected_in_cell_info() {
    let mut t = Terminal::new_detached(80, 24);
    t.feed_bytes(b"\x1b[53mX");
    let info = t.cell_info(0, 0).expect("cell 0,0");
    assert_eq!(info.text, "X");
    assert!(info.overline, "SGR 53 should set overline");

    // SGR 55 turns overline back off.
    t.feed_bytes(b"\x1b[55mY");
    let info = t.cell_info(0, 1).expect("cell 0,1");
    assert_eq!(info.text, "Y");
    assert!(!info.overline, "SGR 55 should clear overline");
}

#[test]
fn sgr_underline_color_reflected_in_cell_info() {
    let mut t = Terminal::new_detached(80, 24);
    t.feed_bytes(b"\x1b[58;5;9mX");
    let info = t.cell_info(0, 0).expect("cell 0,0");
    assert_eq!(info.text, "X");
    assert_ne!(
        info.underline_color, "default",
        "SGR 58 should set a non-default underline color"
    );
    assert_eq!(info.underline_color, "palette:9");

    // SGR 59 resets the underline color to default.
    t.feed_bytes(b"\x1b[59mY");
    let info = t.cell_info(0, 1).expect("cell 0,1");
    assert_eq!(info.underline_color, "default", "SGR 59 should reset");
}

#[test]
fn sgr_vertical_align_reflected_in_cell_info() {
    let mut t = Terminal::new_detached(80, 24);
    t.feed_bytes(b"\x1b[73mS"); // superscript
    let info = t.cell_info(0, 0).expect("cell 0,0");
    assert_eq!(info.vertical_align, "super");

    t.feed_bytes(b"\x1b[74mU"); // subscript
    let info = t.cell_info(0, 1).expect("cell 0,1");
    assert_eq!(info.vertical_align, "sub");

    t.feed_bytes(b"\x1b[75mB"); // back to baseline
    let info = t.cell_info(0, 2).expect("cell 0,2");
    assert_eq!(info.vertical_align, "baseline");
}

#[test]
fn sgr_overline_preserves_other_attributes() {
    // Bold set before overline must survive the AllAttributes round-trip.
    let mut t = Terminal::new_detached(80, 24);
    t.feed_bytes(b"\x1b[1m\x1b[53mX");
    let info = t.cell_info(0, 0).expect("cell 0,0");
    assert!(info.bold, "bold (SGR 1) must be preserved");
    assert!(info.overline, "overline (SGR 53) must be applied");
}

#[test]
fn sgr_reset_clears_overline_and_align() {
    let mut t = Terminal::new_detached(80, 24);
    t.feed_bytes(b"\x1b[53;73mX"); // overline + superscript
    let info = t.cell_info(0, 0).expect("cell 0,0");
    assert!(info.overline);
    assert_eq!(info.vertical_align, "super");

    t.feed_bytes(b"\x1b[0mY"); // full SGR reset
    let info = t.cell_info(0, 1).expect("cell 0,1");
    assert!(!info.overline, "SGR 0 should clear overline");
    assert_eq!(
        info.vertical_align, "baseline",
        "SGR 0 should reset vertical align"
    );
}

#[test]
fn sgr_overline_does_not_regress_basic_attributes() {
    // Sanity: introducing the pen mirror must not break standard SGR reporting.
    let mut t = Terminal::new_detached(80, 24);
    t.feed_bytes(b"\x1b[1;4;7mX");
    let info = t.cell_info(0, 0).expect("cell 0,0");
    assert!(info.bold);
    assert!(info.underline);
    assert!(info.inverse);
    assert!(!info.overline, "overline must stay false when unset");
}

#[test]
fn osc8_hyperlink_attaches_uri_to_cells_then_clears() {
    // OSC 8 opens a hyperlink, prints "LINK", then closes it. The four LINK
    // cells must carry the URI; cells printed after the close must not.
    let mut t = Terminal::new_detached(80, 24);
    t.feed_bytes(b"\x1b]8;;https://example.com\x1b\\LINK\x1b]8;;\x1b\\");

    for col in 0..4 {
        let attrs = t.cell_attrs(0, col).expect("LINK cell");
        let link = attrs
            .hyperlink()
            .unwrap_or_else(|| panic!("cell {col} should carry a hyperlink"));
        assert_eq!(link.uri(), "https://example.com");
    }

    // After the OSC 8 close (empty URI), subsequent cells have no hyperlink.
    t.feed_bytes(b"PLAIN");
    let attrs = t.cell_attrs(0, 4).expect("PLAIN cell");
    assert!(
        attrs.hyperlink().is_none(),
        "hyperlink must not leak past the OSC 8 close"
    );
}

// ---- DECSCUSR (CSI Ps SP q): cursor shape ----

#[test]
fn decscusr_sets_cursor_shape() {
    // Default on a fresh terminal.
    let mut t = Terminal::new_detached(80, 24);
    assert_eq!(t.cursor_shape(), CursorShape::Default);

    // Each xterm parameter maps to its shape + blink flag.
    for (seq, expected) in [
        (&b"\x1b[1 q"[..], CursorShape::BlinkingBlock),
        (&b"\x1b[2 q"[..], CursorShape::SteadyBlock),
        (&b"\x1b[3 q"[..], CursorShape::BlinkingUnderline),
        (&b"\x1b[4 q"[..], CursorShape::SteadyUnderline),
        (&b"\x1b[5 q"[..], CursorShape::BlinkingBar),
        (&b"\x1b[6 q"[..], CursorShape::SteadyBar),
        (&b"\x1b[0 q"[..], CursorShape::Default),
    ] {
        t.feed_bytes(seq);
        assert_eq!(t.cursor_shape(), expected, "seq {seq:?}");
    }
}

#[test]
fn decscusr_reset_by_full_reset() {
    let mut t = Terminal::new_detached(80, 24);
    t.feed_bytes(b"\x1b[5 q"); // blinking bar
    assert_eq!(t.cursor_shape(), CursorShape::BlinkingBar);
    t.feed_bytes(b"\x1bc"); // RIS / FullReset
    assert_eq!(t.cursor_shape(), CursorShape::Default);
}

#[test]
fn decscusr_survives_soft_reset() {
    // DECSTR (CSI ! p) intentionally does NOT reset the cursor shape (matches
    // xterm — only RIS restores the default). Regression guard.
    let mut t = Terminal::new_detached(80, 24);
    t.feed_bytes(b"\x1b[5 q"); // blinking bar
    assert_eq!(t.cursor_shape(), CursorShape::BlinkingBar);
    t.feed_bytes(b"\x1b[!p"); // DECSTR soft reset
    assert_eq!(t.cursor_shape(), CursorShape::BlinkingBar);
}

// ---- OSC color queries (H3): answer with the plumbed theme palette ----

/// Distinct per-channel test palette so each color number's response is
/// unambiguous. fg/bg/cursor and a couple of ANSI entries use values that
/// survive the 8→16-bit widening unchanged in the low byte.
fn test_palette() -> ColorPalette {
    let mut ansi = [TerminalRgb::new(0, 0, 0); 16];
    ansi[1] = TerminalRgb::new(0xff, 0x00, 0x00); // ANSI 1 (red)
    ansi[4] = TerminalRgb::new(0x12, 0x34, 0x56); // ANSI 4 (blue), arbitrary
    ColorPalette {
        foreground: TerminalRgb::new(0xab, 0xcd, 0xef),
        background: TerminalRgb::new(0x10, 0x20, 0x30),
        cursor: TerminalRgb::new(0x11, 0x22, 0x33),
        ansi,
    }
}

#[test]
fn osc11_bg_query_responds_with_palette_background() {
    use std::sync::mpsc;
    let mut t = Terminal::new_detached(80, 24);
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    t.set_input_sink(tx);
    t.set_color_palette(test_palette());
    t.feed_bytes(b"\x1b]11;?\x1b\\"); // bg color query (ST terminated)
    let resp = rx.try_recv().expect("no OSC 11 response");
    // bg = 0x10/0x20/0x30 → widened by *0x101.
    assert_eq!(resp, b"\x1b]11;rgb:1010/2020/3030\x1b\\".to_vec());
}

#[test]
fn osc10_fg_query_responds_with_palette_foreground() {
    use std::sync::mpsc;
    let mut t = Terminal::new_detached(80, 24);
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    t.set_input_sink(tx);
    t.set_color_palette(test_palette());
    t.feed_bytes(b"\x1b]10;?\x1b\\"); // fg color query
    let resp = rx.try_recv().expect("no OSC 10 response");
    assert_eq!(resp, b"\x1b]10;rgb:abab/cdcd/efef\x1b\\".to_vec());
}

#[test]
fn osc12_cursor_query_responds_with_palette_cursor() {
    use std::sync::mpsc;
    let mut t = Terminal::new_detached(80, 24);
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    t.set_input_sink(tx);
    t.set_color_palette(test_palette());
    t.feed_bytes(b"\x1b]12;?\x1b\\"); // cursor color query
    let resp = rx.try_recv().expect("no OSC 12 response");
    assert_eq!(resp, b"\x1b]12;rgb:1111/2222/3333\x1b\\".to_vec());
}

#[test]
fn osc4_palette_query_responds_with_ansi_color() {
    use std::sync::mpsc;
    let mut t = Terminal::new_detached(80, 24);
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    t.set_input_sink(tx);
    t.set_color_palette(test_palette());
    t.feed_bytes(b"\x1b]4;4;?\x1b\\"); // query ANSI index 4
    let resp = rx.try_recv().expect("no OSC 4 response");
    assert_eq!(resp, b"\x1b]4;4;rgb:1212/3434/5656\x1b\\".to_vec());
}

#[test]
fn osc10_multi_query_reflects_fg_bg_cursor_in_sequence() {
    // `OSC 10 ; ? ; ? ; ?` walks fg(10) → bg(11) → cursor(12); each `?` answers
    // the next color number.
    use std::sync::mpsc;
    let mut t = Terminal::new_detached(80, 24);
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    t.set_input_sink(tx);
    t.set_color_palette(test_palette());
    t.feed_bytes(b"\x1b]10;?;?;?\x1b\\");
    assert_eq!(
        rx.try_recv().expect("no fg response"),
        b"\x1b]10;rgb:abab/cdcd/efef\x1b\\".to_vec()
    );
    assert_eq!(
        rx.try_recv().expect("no bg response"),
        b"\x1b]11;rgb:1010/2020/3030\x1b\\".to_vec()
    );
    assert_eq!(
        rx.try_recv().expect("no cursor response"),
        b"\x1b]12;rgb:1111/2222/3333\x1b\\".to_vec()
    );
}

#[test]
fn osc_color_query_without_palette_is_silent() {
    // No palette plumbed → no source → leave the query unanswered (don't guess).
    use std::sync::mpsc;
    let mut t = Terminal::new_detached(80, 24);
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    t.set_input_sink(tx);
    t.feed_bytes(b"\x1b]11;?\x1b\\");
    assert!(rx.try_recv().is_err(), "unset palette must not respond");
}

#[test]
fn osc_color_set_request_is_not_answered() {
    // A *set* (color spec, not `?`) must not emit any response — only queries do.
    use std::sync::mpsc;
    let mut t = Terminal::new_detached(80, 24);
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    t.set_input_sink(tx);
    t.set_color_palette(test_palette());
    t.feed_bytes(b"\x1b]11;rgb:0000/0000/0000\x1b\\"); // set bg, no query
    assert!(rx.try_recv().is_err(), "set request must not respond");
}

// ---- pen-mirror surface-boundary sync (07/H4 follow-up) ----
// termwiz keeps a separate pen per surface; the direct `add_change` paths
// (alt-screen switch, resize reflow, scrollback prefill) bypass `mirror_pen`.
// These assert that `current_pen` stays aligned with the *active* surface so
// cell_info never reports a stale attribute leaked across a surface boundary.

#[test]
fn alt_screen_entry_does_not_leak_primary_pen_into_cell_info() {
    let mut t = Terminal::new_detached(80, 24);
    // Primary: set bold, write a cell.
    t.feed_bytes(b"\x1b[1mP");
    assert!(t.cell_info(0, 0).expect("primary cell").bold);

    // Enter alt screen (1049h), then apply overline only.
    t.feed_bytes(b"\x1b[?1049h");
    t.feed_bytes(b"\x1b[53mX");
    let a = t.cell_info(0, 0).expect("alt cell");
    assert_eq!(a.text, "X");
    assert!(a.overline, "alt cell should have overline");
    assert!(
        !a.bold,
        "stale primary bold must not leak onto the alt cell"
    );
}

#[test]
fn alt_screen_exit_restores_primary_pen() {
    let mut t = Terminal::new_detached(80, 24);
    // Primary bold, write P at 0,0 (cursor advances to col 1).
    t.feed_bytes(b"\x1b[1mP");
    // Round-trip through the alt screen with a different pen.
    t.feed_bytes(b"\x1b[?1049h");
    t.feed_bytes(b"\x1b[53mX"); // overline on alt, no bold
    t.feed_bytes(b"\x1b[?1049l"); // leave alt; cursor restored to (col 1, row 0)
    // Apply overline on primary; the restored pen still carries bold.
    t.feed_bytes(b"\x1b[53mQ");
    let q = t.cell_info(0, 1).expect("primary cell after exit");
    assert_eq!(q.text, "Q");
    assert!(q.bold, "primary pen (bold) must be restored after leaving alt");
    assert!(q.overline, "overline applied on primary after exit");
    // Original primary cell intact.
    let p = t.cell_info(0, 0).expect("original primary cell");
    assert_eq!(p.text, "P");
    assert!(p.bold);
    assert!(!p.overline, "original cell never had overline");
}

#[test]
fn alt_screen_47_switch_does_not_leak_pen() {
    // Mode 47 switches without clearing; the pen must still track the surface.
    let mut t = Terminal::new_detached(80, 24);
    t.feed_bytes(b"\x1b[1mP"); // primary bold
    t.feed_bytes(b"\x1b[?47h"); // enter alt (no clear)
    t.feed_bytes(b"\x1b[53mX"); // overline only on alt
    let a = t.cell_info(0, 0).expect("alt cell");
    assert!(a.overline);
    assert!(!a.bold, "primary bold must not leak across the 47h switch");
}

#[test]
fn resize_restore_does_not_leave_stale_pen() {
    // Shrink then grow forces the grow/tail restore paths to emit
    // `AllAttributes` per restored cell directly on the surface. After that the
    // active pen must still be the logical pen (bold), not the plain attrs left
    // by the last restored cell.
    let mut t = Terminal::new_detached(80, 6);
    t.feed_bytes(b"L1\r\nL2\r\nL3\r\nL4\r\n");
    t.feed_bytes(b"\x1b[1m"); // logical pen = bold (no cell written yet)
    t.resize(80, 3); // shrink: pushes top lines to scrollback
    t.resize(80, 6); // grow: restores them via direct AllAttributes(plain)
    // Home + plain cell: should pick up the logical bold pen, not the
    // restoration artifact left on the surface by the restore loop.
    t.feed_bytes(b"\x1b[HZ");
    let z = t.cell_info(0, 0).expect("cell 0,0");
    assert_eq!(z.text, "Z");
    assert!(z.bold, "resize restore must not leave a stale (non-bold) pen");
}
