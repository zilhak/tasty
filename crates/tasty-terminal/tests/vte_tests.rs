/// Integration tests for VTE processing: input, deletion, editing.
///
/// These drive the **production** ingest path: a detached `Terminal`
/// (`Terminal::new_detached`) feeds bytes straight into `TerminalState::ingest`
/// — the same handlers used with a real PTY — with no PTY, child, or threads.
/// This is fast and deterministic while still exercising the real VTE dispatch
/// (so OSC/CSI/SGR handlers are validated, not a copy).
use std::sync::mpsc::{self, Receiver};

use tasty_terminal::{Terminal, TerminalEvent, TerminalEventKind};

/// Thin harness over the production detached terminal. Captures query responses
/// (DSR / DA / cursor position report) via the input sink.
struct Term {
    term: Terminal,
    input_rx: Receiver<Vec<u8>>,
}

impl Term {
    fn new(cols: usize, rows: usize) -> Self {
        let mut term = Terminal::new_detached(cols, rows);
        let (tx, input_rx) = mpsc::channel();
        term.set_input_sink(tx);
        Self { term, input_rx }
    }

    /// Feed raw bytes through the real ingest path.
    fn feed(&mut self, data: &[u8]) {
        self.term.feed_bytes(data);
    }

    /// Feed a string.
    fn feed_str(&mut self, s: &str) {
        self.term.feed_bytes(s.as_bytes());
    }

    /// Text content of a row (0-indexed), trailing spaces trimmed.
    fn row(&self, row: usize) -> String {
        self.term.screen_row(row)
    }

    /// All bytes the terminal wrote back to its input sink (query responses).
    fn sent_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        while let Ok(chunk) = self.input_rx.try_recv() {
            out.extend_from_slice(&chunk);
        }
        out
    }

    fn take_events(&mut self) -> Vec<TerminalEvent> {
        self.term.take_events()
    }

    fn is_alternate(&self) -> bool {
        self.term.is_alternate_screen()
    }

    fn bracketed_paste(&self) -> bool {
        self.term.bracketed_paste()
    }

    fn application_cursor_keys(&self) -> bool {
        self.term.application_cursor_keys()
    }

    fn synchronized_output(&self) -> bool {
        self.term.synchronized_output()
    }
}

// ============================================================
// Basic text input
// ============================================================

#[test]
fn type_hello() {
    let mut t = Term::new(80, 24);
    t.feed_str("hello");
    assert_eq!(t.row(0), "hello");
}

#[test]
fn type_multiple_words() {
    let mut t = Term::new(80, 24);
    t.feed_str("hello world");
    assert_eq!(t.row(0), "hello world");
}

#[test]
fn type_with_newline() {
    let mut t = Term::new(80, 24);
    t.feed_str("line1\r\nline2");
    assert_eq!(t.row(0), "line1");
    assert_eq!(t.row(1), "line2");
}

#[test]
fn carriage_return_overwrites() {
    let mut t = Term::new(80, 24);
    t.feed_str("abcdef\rXY");
    assert_eq!(t.row(0), "XYcdef");
}

// ============================================================
// Backspace (the actual bug reported)
// ============================================================

#[test]
fn backspace_moves_cursor_left() {
    let mut t = Term::new(80, 24);
    // Shell typically sends: "abc" then BS+space+BS to erase 'c'
    t.feed_str("abc\x08 \x08");
    // After: cursor was at 3, BS moves to 2, space writes ' ' at 2 (cursor now 3),
    // BS moves back to 2. Result: "ab " with cursor at 2.
    // But visually it's "ab" (the space replaced 'c')
    assert_eq!(t.row(0), "ab");
}

#[test]
fn backspace_at_start_of_line_stays() {
    let mut t = Term::new(80, 24);
    t.feed_str("\x08\x08hello");
    // Backspace at position 0 should do nothing
    assert_eq!(t.row(0), "hello");
}

#[test]
fn backspace_shell_erase_pattern() {
    let mut t = Term::new(80, 24);
    // Simulate typing "helo" then pressing backspace and typing "lo"
    // Shell sends: "helo" + BS+SP+BS + "lo"
    t.feed_str("helo\x08 \x08lo");
    assert_eq!(t.row(0), "hello");
}

#[test]
fn multiple_backspace_erase() {
    let mut t = Term::new(80, 24);
    // Type "abcde" then erase last 3 characters
    t.feed_str("abcde");
    // Three BS+SP+BS sequences
    t.feed_str("\x08 \x08\x08 \x08\x08 \x08");
    assert_eq!(t.row(0), "ab");
}

