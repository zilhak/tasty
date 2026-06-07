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
            terminal.handle_mode(mode);
        }
    }
    assert!(terminal.application_cursor_keys());

    let actions = parser.parse_as_vec(b"\x1b[?1l");
    for action in actions {
        if let Action::CSI(CSI::Mode(ref mode)) = action {
            terminal.handle_mode(mode);
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
            terminal.handle_mode(mode);
        }
    }
    assert!(!terminal.cursor_visible());

    let actions = parser.parse_as_vec(b"\x1b[?25h");
    for action in actions {
        if let Action::CSI(CSI::Mode(ref mode)) = action {
            terminal.handle_mode(mode);
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
            terminal.handle_mode(mode);
        }
    }
    assert!(terminal.bracketed_paste());

    let actions = parser.parse_as_vec(b"\x1b[?2004l");
    for action in actions {
        if let Action::CSI(CSI::Mode(ref mode)) = action {
            terminal.handle_mode(mode);
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
            terminal.handle_mode(mode);
        }
    }
    assert_eq!(terminal.mouse_tracking(), MouseTrackingMode::Click);

    let actions = parser.parse_as_vec(b"\x1b[?1003h");
    for action in actions {
        if let Action::CSI(CSI::Mode(ref mode)) = action {
            terminal.handle_mode(mode);
        }
    }
    assert_eq!(terminal.mouse_tracking(), MouseTrackingMode::AllMotion);

    let actions = parser.parse_as_vec(b"\x1b[?1003l");
    for action in actions {
        if let Action::CSI(CSI::Mode(ref mode)) = action {
            terminal.handle_mode(mode);
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
            terminal.handle_mode(mode);
        }
    }
    assert!(terminal.is_alternate_screen());
    assert!(terminal.alternate_surface.is_some());

    let actions = parser.parse_as_vec(b"\x1b[?1049l");
    for action in actions {
        if let Action::CSI(CSI::Mode(ref mode)) = action {
            terminal.handle_mode(mode);
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
            terminal.handle_mode(mode);
        }
    }
    assert!(terminal.is_alternate_screen());

    let actions = parser.parse_as_vec(b"\x1b[?47l");
    for action in actions {
        if let Action::CSI(CSI::Mode(ref mode)) = action {
            terminal.handle_mode(mode);
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
            terminal.handle_mode(mode);
        }
    }

    terminal.resize(120, 40);
    assert_eq!(terminal.cols(), 120);
    assert_eq!(terminal.rows(), 40);
    let (cols, rows) = terminal.surface().dimensions();
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
            terminal.handle_mode(mode);
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
            terminal.handle_mode(mode);
        }
    }
    assert!(terminal.application_cursor_keys());
    assert!(!terminal.cursor_visible());
    assert!(terminal.bracketed_paste());
    assert!(terminal.is_alternate_screen());

    let actions = parser.parse_as_vec(b"\x1bc");
    for action in actions {
        let _changes = terminal.action_to_changes(action);
    }
    assert!(!terminal.application_cursor_keys());
    assert!(terminal.cursor_visible());
    assert!(!terminal.bracketed_paste());
    assert!(!terminal.is_alternate_screen());
}

// ---- Scrollback capture: implicit scroll via line wrap ----

fn first_scrollback_text(terminal: &Terminal, index: usize) -> String {
    terminal
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
    let lines = terminal.surface().screen_lines();
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
    assert_eq!(terminal.surface().cursor_position(), (0, 5));
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
    assert_eq!(terminal.surface().cursor_position(), (0, 3));
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
    let lines = terminal.surface().screen_lines();
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
    assert_eq!(
        a.surface().cursor_position(),
        b.surface().cursor_position(),
        "{ctx}: cursor"
    );
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
    let (cols, rows) = t.surface().dimensions();
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
        server.surface().cursor_position(),
        mirror.surface().cursor_position(),
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
            (ai.text.as_str(), ai.fg.as_str(), ai.bg.as_str(), ai.intensity),
            (bi.text.as_str(), bi.fg.as_str(), bi.bg.as_str(), bi.intensity),
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
