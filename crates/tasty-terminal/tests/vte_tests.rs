/// Integration tests for VTE processing: input, deletion, editing.
/// Uses TestTerminal (no PTY) for fast, deterministic tests.

use tasty_terminal::test_helpers::TestTerminal;

// ============================================================
// Basic text input
// ============================================================

#[test]
fn type_hello() {
    let mut t = TestTerminal::new(80, 24);
    t.feed_str("hello");
    assert_eq!(t.row(0), "hello");
}

#[test]
fn type_multiple_words() {
    let mut t = TestTerminal::new(80, 24);
    t.feed_str("hello world");
    assert_eq!(t.row(0), "hello world");
}

#[test]
fn type_with_newline() {
    let mut t = TestTerminal::new(80, 24);
    t.feed_str("line1\r\nline2");
    assert_eq!(t.row(0), "line1");
    assert_eq!(t.row(1), "line2");
}

#[test]
fn carriage_return_overwrites() {
    let mut t = TestTerminal::new(80, 24);
    t.feed_str("abcdef\rXY");
    assert_eq!(t.row(0), "XYcdef");
}

// ============================================================
// Backspace (the actual bug reported)
// ============================================================

#[test]
fn backspace_moves_cursor_left() {
    let mut t = TestTerminal::new(80, 24);
    // Shell typically sends: "abc" then BS+space+BS to erase 'c'
    t.feed_str("abc\x08 \x08");
    // After: cursor was at 3, BS moves to 2, space writes ' ' at 2 (cursor now 3),
    // BS moves back to 2. Result: "ab " with cursor at 2.
    // But visually it's "ab" (the space replaced 'c')
    assert_eq!(t.row(0), "ab");
}

#[test]
fn backspace_at_start_of_line_stays() {
    let mut t = TestTerminal::new(80, 24);
    t.feed_str("\x08\x08hello");
    // Backspace at position 0 should do nothing
    assert_eq!(t.row(0), "hello");
}

#[test]
fn backspace_shell_erase_pattern() {
    let mut t = TestTerminal::new(80, 24);
    // Simulate typing "helo" then pressing backspace and typing "lo"
    // Shell sends: "helo" + BS+SP+BS + "lo"
    t.feed_str("helo\x08 \x08lo");
    assert_eq!(t.row(0), "hello");
}

#[test]
fn multiple_backspace_erase() {
    let mut t = TestTerminal::new(80, 24);
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
    let mut t = TestTerminal::new(80, 24);
    t.feed_str("ab");
    t.feed(b"\x1b[2C"); // move right 2
    t.feed_str("X");
    assert_eq!(t.row(0), "ab  X");
}

#[test]
fn cursor_move_left() {
    let mut t = TestTerminal::new(80, 24);
    t.feed_str("abcde");
    t.feed(b"\x1b[3D"); // move left 3
    t.feed_str("X");
    assert_eq!(t.row(0), "abXde");
}

#[test]
fn cursor_absolute_position() {
    let mut t = TestTerminal::new(80, 24);
    t.feed(b"\x1b[3;5H"); // row 3, col 5 (1-based)
    t.feed_str("X");
    assert_eq!(t.row(2), "    X"); // row 2 (0-based), col 4 (0-based)
}

#[test]
fn cursor_column_absolute() {
    let mut t = TestTerminal::new(80, 24);
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
    let mut t = TestTerminal::new(80, 24);
    t.feed_str("hello world");
    t.feed(b"\x1b[6G"); // move to column 6
    t.feed(b"\x1b[K");   // erase to end of line
    assert_eq!(t.row(0), "hello");
}

#[test]
fn erase_entire_display() {
    let mut t = TestTerminal::new(80, 24);
    t.feed_str("line1\r\nline2\r\nline3");
    t.feed(b"\x1b[2J"); // erase display
    assert_eq!(t.row(0), "");
    assert_eq!(t.row(1), "");
    assert_eq!(t.row(2), "");
}

#[test]
fn erase_to_end_of_display() {
    let mut t = TestTerminal::new(80, 24);
    t.feed_str("line1\r\nline2\r\nline3");
    t.feed(b"\x1b[2;1H"); // go to row 2, col 1
    t.feed(b"\x1b[J");     // erase to end of display
    assert_eq!(t.row(0), "line1");
    assert_eq!(t.row(1), "");
    assert_eq!(t.row(2), "");
}

// ============================================================
// Overwrite (CR + new text)
// ============================================================