// ============================================================
// Cursor movement (CSI sequences)
// ============================================================

#[test]
fn cursor_move_right() {
    let mut t = Term::new(80, 24);
    t.feed_str("ab");
    t.feed(b"\x1b[2C"); // move right 2
    t.feed_str("X");
    assert_eq!(t.row(0), "ab  X");
}

#[test]
fn cursor_move_left() {
    let mut t = Term::new(80, 24);
    t.feed_str("abcde");
    t.feed(b"\x1b[3D"); // move left 3
    t.feed_str("X");
    assert_eq!(t.row(0), "abXde");
}

#[test]
fn cursor_absolute_position() {
    let mut t = Term::new(80, 24);
    t.feed(b"\x1b[3;5H"); // row 3, col 5 (1-based)
    t.feed_str("X");
    assert_eq!(t.row(2), "    X"); // row 2 (0-based), col 4 (0-based)
}

#[test]
fn cursor_column_absolute() {
    let mut t = Term::new(80, 24);
    t.feed_str("0123456789");
    t.feed(b"\x1b[6G"); // column 6 (1-based) = index 5
    t.feed_str("X");
    assert_eq!(t.row(0), "01234X6789");
}

// ============================================================
// Erase operations
// ============================================================

#[test]
fn erase_to_end_of_line() {
    let mut t = Term::new(80, 24);
    t.feed_str("hello world");
    t.feed(b"\x1b[6G"); // move to column 6
    t.feed(b"\x1b[K"); // erase to end of line
    assert_eq!(t.row(0), "hello");
}

#[test]
fn erase_entire_display() {
    let mut t = Term::new(80, 24);
    t.feed_str("line1\r\nline2\r\nline3");
    t.feed(b"\x1b[2J"); // erase display
    assert_eq!(t.row(0), "");
    assert_eq!(t.row(1), "");
    assert_eq!(t.row(2), "");
}

#[test]
fn erase_to_end_of_display() {
    let mut t = Term::new(80, 24);
    t.feed_str("line1\r\nline2\r\nline3");
    t.feed(b"\x1b[2;1H"); // go to row 2, col 1
    t.feed(b"\x1b[J"); // erase to end of display
    assert_eq!(t.row(0), "line1");
    assert_eq!(t.row(1), "");
    assert_eq!(t.row(2), "");
}

// ============================================================
// Overwrite (CR + new text)
// ============================================================

#[test]
fn overwrite_line() {
    let mut t = Term::new(80, 24);
    t.feed_str("old text\rnew");
    assert_eq!(t.row(0), "new text");
}

#[test]
fn overwrite_with_erase() {
    let mut t = Term::new(80, 24);
    t.feed_str("old text\r\x1b[Knew text");
    assert_eq!(t.row(0), "new text");
}

// ============================================================
// SGR (colors/attributes) — verify they don't break text
// ============================================================

#[test]
fn sgr_colored_text() {
    let mut t = Term::new(80, 24);
    t.feed(b"\x1b[31mred\x1b[0m normal");
    assert_eq!(t.row(0), "red normal");
}

#[test]
fn sgr_bold_text() {
    let mut t = Term::new(80, 24);
    t.feed(b"\x1b[1mbold\x1b[0m");
    assert_eq!(t.row(0), "bold");
}

// ============================================================
// Alternate screen
// ============================================================

#[test]
fn alternate_screen_switch() {
    let mut t = Term::new(80, 24);
    t.feed_str("main screen");
    assert_eq!(t.row(0), "main screen");

    // Enter alternate screen
    t.feed(b"\x1b[?1049h");
    assert!(t.is_alternate());
    assert_eq!(t.row(0), ""); // alternate is empty

    t.feed_str("alt screen");
    assert_eq!(t.row(0), "alt screen");

    // Leave alternate screen
    t.feed(b"\x1b[?1049l");
    assert!(!t.is_alternate());
    assert_eq!(t.row(0), "main screen"); // original content restored
}

// ============================================================
// Bracketed paste
// ============================================================

#[test]
fn bracketed_paste_mode() {
    let mut t = Term::new(80, 24);
    assert!(!t.bracketed_paste());

    t.feed(b"\x1b[?2004h"); // enable
    assert!(t.bracketed_paste());

    t.feed(b"\x1b[?2004l"); // disable
    assert!(!t.bracketed_paste());
}

// ============================================================
// Application cursor keys
// ============================================================

