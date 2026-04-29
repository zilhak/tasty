//! Test TUI app for verifying tasty terminal emulation.
//!
//! Each subcommand outputs deterministic VTE sequences so that
//! tasty's debug IPC (`debug.cell_info`, `debug.screen_attrs`) can
//! verify the terminal parsed and rendered them correctly.

use std::io::{self, Write};

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "tasty-test-tui", about = "Test TUI scenarios for tasty")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Place a marker character at a specific position to verify cursor movement.
    Cursor {
        /// Target row (0-indexed)
        #[arg(long, default_value = "5")]
        row: u16,
        /// Target column (0-indexed)
        #[arg(long, default_value = "10")]
        col: u16,
        /// Marker character
        #[arg(long, default_value = "X")]
        marker: String,
        /// Exit immediately after output (don't wait)
        #[arg(long)]
        exit: bool,
    },
    /// Output colored text using ANSI 16 colors and TrueColor to verify SGR parsing.
    Colors {
        /// Exit immediately after output
        #[arg(long)]
        exit: bool,
    },
    /// Output text with various attributes (bold, italic, underline, etc.).
    Attrs {
        /// Exit immediately after output
        #[arg(long)]
        exit: bool,
    },
    /// Enter alternate screen, draw content, then exit.
    Altscreen {
        /// Exit immediately after output
        #[arg(long)]
        exit: bool,
    },
    /// Output CJK/fullwidth characters to verify wide-char handling.
    Unicode {
        /// Exit immediately after output
        #[arg(long)]
        exit: bool,
    },
    /// Set up a scroll region and scroll within it.
    ScrollRegion {
        /// Exit immediately after output
        #[arg(long)]
        exit: bool,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Cursor {
            row,
            col,
            marker,
            exit,
        } => scenario_cursor(row, col, &marker, exit),
        Commands::Colors { exit } => scenario_colors(exit),
        Commands::Attrs { exit } => scenario_attrs(exit),
        Commands::Altscreen { exit } => scenario_altscreen(exit),
        Commands::Unicode { exit } => scenario_unicode(exit),
        Commands::ScrollRegion { exit } => scenario_scroll_region(exit),
    }
}

/// Clear screen and move cursor to a position, then print completion marker.
fn clear_and_setup(out: &mut io::Stdout) {
    // Save cursor, clear screen, move to (0,0)
    write!(out, "\x1b[2J\x1b[H").unwrap();
    out.flush().unwrap();
}

/// Print a completion marker and optionally wait for input.
fn finish(out: &mut io::Stdout, marker: &str, exit: bool) {
    // Move to bottom-left and print marker
    write!(out, "\x1b[999;1H{marker}").unwrap();
    out.flush().unwrap();

    if !exit {
        // Wait for any key
        let _ = crossterm::event::read();
    }
}

// ── Scenario: cursor ──

fn scenario_cursor(row: u16, col: u16, marker: &str, exit: bool) {
    let mut out = io::stdout();
    clear_and_setup(&mut out);

    // CUP: move to row, col (1-indexed in VTE)
    write!(out, "\x1b[{};{}H{}", row + 1, col + 1, marker).unwrap();
    out.flush().unwrap();

    finish(&mut out, "CURSOR_TEST_DONE", exit);
}

// ── Scenario: colors ──

fn scenario_colors(exit: bool) {
    let mut out = io::stdout();
    clear_and_setup(&mut out);

    // Row 0: ANSI 16 foreground colors — each char "0".."F" in its color
    write!(out, "\x1b[H").unwrap();
    for i in 0u8..16 {
        write!(out, "\x1b[38;5;{i}m{:X}", i).unwrap();
    }
    write!(out, "\x1b[0m").unwrap();

    // Row 1: ANSI 16 background colors — space with colored bg
    write!(out, "\x1b[2;1H").unwrap();
    for i in 0u8..16 {
        write!(out, "\x1b[48;5;{i}m ").unwrap();
    }
    write!(out, "\x1b[0m").unwrap();

    // Row 2: TrueColor — red, green, blue blocks
    write!(out, "\x1b[3;1H").unwrap();
    write!(out, "\x1b[38;2;255;0;0mR").unwrap(); // col 0: red fg
    write!(out, "\x1b[38;2;0;255;0mG").unwrap(); // col 1: green fg
    write!(out, "\x1b[38;2;0;0;255mB").unwrap(); // col 2: blue fg
    write!(out, "\x1b[0m").unwrap();

    finish(&mut out, "COLORS_TEST_DONE", exit);
}