#[test]
fn overwrite_line() {
    let mut t = TestTerminal::new(80, 24);
    t.feed_str("old text\rnew");
    assert_eq!(t.row(0), "new text");
}

#[test]
fn overwrite_with_erase() {
    let mut t = TestTerminal::new(80, 24);
    t.feed_str("old text\r\x1b[Knew text");
    assert_eq!(t.row(0), "new text");
}

// ============================================================
// SGR (colors/attributes) — verify they don't break text
// ============================================================

#[test]
fn sgr_colored_text() {
    let mut t = TestTerminal::new(80, 24);
    t.feed(b"\x1b[31mred\x1b[0m normal");
    assert_eq!(t.row(0), "red normal");
}

#[test]
fn sgr_bold_text() {
    let mut t = TestTerminal::new(80, 24);
    t.feed(b"\x1b[1mbold\x1b[0m");
    assert_eq!(t.row(0), "bold");
}

// ============================================================
// Alternate screen
// ============================================================

#[test]
fn alternate_screen_switch() {
    let mut t = TestTerminal::new(80, 24);
    t.feed_str("main screen");
    assert_eq!(t.row(0), "main screen");

    // Enter alternate screen
    t.feed(b"\x1b[?1049h");
    assert!(t.use_alternate);
    assert_eq!(t.row(0), ""); // alternate is empty

    t.feed_str("alt screen");
    assert_eq!(t.row(0), "alt screen");

    // Leave alternate screen
    t.feed(b"\x1b[?1049l");
    assert!(!t.use_alternate);
    assert_eq!(t.row(0), "main screen"); // original content restored
}

// ============================================================
// Bracketed paste
// ============================================================

#[test]
fn bracketed_paste_mode() {
    let mut t = TestTerminal::new(80, 24);
    assert!(!t.bracketed_paste);

    t.feed(b"\x1b[?2004h"); // enable
    assert!(t.bracketed_paste);

    t.feed(b"\x1b[?2004l"); // disable
    assert!(!t.bracketed_paste);
}

// ============================================================
// Application cursor keys
// ============================================================

#[test]
fn application_cursor_keys_mode() {
    let mut t = TestTerminal::new(80, 24);
    assert!(!t.application_cursor_keys);

    t.feed(b"\x1b[?1h"); // enable DECCKM
    assert!(t.application_cursor_keys);

    t.feed(b"\x1b[?1l"); // disable
    assert!(!t.application_cursor_keys);
}

// ============================================================
// Full reset
// ============================================================

#[test]
fn full_reset() {
    let mut t = TestTerminal::new(80, 24);
    t.feed_str("some text");
    t.feed(b"\x1b[?1h");    // enable DECCKM
    t.feed(b"\x1b[?2004h"); // enable bracketed paste
    t.feed(b"\x1bc");       // RIS (full reset)

    assert!(!t.application_cursor_keys);
    assert!(!t.bracketed_paste);
    assert_eq!(t.row(0), ""); // screen cleared
}

// ============================================================
// Line wrapping
// ============================================================

#[test]
fn line_wrapping() {
    let mut t = TestTerminal::new(10, 24);
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
    let mut t = TestTerminal::new(80, 24);
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
    let mut t = TestTerminal::new(80, 24);
    t.feed(b"");
    assert_eq!(t.row(0), "");
}

#[test]
fn only_newlines() {
    let mut t = TestTerminal::new(80, 24);
    t.feed_str("\r\n\r\n\r\n");
    assert_eq!(t.row(0), "");
    assert_eq!(t.row(1), "");
    assert_eq!(t.row(2), "");
}

#[test]
fn cursor_up_from_top() {
    let mut t = TestTerminal::new(80, 24);
    t.feed(b"\x1b[10A"); // move up 10 from row 0 — should clamp
    t.feed_str("X");
    assert_eq!(t.row(0), "X"); // still on row 0
}

#[test]
fn unicode_text() {
    let mut t = TestTerminal::new(80, 24);
    t.feed_str("한글 테스트");
    let row = t.row(0);
    assert!(row.contains("한글"));
    assert!(row.contains("테스트"));
}

#[test]
fn mixed_ascii_and_escape() {
    let mut t = TestTerminal::new(80, 24);
    // Simulate a colorized prompt: "\x1b[32m$ \x1b[0mhello"
    t.feed(b"\x1b[32m$ \x1b[0mhello");
    assert_eq!(t.row(0), "$ hello");
}
