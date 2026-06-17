//! Scrollback-capture integrity verification (E1 width-mismatch, E2 partial-region
//! over-scroll). Drives the REAL ingest path via `Terminal::new_detached` +
//! `feed_bytes`, so the production VTE handlers are exercised directly. Oracles:
//!   A) "tall screen" ground truth: feed the same bytes into a screen tall enough
//!      that nothing scrolls, so capture logic never fires and the pure termwiz
//!      layout is the oracle. subject(scrollback ++ visible) must equal it.
//!   B) marker uniqueness: every unique marker appears exactly once across
//!      (scrollback ∪ visible). 0 = loss, >1 = duplication.
//!
//! This file only ADDS a test; it never modifies production code. Bugs found are
//! reported, not fixed.

use tasty_terminal::Terminal;

// ── helpers ──────────────────────────────────────────────────────────────────

/// Text of one scrollback line (cells joined, trailing blanks trimmed).
fn scrollback_row_text(t: &Terminal, i: usize) -> String {
    let cells = t.scrollback_line_owned(i).unwrap_or_default();
    let mut s = String::new();
    for (c, _) in &cells {
        s.push_str(c);
    }
    s.trim_end().to_string()
}

/// subject reconstruction: all scrollback rows (oldest→newest) followed by all
/// visible screen rows, with trailing empty rows trimmed.
fn reconstruct(t: &Terminal) -> Vec<String> {
    let mut rows = Vec::new();
    for i in 0..t.scrollback_len() {
        rows.push(scrollback_row_text(t, i));
    }
    let (_cols, nrows) = t.dimensions();
    for r in 0..nrows {
        rows.push(t.screen_row(r)); // screen_row already trim_end's
    }
    trim_trailing_empty(&mut rows);
    rows
}

/// Just the visible rows of a terminal (trailing empties trimmed) — used for the
/// tall reference where nothing scrolled off.
fn visible_rows(t: &Terminal) -> Vec<String> {
    let (_cols, nrows) = t.dimensions();
    let mut rows: Vec<String> = (0..nrows).map(|r| t.screen_row(r)).collect();
    trim_trailing_empty(&mut rows);
    rows
}

fn trim_trailing_empty(rows: &mut Vec<String>) {
    while rows.last().map(|s| s.is_empty()).unwrap_or(false) {
        rows.pop();
    }
}

/// Whole buffer as joined text for marker counting (scrollback ++ visible).
fn buffer_text(t: &Terminal) -> String {
    reconstruct(t).join("\n")
}

fn dump_rows(label: &str, rows: &[String]) {
    eprintln!("--- {label} ({} rows) ---", rows.len());
    for (i, r) in rows.iter().enumerate() {
        eprintln!("  [{i:2}] {:?}", r);
    }
}

/// Oracle A: feed `bytes` to a short screen (subject) and a tall screen (ref);
/// subject's reconstruct must match ref's visible rows. Returns Ok or a dump.
fn oracle_a(name: &str, cols: usize, h_small: usize, h_tall: usize, bytes: &[u8]) -> bool {
    let mut subject = Terminal::new_detached(cols, h_small);
    subject.set_scrollback_limit(100_000);
    subject.feed_bytes(bytes);

    let mut reference = Terminal::new_detached(cols, h_tall);
    reference.set_scrollback_limit(100_000);
    reference.feed_bytes(bytes);

    let got = reconstruct(&subject);
    let want = visible_rows(&reference);

    // The tall reference must itself not have scrolled (sanity: oracle validity).
    if reference.scrollback_len() != 0 {
        eprintln!(
            "[{name}] ORACLE INVALID: tall ref scrolled ({} lines) — raise h_tall",
            reference.scrollback_len()
        );
        return false;
    }

    if got == want {
        eprintln!("[{name}] PASS  ({} rows, subject scrollback={})", got.len(), subject.scrollback_len());
        true
    } else {
        eprintln!("[{name}] FAIL: subject(scrollback++visible) != tall reference");
        dump_rows("subject_full", &got);
        dump_rows("ref_full", &want);
        // First diverging row, char/byte detail.
        let n = got.len().max(want.len());
        for i in 0..n {
            let a = got.get(i).map(String::as_str).unwrap_or("<none>");
            let b = want.get(i).map(String::as_str).unwrap_or("<none>");
            if a != b {
                eprintln!("  first divergence at row {i}:");
                eprintln!("    subject = {a:?}  bytes={:02x?}", a.as_bytes());
                eprintln!("    ref     = {b:?}  bytes={:02x?}", b.as_bytes());
                break;
            }
        }
        false
    }
}