// ── Scenario: attrs ──

fn scenario_attrs(exit: bool) {
    let mut out = io::stdout();
    clear_and_setup(&mut out);

    // Row 0: bold
    write!(out, "\x1b[H\x1b[1mBOLD\x1b[0m").unwrap();
    // Row 1: italic
    write!(out, "\x1b[2;1H\x1b[3mITALIC\x1b[0m").unwrap();
    // Row 2: underline
    write!(out, "\x1b[3;1H\x1b[4mUNDERLINE\x1b[0m").unwrap();
    // Row 3: strikethrough
    write!(out, "\x1b[4;1H\x1b[9mSTRIKE\x1b[0m").unwrap();
    // Row 4: inverse
    write!(out, "\x1b[5;1H\x1b[7mINVERSE\x1b[0m").unwrap();
    // Row 5: combined bold+italic+underline
    write!(out, "\x1b[6;1H\x1b[1;3;4mCOMBO\x1b[0m").unwrap();

    out.flush().unwrap();
    finish(&mut out, "ATTRS_TEST_DONE", exit);
}

// ── Scenario: altscreen ──

fn scenario_altscreen(exit: bool) {
    let mut out = io::stdout();

    // Write marker on normal screen first
    write!(out, "\x1b[2J\x1b[HNORMAL_SCREEN").unwrap();
    out.flush().unwrap();

    // Enter alternate screen (DECSET 1049)
    write!(out, "\x1b[?1049h").unwrap();
    write!(out, "\x1b[2J\x1b[HALT_SCREEN_CONTENT").unwrap();
    out.flush().unwrap();

    // Print done marker on alt screen
    write!(out, "\x1b[999;1HALTSCREEN_TEST_DONE").unwrap();
    out.flush().unwrap();

    if !exit {
        let _ = crossterm::event::read();
    }

    // Exit alternate screen (DECRST 1049)
    write!(out, "\x1b[?1049l").unwrap();
    out.flush().unwrap();
}

// ── Scenario: unicode ──

fn scenario_unicode(exit: bool) {
    let mut out = io::stdout();
    clear_and_setup(&mut out);

    // Row 0: Korean (Hangul) — 2-cell wide characters
    write!(out, "\x1b[H\u{D55C}\u{AE00}").unwrap(); // 한글
    // Row 1: CJK ideographs
    write!(out, "\x1b[2;1H\u{6F22}\u{5B57}").unwrap(); // 漢字
    // Row 2: Mixed ASCII + wide
    write!(out, "\x1b[3;1HAB\u{D55C}CD").unwrap(); // AB한CD
    // Row 3: Japanese hiragana
    write!(out, "\x1b[4;1H\u{3042}\u{3044}\u{3046}").unwrap(); // あいう

    out.flush().unwrap();
    finish(&mut out, "UNICODE_TEST_DONE", exit);
}

// ── Scenario: scroll-region ──

fn scenario_scroll_region(exit: bool) {
    let mut out = io::stdout();
    clear_and_setup(&mut out);

    // Set scroll region to rows 2-5 (1-indexed: 3-6)
    write!(out, "\x1b[3;6r").unwrap();

    // Fill rows with labels
    for i in 0..8u16 {
        write!(out, "\x1b[{};1HLINE{}", i + 1, i).unwrap();
    }

    // Move inside scroll region and scroll up
    write!(out, "\x1b[6;1H").unwrap(); // bottom of region
    write!(out, "\n").unwrap(); // should scroll within region only
    write!(out, "SCROLLED").unwrap();

    // Reset scroll region
    write!(out, "\x1b[r").unwrap();

    out.flush().unwrap();
    finish(&mut out, "SCROLL_TEST_DONE", exit);
}
