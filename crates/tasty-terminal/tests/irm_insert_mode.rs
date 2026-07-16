//! IRM (Insert/Replace Mode, standard mode 4) integration tests.
//!
//! Uses the production `Terminal::new_detached` + `feed_bytes` path (no
//! `TestTerminal`), so the SM/RM dispatch, `handle_mode` standard-mode arm, and
//! the insert-mode Print shift are all exercised end to end.

use tasty_terminal::Terminal;

#[test]
fn irm_on_inserts_and_shifts_right() {
    let mut t = Terminal::new_detached(80, 24);
    t.feed_bytes(b"ABC"); // "ABC", cursor at col3
    t.feed_bytes(b"\x1b[1G"); // cursor to col0 (over 'A')
    t.feed_bytes(b"\x1b[4h"); // IRM on (standard SM 4)
    t.feed_bytes(b"X"); // insert: X pushes ABC right
    assert_eq!(t.screen_row(0, true), "XABC");
}

#[test]
fn irm_off_overwrites() {
    let mut t = Terminal::new_detached(80, 24);
    t.feed_bytes(b"ABC\x1b[1G");
    t.feed_bytes(b"\x1b[4l"); // IRM off (default)
    t.feed_bytes(b"X");
    assert_eq!(t.screen_row(0, true), "XBC"); // overwrite
}

#[test]
fn irm_default_is_overwrite() {
    // No SM/RM at all — default mode must overwrite, no regression.
    let mut t = Terminal::new_detached(80, 24);
    t.feed_bytes(b"ABC\x1b[1G");
    t.feed_bytes(b"X");
    assert_eq!(t.screen_row(0, true), "XBC");
}

#[test]
fn irm_multiple_inserts_accumulate() {
    let mut t = Terminal::new_detached(80, 24);
    t.feed_bytes(b"ABC\x1b[1G");
    t.feed_bytes(b"\x1b[4h");
    t.feed_bytes(b"XY"); // PrintString path: shift by total width
    assert_eq!(t.screen_row(0, true), "XYABC");
}

#[test]
fn irm_reset_back_to_overwrite() {
    let mut t = Terminal::new_detached(80, 24);
    t.feed_bytes(b"ABC\x1b[1G");
    t.feed_bytes(b"\x1b[4h"); // on
    t.feed_bytes(b"X"); // -> "XABC", cursor at col1
    t.feed_bytes(b"\x1b[4l"); // off
    t.feed_bytes(b"Z"); // overwrite the 'A' now at col1
    assert_eq!(t.screen_row(0, true), "XZBC");
}

#[test]
fn irm_wide_grapheme_insert() {
    // CJK width-2 glyph inserted in insert mode shifts by 2 columns.
    let mut t = Terminal::new_detached(80, 24);
    t.feed_bytes(b"ABC\x1b[1G");
    t.feed_bytes(b"\x1b[4h");
    t.feed_bytes("가".as_bytes()); // width-2
    assert_eq!(t.screen_row(0, true), "가ABC");
}

#[test]
fn irm_reset_by_full_reset() {
    // RIS (ESC c) must clear insert_mode.
    let mut t = Terminal::new_detached(80, 24);
    t.feed_bytes(b"\x1b[4h"); // IRM on
    t.feed_bytes(b"\x1bc"); // RIS full reset
    t.feed_bytes(b"ABC\x1b[1G");
    t.feed_bytes(b"X");
    assert_eq!(t.screen_row(0, true), "XBC"); // back to overwrite
}

#[test]
fn standard_show_cursor_mode_25() {
    // Standard mode 25 mirrors DEC private 25 (DECTCEM).
    let mut t = Terminal::new_detached(80, 24);
    assert!(t.cursor_visible());
    t.feed_bytes(b"\x1b[25l"); // hide
    assert!(!t.cursor_visible());
    t.feed_bytes(b"\x1b[25h"); // show
    assert!(t.cursor_visible());
}
