//! VTE sequence simulator for testing tasty terminal emulation.
//!
//! Translates high-level commands (e.g. "cursor 5 3", "bold", "print hello")
//! into raw VTE escape sequences. The terminal sees the same byte stream
//! as a real TUI app (vim, htop, etc.) would produce.
//!
//! Two modes:
//! - **Interactive**: stdin REPL — external test sends commands via `surface.send`.
//! - **Scenario**: predefined one-shot scenarios for manual verification.

use std::io::{self, BufRead, Write};

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "tasty-tui-sim", about = "VTE sequence simulator for tasty")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Interactive REPL mode — read commands from stdin (default if no subcommand).
    Interactive,
    /// One-shot: cursor movement test.
    Cursor {
        #[arg(long, default_value = "5")]
        row: u16,
        #[arg(long, default_value = "10")]
        col: u16,
        #[arg(long, default_value = "X")]
        marker: String,
        #[arg(long)]
        exit: bool,
    },
    /// One-shot: ANSI 16 + TrueColor test.
    Colors {
        #[arg(long)]
        exit: bool,
    },
    /// One-shot: text attributes test.
    Attrs {
        #[arg(long)]
        exit: bool,
    },
    /// One-shot: alternate screen test.
    Altscreen {
        #[arg(long)]
        exit: bool,
    },
    /// One-shot: CJK/fullwidth test.
    Unicode {
        #[arg(long)]
        exit: bool,
    },
    /// One-shot: scroll region test.
    ScrollRegion {
        #[arg(long)]
        exit: bool,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        None | Some(Commands::Interactive) => interactive_mode(),
        Some(Commands::Cursor {
            row,
            col,
            marker,
            exit,
        }) => scenario_cursor(row, col, &marker, exit),
        Some(Commands::Colors { exit }) => scenario_colors(exit),
        Some(Commands::Attrs { exit }) => scenario_attrs(exit),
        Some(Commands::Altscreen { exit }) => scenario_altscreen(exit),
        Some(Commands::Unicode { exit }) => scenario_unicode(exit),
        Some(Commands::ScrollRegion { exit }) => scenario_scroll_region(exit),
    }
}

// ═══════════════════════════════════════════════════════════════════
// Interactive REPL
// ═══════════════════════════════════════════════════════════════════

