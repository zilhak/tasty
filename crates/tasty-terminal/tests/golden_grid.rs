//! 실제 VTE ingest 결과를 텍스트·셀 속성 스냅샷과 비교한다. 커서·줄바꿈·스크롤 영역·
//! 소거·SGR을 검사하며 GPU 픽셀과 호스트 위젯 배치는 포함하지 않는다.
//! raw escape를 detached 터미널에 넣으므로 GUI나 실제 PTY 없이 실행한다.
//! CI의 headless 조합에서 자동 실행한다. check-headless는 전체 suite를 실행하지만,
//! default 조합의 --lib --bins 명령은 이 integration target을 실행하지 않는다.
//! 실행 범위는 docs/dev-guide/ci-gates.md 참조.
//!
//! 시각 검증 범위: docs/ai-verification/screenshot-methods.md#시각-판정-체크리스트.

use tasty_terminal::Terminal;

/// Render the visible grid as deterministic text: one line per row, trailing
/// spaces trimmed, blank rows preserved as empty lines (so the full row count
/// is pinned, not just the populated prefix).
fn grid_text(term: &Terminal) -> String {
    let (_, rows) = term.dimensions();
    (0..rows)
        .map(|r| term.screen_row(r, true))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Compact per-cell attribute dump for every populated cell, sorted by
/// `(row, col)`. One line per cell: `"{row},{col} {glyph} {flags}"`.
///
/// `flags` is a comma-joined, fixed-order list of the non-default attributes
/// (so the output is deterministic). A populated cell with no attributes shows
/// `-`. Cells that are an empty default space are skipped entirely.
fn styled_cells(term: &Terminal) -> String {
    let (_, rows) = term.dimensions();
    let mut lines = Vec::new();
    for r in 0..rows {
        for (c, info) in term.row_cells(r) {
            let mut flags = Vec::new();
            if info.intensity == "bold" {
                flags.push("bold".to_string());
            }
            if info.intensity == "half" {
                flags.push("faint".to_string());
            }
            if info.italic {
                flags.push("italic".to_string());
            }
            if info.underline {
                flags.push(format!("underline:{}", info.underline_style));
            }
            if info.inverse {
                flags.push("inverse".to_string());
            }
            if info.strikethrough {
                flags.push("strike".to_string());
            }
            if info.fg != "default" {
                flags.push(format!("fg={}", info.fg));
            }
            if info.bg != "default" {
                flags.push(format!("bg={}", info.bg));
            }

            // Skip blank, fully-default filler cells.
            if info.text == " " && flags.is_empty() {
                continue;
            }

            let flag_str = if flags.is_empty() {
                "-".to_string()
            } else {
                flags.join(",")
            };
            lines.push(format!("{r},{c} {} {flag_str}", info.text));
        }
    }
    lines.join("\n")
}

#[test]
fn golden_cursor_positioning_layout() {
    let mut t = Terminal::new_detached(12, 4);
    t.feed_bytes(b"\x1b[2J\x1b[H"); // clear + home
    t.feed_bytes(b"\x1b[1;1HABC"); // row 0, col 0
    t.feed_bytes(b"\x1b[2;4HDE"); // row 1, col 3
    t.feed_bytes(b"\x1b[4;1HZ"); // row 3, col 0

    let expected = "\
ABC
   DE

Z";
    assert_eq!(grid_text(&t), expected);
}

#[test]
fn golden_line_wrapping() {
    let mut t = Terminal::new_detached(10, 3);
    t.feed_bytes(b"0123456789abcd");

    let expected = "\
0123456789
abcd
";
    assert_eq!(grid_text(&t), expected);
}

#[test]
fn golden_scroll_region_scroll_up() {
    let mut t = Terminal::new_detached(8, 5);
    t.feed_bytes(b"\x1b[2J\x1b[H");
    t.feed_bytes(b"r0\r\nr1\r\nr2\r\nr3\r\nr4"); // rows 0..4

    // Limit scrolling to rows 3..5 (1-based) = rows 2..4 (0-based).
    t.feed_bytes(b"\x1b[3;5r");
    // Park on the bottom margin and emit LF → region scrolls up by one.
    t.feed_bytes(b"\x1b[5;1H\n");

    // Rows 0,1 untouched; old r2 evicted; r3→row2, r4→row3, row4 blank.
    let expected = "\
r0
r1
r3
r4
";
    assert_eq!(grid_text(&t), expected);
}

#[test]
fn golden_erase_display_below() {
    let mut t = Terminal::new_detached(8, 4);
    t.feed_bytes(b"\x1b[2J\x1b[H");
    t.feed_bytes(b"aaa\r\nbbb\r\nccc\r\nddd");
    t.feed_bytes(b"\x1b[2;1H"); // row 1, col 0
    t.feed_bytes(b"\x1b[J"); // erase from cursor to end of display

    let expected = "\
aaa


";
    assert_eq!(grid_text(&t), expected);
}

#[test]
fn golden_sgr_attributes() {
    let mut t = Terminal::new_detached(20, 1);
    t.feed_bytes(b"\x1b[2J\x1b[H");
    t.feed_bytes(b"\x1b[1mB"); // bold
    t.feed_bytes(b"\x1b[22m\x1b[3mI"); // bold off, italic
    t.feed_bytes(b"\x1b[23m\x1b[4mU"); // italic off, underline
    t.feed_bytes(b"\x1b[24m\x1b[7mR"); // underline off, inverse
    t.feed_bytes(b"\x1b[27m\x1b[9mS"); // inverse off, strikethrough
    t.feed_bytes(b"\x1b[0m\x1b[31mr"); // reset, red (palette 1) fg
    t.feed_bytes(b"\x1b[0mx"); // reset, plain

    let expected = "\
0,0 B bold
0,1 I italic
0,2 U underline:single
0,3 R inverse
0,4 S strike
0,5 r fg=palette:1
0,6 x -";
    assert_eq!(styled_cells(&t), expected);
}