/// Oracle B: each marker in `markers` must appear exactly once in (scrollback ∪
/// visible). Reports duplicates/losses.
fn oracle_b(name: &str, t: &Terminal, markers: &[&str]) -> bool {
    let buf = buffer_text(t);
    let mut ok = true;
    for m in markers {
        let c = buf.matches(m).count();
        if c != 1 {
            ok = false;
            eprintln!(
                "[{name}] marker {m:?} count={c} ({})",
                if c == 0 { "LOST" } else { "DUPLICATED" }
            );
        }
    }
    if ok {
        eprintln!("[{name}] PASS  (all {} markers exactly once)", markers.len());
    } else {
        eprintln!("[{name}] FAIL — buffer dump:");
        dump_rows("buffer", &reconstruct(t));
    }
    ok
}

// ── E1 root-cause characterization: the two width functions ───────────────────

/// Directly compare the two width sources that the capture path and termwiz use,
/// for a set of tricky graphemes. Prints any disagreement. This does not assert
/// (it characterizes the divergence regardless of whether capture misaligns).
#[test]
fn e1_width_function_divergence_report() {
    use termwiz::cell::{grapheme_column_width, Cell, CellAttributes};

    // (label, grapheme)
    let cases: &[(&str, &str)] = &[
        ("ascii a", "a"),
        ("CJK 한", "한"),
        ("CJK 글", "글"),
        ("emoji family", "👨\u{200d}👩\u{200d}👧\u{200d}👦"),
        ("flag KR", "🇰🇷"),
        ("e+combining acute", "e\u{0301}"),
        ("a+two combining", "a\u{0301}\u{0302}"),
        ("zero-width joiner alone", "\u{200d}"),
        ("variation selector", "❤\u{fe0f}"),
        ("skin tone", "👍\u{1f3fd}"),
    ];

    eprintln!("=== E1 width-function comparison (capture vs termwiz print_text) ===");
    eprintln!("  grapheme_column_width(g,None).max(1)  vs  Cell::new_grapheme(g).width()");
    let mut divergences = 0;
    for (label, g) in cases {
        let capture_w = grapheme_column_width(g, None).max(1);
        let cell = Cell::new_grapheme(g, CellAttributes::default(), None);
        let cell_w = cell.width();
        let mark = if capture_w == cell_w { "" } else { "  <-- DIVERGE" };
        if capture_w != cell_w {
            divergences += 1;
        }
        eprintln!(
            "  {label:24} capture={capture_w} cell={cell_w} (cell.str={:?}){mark}",
            cell.str()
        );
    }
    eprintln!("=== total width divergences: {divergences} ===");
}

// ── Scenarios ─────────────────────────────────────────────────────────────────

const COLS: usize = 10;
const H_SMALL: usize = 4;
const H_TALL: usize = 64;

/// S0 — ASCII sanity. 8 hard-newline lines into H=4. Trust the harness first.
#[test]
fn s0_ascii_sanity() {
    let bytes = b"L0\r\nL1\r\nL2\r\nL3\r\nL4\r\nL5\r\nL6\r\nL7";
    assert!(oracle_a("S0", COLS, H_SMALL, H_TALL, bytes), "S0 must pass");
}

/// S1 — CJK width-2 wrapping driving a bottom-row wrap-scroll.
#[test]
fn s1_cjk_wrap() {
    // cols=10 → 5 CJK per row. Feed many → forces wrap-scroll at bottom.
    let mut s = String::new();
    for _ in 0..16 {
        s.push_str("한글한글한"); // 5 wide = 10 cols exactly
    }
    let pass = oracle_a("S1", COLS, H_SMALL, H_TALL, s.as_bytes());
    assert!(pass, "S1 CJK wrap diverged — see dump");
}

/// S2 — emoji / ZWJ / combining at wrap boundary + bottom.
#[test]
fn s2_emoji_zwj() {
    let mut s = String::new();
    // Mix family-emoji, flag, combining e, plain ascii to fill rows and wrap.
    for i in 0..12 {
        s.push_str(&format!("{i}"));
        s.push_str("👨\u{200d}👩\u{200d}👧\u{200d}👦"); // family
        s.push_str("🇰🇷"); // flag
        s.push_str("e\u{0301}"); // combining
        s.push_str("XY");
    }
    let pass = oracle_a("S2", COLS, H_SMALL, H_TALL, s.as_bytes());
    assert!(pass, "S2 emoji/ZWJ diverged — see dump");
}

/// S3 — dense combining marks + zero-width, wrap-scrolled.
#[test]
fn s3_combining_zero_width() {
    let mut s = String::new();
    for i in 0..20 {
        s.push_str(&format!("{}", i % 10));
        s.push_str("a\u{0301}\u{0302}"); // a + two combining
        s.push_str("o\u{0308}"); // o + diaeresis
        s.push('\u{200b}'); // zero-width space
        s.push_str("bc");
    }
    let pass = oracle_a("S3", COLS, H_SMALL, H_TALL, s.as_bytes());
    assert!(pass, "S3 combining/zero-width diverged — see dump");
}

