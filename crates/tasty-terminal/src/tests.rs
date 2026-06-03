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