fn interactive_mode() {
    let mut out = io::stdout();

    // Signal that REPL is ready
    write!(out, "READY\r\n").unwrap();
    out.flush().unwrap();

    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break, // stdin closed
        };
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        let parts: Vec<&str> = line.splitn(2, ' ').collect();
        let cmd = parts[0];
        let args = if parts.len() > 1 { parts[1] } else { "" };

        match cmd {
            // ── Screen control ──
            "clear" => {
                write!(out, "\x1b[2J\x1b[H").unwrap();
            }
            "reset" => {
                write!(out, "\x1bc").unwrap(); // RIS: full terminal reset
            }

            // ── Cursor movement ──
            "cursor" => {
                // cursor <row> <col> (0-indexed)
                if let Some((row, col)) = parse_two_u16(args) {
                    write!(out, "\x1b[{};{}H", row + 1, col + 1).unwrap();
                } else {
                    write!(out, "ERR: cursor <row> <col>\r\n").unwrap();
                }
            }
            "cursor-up" => {
                let n = parse_u16_or(args, 1);
                write!(out, "\x1b[{n}A").unwrap();
            }
            "cursor-down" => {
                let n = parse_u16_or(args, 1);
                write!(out, "\x1b[{n}B").unwrap();
            }
            "cursor-right" => {
                let n = parse_u16_or(args, 1);
                write!(out, "\x1b[{n}C").unwrap();
            }
            "cursor-left" => {
                let n = parse_u16_or(args, 1);
                write!(out, "\x1b[{n}D").unwrap();
            }
            "cursor-save" => {
                write!(out, "\x1b7").unwrap(); // DECSC
            }
            "cursor-restore" => {
                write!(out, "\x1b8").unwrap(); // DECRC
            }

            // ── Text output ──
            "print" => {
                write!(out, "{args}").unwrap();
            }
            "println" => {
                write!(out, "{args}\r\n").unwrap();
            }
            "newline" => {
                write!(out, "\n").unwrap();
            }
            "cr" => {
                write!(out, "\r").unwrap();
            }
            "tab" => {
                write!(out, "\t").unwrap();
            }
            "bell" => {
                write!(out, "\x07").unwrap();
            }

            // ── SGR (text attributes & colors) ──
            "sgr" => {
                // sgr <params> — e.g. "sgr 1" for bold, "sgr 38;5;1" for red fg
                write!(out, "\x1b[{args}m").unwrap();
            }
            "sgr-reset" => {
                write!(out, "\x1b[0m").unwrap();
            }
            "fg" => {
                // fg <color> — palette index or r;g;b
                if let Some(idx) = args.parse::<u8>().ok() {
                    write!(out, "\x1b[38;5;{idx}m").unwrap();
                } else {
                    // Assume r;g;b format
                    write!(out, "\x1b[38;2;{args}m").unwrap();
                }
            }
            "bg" => {
                if let Some(idx) = args.parse::<u8>().ok() {
                    write!(out, "\x1b[48;5;{idx}m").unwrap();
                } else {
                    write!(out, "\x1b[48;2;{args}m").unwrap();
                }
            }
            "bold" => {
                write!(out, "\x1b[1m").unwrap();
            }
            "italic" => {
                write!(out, "\x1b[3m").unwrap();
            }
            "underline" => {
                write!(out, "\x1b[4m").unwrap();
            }
            "strikethrough" => {
                write!(out, "\x1b[9m").unwrap();
            }
            "inverse" => {
                write!(out, "\x1b[7m").unwrap();
            }
            "dim" => {
                write!(out, "\x1b[2m").unwrap();
            }
            "blink" => {
                // SGR 5 — slow blink
                write!(out, "\x1b[5m").unwrap();
            }
            "blink-rapid" => {
                // SGR 6 — rapid blink
                write!(out, "\x1b[6m").unwrap();
            }
            "blink-off" => {
                // SGR 25
                write!(out, "\x1b[25m").unwrap();
            }
            "invisible" => {
                // SGR 8
                write!(out, "\x1b[8m").unwrap();
            }
            "invisible-off" => {
                // SGR 28
                write!(out, "\x1b[28m").unwrap();
            }
            "overline" => {
                // SGR 53
                write!(out, "\x1b[53m").unwrap();
            }
            "overline-off" => {
                // SGR 55
                write!(out, "\x1b[55m").unwrap();
            }
            "underline-double" => {
                // SGR 21
                write!(out, "\x1b[21m").unwrap();
            }
            "underline-curly" => {
                // SGR 4:3 (extended SGR sub-parameter)
                write!(out, "\x1b[4:3m").unwrap();
            }
            "underline-dotted" => {
                // SGR 4:4
                write!(out, "\x1b[4:4m").unwrap();
            }
            "underline-dashed" => {
                // SGR 4:5
                write!(out, "\x1b[4:5m").unwrap();
            }
            "underline-color" => {
                // underline-color <N> for palette, or "<r;g;b>" for truecolor
                if let Some(idx) = args.parse::<u8>().ok() {
                    write!(out, "\x1b[58:5:{idx}m").unwrap();
                } else if !args.is_empty() {
                    write!(out, "\x1b[58:2::{args}m").unwrap();
                } else {
                    // SGR 59 — default underline color
                    write!(out, "\x1b[59m").unwrap();
                }
            }
            "intensity-off" => {
                // SGR 22 — neither bold nor faint
                write!(out, "\x1b[22m").unwrap();
            }
            "underline-off" => {
                // SGR 24
                write!(out, "\x1b[24m").unwrap();
            }

            // ── Erase ──
            "erase-display" => {
                let mode = parse_u16_or(args, 0);
                write!(out, "\x1b[{mode}J").unwrap();
            }
            "erase-line" => {
                let mode = parse_u16_or(args, 0);
                write!(out, "\x1b[{mode}K").unwrap();
            }

            // ── Alternate screen ──
            "altscreen-enter" => {
                write!(out, "\x1b[?1049h").unwrap();
            }
            "altscreen-exit" => {
                write!(out, "\x1b[?1049l").unwrap();
            }

            // ── Scroll region ──
            "scroll-region" => {
                // scroll-region <top> <bottom> (0-indexed)
                if let Some((top, bottom)) = parse_two_u16(args) {
                    write!(out, "\x1b[{};{}r", top + 1, bottom + 1).unwrap();
                } else {
                    write!(out, "ERR: scroll-region <top> <bottom>\r\n").unwrap();
                }
            }
            "scroll-region-reset" => {
                write!(out, "\x1b[r").unwrap();
            }
            "scroll-up" => {
                let n = parse_u16_or(args, 1);
                write!(out, "\x1b[{n}S").unwrap();
            }
            "scroll-down" => {
                let n = parse_u16_or(args, 1);
                write!(out, "\x1b[{n}T").unwrap();
            }

            // ── Terminal size ──
            "size" => {
                let (cols, rows) = crossterm::terminal::size().unwrap_or((0, 0));
                write!(out, "SIZE:{cols}x{rows}\r\n").unwrap();
            }

            // ── Mouse tracking ──
            "mouse-track" => {
                // Enable mouse tracking (X10 + SGR encoding)
                write!(out, "\x1b[?1000h\x1b[?1006h").unwrap();
            }
            "mouse-track-off" => {
                write!(out, "\x1b[?1000l\x1b[?1006l").unwrap();
            }
            "mouse-track-motion" => {
                // Cell motion + SGR
                write!(out, "\x1b[?1002h\x1b[?1006h").unwrap();
            }
            "mouse-track-all" => {
                // All motion + SGR
                write!(out, "\x1b[?1003h\x1b[?1006h").unwrap();
            }

            // ── DECSET/DECRST modes ──
            "decset" => {
                write!(out, "\x1b[?{args}h").unwrap();
            }
            "decrst" => {
                write!(out, "\x1b[?{args}l").unwrap();
            }

            // ── Raw escape ──
            "raw" => {
                // raw <hex bytes> — e.g. "raw 1b5b48" for ESC[H
                if let Some(bytes) = hex_to_bytes(args) {
                    out.write_all(&bytes).unwrap();
                } else {
                    write!(out, "ERR: raw <hex>\r\n").unwrap();
                }
            }
            "esc" => {
                // esc <sequence> — e.g. "esc [H" sends ESC[H
                write!(out, "\x1b{args}").unwrap();
            }

            // ── Termination ──
            "quit" | "exit" => {
                write!(out, "BYE\r\n").unwrap();
                out.flush().unwrap();
                std::process::exit(0);
            }
            "exit-code" => {
                // exit-code <N>
                let code = args.parse::<i32>().unwrap_or(1);
                out.flush().unwrap();
                std::process::exit(code);
            }
            "crash" => {
                out.flush().unwrap();
                std::process::abort();
            }
            "panic" => {
                out.flush().unwrap();
                panic!("tasty-tui-sim: panic requested");
            }

            // ── Predefined scenarios (run inline) ──
            "scenario" => match args {
                "cursor" => {
                    scenario_cursor_inline(&mut out, 5, 10, "X");
                }
                "colors" => {
                    scenario_colors_inline(&mut out);
                }
                "attrs" => {
                    scenario_attrs_inline(&mut out);
                }
                "unicode" => {
                    scenario_unicode_inline(&mut out);
                }
                "scroll-region" => {
                    scenario_scroll_region_inline(&mut out);
                }
                _ => {
                    write!(out, "ERR: unknown scenario '{args}'\r\n").unwrap();
                }
            },

            _ => {
                write!(out, "ERR: unknown command '{cmd}'\r\n").unwrap();
            }
        }

        out.flush().unwrap();

        // Ack after every command so the test can synchronize
        write!(out, "OK\r\n").unwrap();
        out.flush().unwrap();
    }
}

