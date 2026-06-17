//! ED3 (`CSI 3J`, EraseScrollback) verification. Drives the REAL ingest path via
//! `Terminal::new_detached` + `feed_bytes` — NOT `test_helpers::TestTerminal`.
//!
//! ED3 erases scrollback history only; the visible screen is preserved (ED2
//! erases the screen). `clear` 류 명령은 보통 `\x1b[3J\x1b[2J` 를 함께 보낸다.

use tasty_terminal::Terminal;

/// Fill a 10x3 terminal so several lines spill into scrollback.
fn filled() -> Terminal {
    let mut t = Terminal::new_detached(10, 3);
    t.set_scrollback_limit(100_000);
    // 6 logical lines into a 3-row screen → 3 rows scroll off into scrollback.
    t.feed_bytes(b"A\r\nB\r\nC\r\nD\r\nE\r\nF");
    t
}

#[test]
fn ed3_clears_scrollback() {
    let mut t = filled();
    assert!(t.scrollback_len() > 0, "precondition: scrollback populated");

    t.feed_bytes(b"\x1b[3J");
    assert_eq!(t.scrollback_len(), 0, "ED3 must empty scrollback");
}

#[test]
fn ed3_preserves_visible_screen() {
    let mut t = filled();
    let before: Vec<String> = (0..3).map(|r| t.screen_row(r)).collect();

    t.feed_bytes(b"\x1b[3J");
    let after: Vec<String> = (0..3).map(|r| t.screen_row(r)).collect();

    assert_eq!(before, after, "ED3 must not touch the visible screen");
}

#[test]
fn ed3_resets_scroll_offset() {
    let mut t = filled();
    // User scrolls up into history.
    t.scroll_up(2);
    assert!(t.scroll_offset() > 0, "precondition: scrolled up");

    t.feed_bytes(b"\x1b[3J");
    assert_eq!(
        t.scroll_offset(),
        0,
        "viewport must snap back to live when history is erased"
    );
}

#[test]
fn ed3_scrollback_reaccumulates_cleanly() {
    let mut t = filled();
    t.feed_bytes(b"\x1b[3J");
    assert_eq!(t.scrollback_len(), 0);

    // Feeding more lines must start accumulating scrollback from zero again,
    // proving the post-clear bookkeeping (scroll_offset / line tails) is intact.
    t.feed_bytes(b"\r\nG\r\nH\r\nI\r\nJ");
    assert!(
        t.scrollback_len() > 0,
        "scrollback must re-accumulate after ED3"
    );
}