/// S4 — mixed ASCII+CJK where wrap lands exactly on / one past the column edge.
#[test]
fn s4_mixed_width_boundary() {
    // Case A: ascii then CJK so a wide char would straddle the right edge.
    let mut a = String::new();
    for _ in 0..10 {
        a.push_str("abc한글def한"); // 3 + 4(2 CJK) + 3 + 2 = 12 cols → wraps mid
    }
    let pass_a = oracle_a("S4a", COLS, H_SMALL, H_TALL, a.as_bytes());

    // Case B: exact boundary — 9 ascii then 1 CJK (col 10 occupied by wide -> push)
    let mut b = String::new();
    for _ in 0..10 {
        b.push_str("123456789한"); // 9 ascii + 1 wide (needs col10-11) -> wraps
    }
    let pass_b = oracle_a("S4b", COLS, H_SMALL, H_TALL, b.as_bytes());

    assert!(pass_a && pass_b, "S4 mixed-width boundary diverged — see dump");
}

/// S5 — ⚠️E2: partial top-anchored region, scroll_count > region_size.
/// H=6, fill L00..L05, set margins rows1..4 (region [0,3], size 4), cursor into
/// region, then SU 100. Predicted bug: L04/L05 (outside region) duplicated into
/// scrollback while staying on screen.
#[test]
fn s5_partial_region_overscroll() {
    let mut t = Terminal::new_detached(COLS, 6);
    t.set_scrollback_limit(100_000);
    t.feed_bytes(b"\x1b[2J\x1b[H");
    t.feed_bytes(b"L00\r\nL01\r\nL02\r\nL03\r\nL04\r\nL05");
    t.feed_bytes(b"\x1b[1;4r"); // DECSTBM rows 1..4 -> region (0,3), size 4
    t.feed_bytes(b"\x1b[3;1H"); // cursor into region
    t.feed_bytes(b"\x1b[100S"); // SU 100

    eprintln!(
        "[S5] after SU100: scrollback_len={}, screen rows:",
        t.scrollback_len()
    );
    for r in 0..6 {
        eprintln!("  screen[{r}]={:?}", t.screen_row(r));
    }
    for i in 0..t.scrollback_len() {
        eprintln!("  scrollback[{i}]={:?}", scrollback_row_text(&t, i));
    }

    let markers = ["L00", "L01", "L02", "L03", "L04", "L05"];
    let pass = oracle_b("S5", &t, &markers);
    assert!(pass, "S5 partial-region over-scroll: marker duplication/loss — see dump");
}

/// S6 — control: full-screen region (no partial). Same SU should be clean.
#[test]
fn s6_fullscreen_control() {
    let mut t = Terminal::new_detached(COLS, 6);
    t.set_scrollback_limit(100_000);
    t.feed_bytes(b"\x1b[2J\x1b[H");
    t.feed_bytes(b"L00\r\nL01\r\nL02\r\nL03\r\nL04\r\nL05");
    t.feed_bytes(b"\x1b[1;6r"); // full-screen margins (region [0,5], size 6)
    t.feed_bytes(b"\x1b[1;1H");
    t.feed_bytes(b"\x1b[100S");

    eprintln!("[S6] scrollback_len={}", t.scrollback_len());
    for i in 0..t.scrollback_len() {
        eprintln!("  scrollback[{i}]={:?}", scrollback_row_text(&t, i));
    }
    let markers = ["L00", "L01", "L02", "L03", "L04", "L05"];
    let pass = oracle_b("S6", &t, &markers);
    assert!(pass, "S6 full-screen control should be clean — see dump");
}

/// S7 — top-anchored partial region, many LFs scrolling the region bottom.
#[test]
fn s7_partial_region_lf_scroll() {
    let mut t = Terminal::new_detached(COLS, 6);
    t.set_scrollback_limit(100_000);
    t.feed_bytes(b"\x1b[2J\x1b[H");
    t.feed_bytes(b"L00\r\nL01\r\nL02\r\nL03\r\nL04\r\nL05");
    t.feed_bytes(b"\x1b[1;4r"); // region (0,3), size 4
    // Move cursor to region bottom (row 4 = index 3) and emit CR+LF pairs so the
    // column resets each time and only the region (rows 0..3) scrolls. Each
    // CR/LF at the region bottom triggers one region-up scroll (scroll_count=1).
    t.feed_bytes(b"\x1b[4;1H");
    for i in 0..8 {
        t.feed_bytes(format!("N{i:02}\r\n").as_bytes());
    }

    eprintln!("[S7] scrollback_len={}", t.scrollback_len());
    for r in 0..6 {
        eprintln!("  screen[{r}]={:?}", t.screen_row(r));
    }
    for i in 0..t.scrollback_len() {
        eprintln!("  scrollback[{i}]={:?}", scrollback_row_text(&t, i));
    }
    // L04, L05 are outside the region and must never duplicate.
    let markers = ["L04", "L05"];
    let pass = oracle_b("S7", &t, &markers);
    assert!(pass, "S7 partial-region LF scroll: L04/L05 must stay unique — see dump");
}