// ═══════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════

fn parse_two_u16(s: &str) -> Option<(u16, u16)> {
    let parts: Vec<&str> = s.split_whitespace().collect();
    if parts.len() >= 2 {
        Some((parts[0].parse().ok()?, parts[1].parse().ok()?))
    } else {
        None
    }
}

fn parse_u16_or(s: &str, default: u16) -> u16 {
    s.trim().parse().unwrap_or(default)
}

fn hex_to_bytes(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

// ═══════════════════════════════════════════════════════════════════
// Inline scenario helpers (used by both interactive "scenario" cmd
// and one-shot subcommands)
// ═══════════════════════════════════════════════════════════════════

fn scenario_cursor_inline(out: &mut io::Stdout, row: u16, col: u16, marker: &str) {
    write!(out, "\x1b[2J\x1b[H").unwrap();
    write!(out, "\x1b[{};{}H{}", row + 1, col + 1, marker).unwrap();
}

fn scenario_colors_inline(out: &mut io::Stdout) {
    write!(out, "\x1b[2J\x1b[H").unwrap();
    // Row 0: ANSI 16 fg
    for i in 0u8..16 {
        write!(out, "\x1b[38;5;{i}m{:X}", i).unwrap();
    }
    write!(out, "\x1b[0m").unwrap();
    // Row 1: ANSI 16 bg
    write!(out, "\x1b[2;1H").unwrap();
    for i in 0u8..16 {
        write!(out, "\x1b[48;5;{i}m ").unwrap();
    }
    write!(out, "\x1b[0m").unwrap();
    // Row 2: TrueColor
    write!(out, "\x1b[3;1H").unwrap();
    write!(
        out,
        "\x1b[38;2;255;0;0mR\x1b[38;2;0;255;0mG\x1b[38;2;0;0;255mB\x1b[0m"
    )
    .unwrap();
}

fn scenario_attrs_inline(out: &mut io::Stdout) {
    write!(out, "\x1b[2J\x1b[H").unwrap();
    write!(out, "\x1b[1mBOLD\x1b[0m").unwrap();
    write!(out, "\x1b[2;1H\x1b[3mITALIC\x1b[0m").unwrap();
    write!(out, "\x1b[3;1H\x1b[4mUNDERLINE\x1b[0m").unwrap();
    write!(out, "\x1b[4;1H\x1b[9mSTRIKE\x1b[0m").unwrap();
    write!(out, "\x1b[5;1H\x1b[7mINVERSE\x1b[0m").unwrap();
    write!(out, "\x1b[6;1H\x1b[1;3;4mCOMBO\x1b[0m").unwrap();
    write!(out, "\x1b[7;1H\x1b[2mDIM\x1b[0m").unwrap();
}

fn scenario_unicode_inline(out: &mut io::Stdout) {
    write!(out, "\x1b[2J\x1b[H").unwrap();
    write!(out, "\u{D55C}\u{AE00}").unwrap();
    write!(out, "\x1b[2;1H\u{6F22}\u{5B57}").unwrap();
    write!(out, "\x1b[3;1HAB\u{D55C}CD").unwrap();
    write!(out, "\x1b[4;1H\u{3042}\u{3044}\u{3046}").unwrap();
}

fn scenario_scroll_region_inline(out: &mut io::Stdout) {
    write!(out, "\x1b[2J\x1b[H").unwrap();
    write!(out, "\x1b[3;6r").unwrap();
    for i in 0..8u16 {
        write!(out, "\x1b[{};1HLINE{}", i + 1, i).unwrap();
    }
    write!(out, "\x1b[6;1H\nSCROLLED").unwrap();
    write!(out, "\x1b[r").unwrap();
}

// ═══════════════════════════════════════════════════════════════════
// One-shot scenario subcommands (for manual use)
// ═══════════════════════════════════════════════════════════════════

fn clear_and_setup(out: &mut io::Stdout) {
    write!(out, "\x1b[2J\x1b[H").unwrap();
    out.flush().unwrap();
}

fn finish(out: &mut io::Stdout, marker: &str, exit: bool) {
    write!(out, "\x1b[999;1H{marker}").unwrap();
    out.flush().unwrap();
    if !exit {
        // "keep open" 모드: stdin이 닫히면 Err — 정상적인 종료 신호이므로 무시.
        if let Err(e) = crossterm::event::read() {
            eprintln!("tasty-tui-sim: event wait ended: {e}");
        }
    }
}

fn scenario_cursor(row: u16, col: u16, marker: &str, exit: bool) {
    let mut out = io::stdout();
    clear_and_setup(&mut out);
    write!(out, "\x1b[{};{}H{}", row + 1, col + 1, marker).unwrap();
    out.flush().unwrap();
    finish(&mut out, "CURSOR_TEST_DONE", exit);
}

fn scenario_colors(exit: bool) {
    let mut out = io::stdout();
    scenario_colors_inline(&mut out);
    out.flush().unwrap();
    finish(&mut out, "COLORS_TEST_DONE", exit);
}

fn scenario_attrs(exit: bool) {
    let mut out = io::stdout();
    scenario_attrs_inline(&mut out);
    out.flush().unwrap();
    finish(&mut out, "ATTRS_TEST_DONE", exit);
}

fn scenario_altscreen(exit: bool) {
    let mut out = io::stdout();
    write!(out, "\x1b[2J\x1b[HNORMAL_SCREEN").unwrap();
    out.flush().unwrap();
    write!(out, "\x1b[?1049h\x1b[2J\x1b[HALT_SCREEN_CONTENT").unwrap();
    out.flush().unwrap();
    write!(out, "\x1b[999;1HALTSCREEN_TEST_DONE").unwrap();
    out.flush().unwrap();
    if !exit {
        // "keep open" 모드: stdin이 닫히면 Err — 정상적인 종료 신호이므로 무시.
        if let Err(e) = crossterm::event::read() {
            eprintln!("tasty-tui-sim: event wait ended: {e}");
        }
    }
    write!(out, "\x1b[?1049l").unwrap();
    out.flush().unwrap();
}

fn scenario_unicode(exit: bool) {
    let mut out = io::stdout();
    scenario_unicode_inline(&mut out);
    out.flush().unwrap();
    finish(&mut out, "UNICODE_TEST_DONE", exit);
}

fn scenario_scroll_region(exit: bool) {
    let mut out = io::stdout();
    scenario_scroll_region_inline(&mut out);
    out.flush().unwrap();
    finish(&mut out, "SCROLL_TEST_DONE", exit);
}