#[test]
fn application_cursor_keys_mode() {
    let mut t = Term::new(80, 24);
    assert!(!t.application_cursor_keys());

    t.feed(b"\x1b[?1h"); // enable DECCKM
    assert!(t.application_cursor_keys());

    t.feed(b"\x1b[?1l"); // disable
    assert!(!t.application_cursor_keys());
}

// ============================================================
// Synchronized output
// ============================================================

#[test]
fn synchronized_output_applies_immediately() {
    let mut t = Term::new(80, 24);

    // Changes are applied immediately even during sync output,
    // so cursor_position() stays current for VTE operations that
    // depend on it (EraseLine, DeleteLine, etc.).
    t.feed(b"\x1b[?2026habc\rXY");
    assert_eq!(t.row(0), "XYc");
    assert!(t.synchronized_output());

    t.feed(b"\x1b[?2026l");
    assert_eq!(t.row(0), "XYc");
    assert!(!t.synchronized_output());
}

// ============================================================
// Device status / cursor position report
// ============================================================

#[test]
fn cursor_position_report_responds_with_one_based_coordinates() {
    let mut t = Term::new(80, 24);
    t.feed(b"\x1b[3;5H");
    t.feed(b"\x1b[6n");
    assert_eq!(String::from_utf8_lossy(&t.sent_bytes()), "\x1b[3;5R");
}

#[test]
fn status_report_returns_terminal_ok() {
    let mut t = Term::new(80, 24);
    t.feed(b"\x1b[5n");
    assert_eq!(String::from_utf8_lossy(&t.sent_bytes()), "\x1b[0n");
}

// ============================================================
// OSC handling — proves the production ingest path runs the real
// OSC handler (the old duplicate harness dropped all OSC sequences,
// so this regression guard could not have passed before unification).
// ============================================================

#[test]
fn osc_set_window_title_emits_event() {
    let mut t = Term::new(80, 24);
    t.feed(b"\x1b]2;hi\x07"); // OSC 2 — set window title
    let events = t.take_events();
    assert!(
        events
            .iter()
            .any(|e| matches!(&e.kind, TerminalEventKind::TitleChanged(s) if s == "hi")),
        "expected TitleChanged(\"hi\") from production OSC handler, got none"
    );
}

// ============================================================
// Full reset
// ============================================================

#[test]
fn full_reset() {
    let mut t = Term::new(80, 24);
    t.feed_str("some text");
    t.feed(b"\x1b[?1h"); // enable DECCKM
    t.feed(b"\x1b[?2004h"); // enable bracketed paste
    t.feed(b"\x1bc"); // RIS (full reset)

    assert!(!t.application_cursor_keys());
    assert!(!t.bracketed_paste());
    assert_eq!(t.row(0), ""); // screen cleared
}

// ============================================================
// Line wrapping
// ============================================================

#[test]
fn line_wrapping() {
    let mut t = Term::new(10, 24);
    t.feed_str("0123456789wrap");
    // "0123456789" fills row 0, "wrap" goes to row 1
    assert_eq!(t.row(0), "0123456789");
    assert_eq!(t.row(1), "wrap");
}

// ============================================================
// Tab character
// ============================================================

#[test]
fn tab_character() {
    let mut t = Term::new(80, 24);
    t.feed_str("a\tb");
    let row = t.row(0);
    // Tab should advance cursor, 'a' and 'b' should both be present
    assert!(row.starts_with("a"));
    assert!(row.contains("b"));
}

// ============================================================
// Edge cases
// ============================================================

#[test]
fn empty_input() {
    let mut t = Term::new(80, 24);
    t.feed(b"");
    assert_eq!(t.row(0), "");
}

#[test]
fn only_newlines() {
    let mut t = Term::new(80, 24);
    t.feed_str("\r\n\r\n\r\n");
    assert_eq!(t.row(0), "");
    assert_eq!(t.row(1), "");
    assert_eq!(t.row(2), "");
}

#[test]
fn cursor_up_from_top() {
    let mut t = Term::new(80, 24);
    t.feed(b"\x1b[10A"); // move up 10 from row 0 — should clamp
    t.feed_str("X");
    assert_eq!(t.row(0), "X"); // still on row 0
}

#[test]
fn unicode_text() {
    let mut t = Term::new(80, 24);
    t.feed_str("한글 테스트");
    let row = t.row(0);
    assert!(row.contains("한글"));
    assert!(row.contains("테스트"));
}

#[test]
fn mixed_ascii_and_escape() {
    let mut t = Term::new(80, 24);
    // Simulate a colorized prompt: "\x1b[32m$ \x1b[0mhello"
    t.feed(b"\x1b[32m$ \x1b[0mhello");
    assert_eq!(t.row(0), "$ hello");
}
