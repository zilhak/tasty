#![forbid(unsafe_code)]

//! VTE sequence simulator for testing tasty terminal emulation.
//!
//! Translates high-level commands (e.g. "cursor 5 3", "bold", "print hello")
//! into raw VTE escape sequences. The terminal sees the same byte stream
//! as a real TUI app (vim, htop, etc.) would produce.
//!
//! Two modes:
//! - **Interactive**: stdin REPL — external test sends commands via `surface.send`.
//! - **Scenario**: predefined one-shot scenarios for manual verification.
//!
//! Exposed both as the standalone `tasty-tui-sim` binary and, in debug builds,
//! as the `tasty debug sim` subcommand (same logic, shared via this lib).

use std::io::{self, BufRead, Write};

use clap::Subcommand;

#[derive(Subcommand)]
pub enum Commands {
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
    /// Stress mode: full-screen truecolor redraw every frame (reproduces input lag).
    Flood {
        /// Delay between frames in ms (0 = unthrottled / max throughput).
        #[arg(long, default_value = "0")]
        rate_ms: u64,
        /// Columns (0 = auto via terminal size, fallback 200).
        #[arg(long, default_value = "0")]
        cols: u16,
        /// Rows (0 = auto via terminal size, fallback 50).
        #[arg(long, default_value = "0")]
        rows: u16,
        /// Number of frames (0 = infinite).
        #[arg(long, default_value = "0")]
        frames: u64,
        /// Stay on the main screen instead of entering the alternate screen.
        #[arg(long)]
        inline: bool,
    },
}

/// Dispatch a parsed command. `None` (no subcommand) runs the interactive REPL,
/// matching the standalone binary's default.
pub fn run(command: &Option<Commands>) {
    match command {
        None | Some(Commands::Interactive) => interactive_mode(),
        Some(Commands::Cursor {
            row,
            col,
            marker,
            exit,
        }) => scenario_cursor(*row, *col, marker, *exit),
        Some(Commands::Colors { exit }) => scenario_colors(*exit),
        Some(Commands::Attrs { exit }) => scenario_attrs(*exit),
        Some(Commands::Altscreen { exit }) => scenario_altscreen(*exit),
        Some(Commands::Unicode { exit }) => scenario_unicode(*exit),
        Some(Commands::ScrollRegion { exit }) => scenario_scroll_region(*exit),
        Some(Commands::Flood {
            rate_ms,
            cols,
            rows,
            frames,
            inline,
        }) => flood_mode(*rate_ms, *cols, *rows, *frames, *inline),
    }
}

// ═══════════════════════════════════════════════════════════════════
// Interactive REPL
// ═══════════════════════════════════════════════════════════════════

/// Handle a single REPL command, writing its VTE output to `out`. Returns
/// `Err` the moment any write fails (e.g. `BrokenPipe` because the peer
/// surface already closed) so the caller can stop the loop instead of
/// propagating a panic — same "write failure = peer gone = quiet exit"
/// policy `flood_mode` already established, applied here per-command.
///
/// `quit`/`exit-code`/`crash`/`panic` terminate the process unconditionally
/// (there is no caller left to report an error to), so their own writes are
/// best-effort rather than `?`.
fn handle_command(out: &mut io::Stdout, cmd: &str, args: &str) -> io::Result<()> {
    match cmd {
        // ── Screen control ──
        "clear" => {
            write!(out, "\x1b[2J\x1b[H")?;
        }
        "reset" => {
            write!(out, "\x1bc")?; // RIS: full terminal reset
        }

        // ── Cursor movement ──
        "cursor" => {
            // cursor <row> <col> (0-indexed)
            if let Some((row, col)) = parse_two_u16(args) {
                write!(out, "\x1b[{};{}H", row + 1, col + 1)?;
            } else {
                write!(out, "ERR: cursor <row> <col>\r\n")?;
            }
        }
        "cursor-up" => {
            let n = parse_u16_or(args, 1);
            write!(out, "\x1b[{n}A")?;
        }
        "cursor-down" => {
            let n = parse_u16_or(args, 1);
            write!(out, "\x1b[{n}B")?;
        }
        "cursor-right" => {
            let n = parse_u16_or(args, 1);
            write!(out, "\x1b[{n}C")?;
        }
        "cursor-left" => {
            let n = parse_u16_or(args, 1);
            write!(out, "\x1b[{n}D")?;
        }
        "cursor-save" => {
            write!(out, "\x1b7")?; // DECSC
        }
        "cursor-restore" => {
            write!(out, "\x1b8")?; // DECRC
        }

        // ── Text output ──
        "print" => {
            write!(out, "{args}")?;
        }
        "println" => {
            write!(out, "{args}\r\n")?;
        }
        "newline" => {
            writeln!(out)?;
        }
        "cr" => {
            write!(out, "\r")?;
        }
        "tab" => {
            write!(out, "\t")?;
        }
        "bell" => {
            write!(out, "\x07")?;
        }

        // ── SGR (text attributes & colors) ──
        "sgr" => {
            // sgr <params> — e.g. "sgr 1" for bold, "sgr 38;5;1" for red fg
            write!(out, "\x1b[{args}m")?;
        }
        "sgr-reset" => {
            write!(out, "\x1b[0m")?;
        }
        "fg" => {
            // fg <color> — palette index or r;g;b
            if let Ok(idx) = args.parse::<u8>() {
                write!(out, "\x1b[38;5;{idx}m")?;
            } else {
                // Assume r;g;b format
                write!(out, "\x1b[38;2;{args}m")?;
            }
        }
        "bg" => {
            if let Ok(idx) = args.parse::<u8>() {
                write!(out, "\x1b[48;5;{idx}m")?;
            } else {
                write!(out, "\x1b[48;2;{args}m")?;
            }
        }
        "bold" => {
            write!(out, "\x1b[1m")?;
        }
        "italic" => {
            write!(out, "\x1b[3m")?;
        }
        "underline" => {
            write!(out, "\x1b[4m")?;
        }
        "strikethrough" => {
            write!(out, "\x1b[9m")?;
        }
        "inverse" => {
            write!(out, "\x1b[7m")?;
        }
        "dim" => {
            write!(out, "\x1b[2m")?;
        }
        "blink" => {
            // SGR 5 — slow blink
            write!(out, "\x1b[5m")?;
        }
        "blink-rapid" => {
            // SGR 6 — rapid blink
            write!(out, "\x1b[6m")?;
        }
        "blink-off" => {
            // SGR 25
            write!(out, "\x1b[25m")?;
        }
        "invisible" => {
            // SGR 8
            write!(out, "\x1b[8m")?;
        }
        "invisible-off" => {
            // SGR 28
            write!(out, "\x1b[28m")?;
        }
        "overline" => {
            // SGR 53
            write!(out, "\x1b[53m")?;
        }
        "overline-off" => {
            // SGR 55
            write!(out, "\x1b[55m")?;
        }
        "underline-double" => {
            // SGR 21
            write!(out, "\x1b[21m")?;
        }
        "underline-curly" => {
            // SGR 4:3 (extended SGR sub-parameter)
            write!(out, "\x1b[4:3m")?;
        }
        "underline-dotted" => {
            // SGR 4:4
            write!(out, "\x1b[4:4m")?;
        }
        "underline-dashed" => {
            // SGR 4:5
            write!(out, "\x1b[4:5m")?;
        }
        "underline-color" => {
            // underline-color <N> for palette, or "<r;g;b>" for truecolor
            if let Ok(idx) = args.parse::<u8>() {
                write!(out, "\x1b[58:5:{idx}m")?;
            } else if !args.is_empty() {
                write!(out, "\x1b[58:2::{args}m")?;
            } else {
                // SGR 59 — default underline color
                write!(out, "\x1b[59m")?;
            }
        }
        "intensity-off" => {
            // SGR 22 — neither bold nor faint
            write!(out, "\x1b[22m")?;
        }
        "underline-off" => {
            // SGR 24
            write!(out, "\x1b[24m")?;
        }

        // ── Erase ──
        "erase-display" => {
            let mode = parse_u16_or(args, 0);
            write!(out, "\x1b[{mode}J")?;
        }
        "erase-line" => {
            let mode = parse_u16_or(args, 0);
            write!(out, "\x1b[{mode}K")?;
        }

        // ── Alternate screen ──
        "altscreen-enter" => {
            write!(out, "\x1b[?1049h")?;
        }
        "altscreen-exit" => {
            write!(out, "\x1b[?1049l")?;
        }

        // ── Scroll region ──
        "scroll-region" => {
            // scroll-region <top> <bottom> (0-indexed)
            if let Some((top, bottom)) = parse_two_u16(args) {
                write!(out, "\x1b[{};{}r", top + 1, bottom + 1)?;
            } else {
                write!(out, "ERR: scroll-region <top> <bottom>\r\n")?;
            }
        }
        "scroll-region-reset" => {
            write!(out, "\x1b[r")?;
        }
        "scroll-up" => {
            let n = parse_u16_or(args, 1);
            write!(out, "\x1b[{n}S")?;
        }
        "scroll-down" => {
            let n = parse_u16_or(args, 1);
            write!(out, "\x1b[{n}T")?;
        }

        // ── Terminal size ──
        "size" => {
            let (cols, rows) = crossterm::terminal::size().unwrap_or((0, 0));
            write!(out, "SIZE:{cols}x{rows}\r\n")?;
        }

        // ── Mouse tracking ──
        "mouse-track" => {
            // Enable mouse tracking (X10 + SGR encoding)
            write!(out, "\x1b[?1000h\x1b[?1006h")?;
        }
        "mouse-track-off" => {
            write!(out, "\x1b[?1000l\x1b[?1006l")?;
        }
        "mouse-track-motion" => {
            // Cell motion + SGR
            write!(out, "\x1b[?1002h\x1b[?1006h")?;
        }
        "mouse-track-all" => {
            // All motion + SGR
            write!(out, "\x1b[?1003h\x1b[?1006h")?;
        }

        // ── DECSET/DECRST modes ──
        "decset" => {
            write!(out, "\x1b[?{args}h")?;
        }
        "decrst" => {
            write!(out, "\x1b[?{args}l")?;
        }

        // ── Raw escape ──
        "raw" => {
            // raw <hex bytes> — e.g. "raw 1b5b48" for ESC[H
            if let Some(bytes) = hex_to_bytes(args) {
                out.write_all(&bytes)?;
            } else {
                write!(out, "ERR: raw <hex>\r\n")?;
            }
        }
        "esc" => {
            // esc <sequence> — e.g. "esc [H" sends ESC[H
            write!(out, "\x1b{args}")?;
        }

        // ── Termination ── (process exits unconditionally — no caller left
        // to propagate an error to, so these writes are best-effort.)
        "quit" | "exit" => {
            let _ = write!(out, "BYE\r\n"); // 직후 exit — 전달할 호출자가 없다.
            let _ = out.flush(); // 위와 같음.
            std::process::exit(0);
        }
        "exit-code" => {
            // exit-code <N>
            let code = args.parse::<i32>().unwrap_or(1);
            let _ = out.flush(); // 직후 exit — 전달할 호출자가 없다.
            std::process::exit(code);
        }
        "crash" => {
            let _ = out.flush(); // 직후 abort — 전달할 호출자가 없다.
            std::process::abort();
        }
        "panic" => {
            let _ = out.flush(); // 직후 panic — 전달할 호출자가 없다.
            panic!("tasty-tui-sim: panic requested");
        }

        // ── Predefined scenarios (run inline) ──
        "scenario" => match args {
            "cursor" => scenario_cursor_inline(out, 5, 10, "X")?,
            "colors" => scenario_colors_inline(out)?,
            "attrs" => scenario_attrs_inline(out)?,
            "unicode" => scenario_unicode_inline(out)?,
            "scroll-region" => scenario_scroll_region_inline(out)?,
            _ => {
                write!(out, "ERR: unknown scenario '{args}'\r\n")?;
            }
        },

        _ => {
            write!(out, "ERR: unknown command '{cmd}'\r\n")?;
        }
    }
    Ok(())
}

/// Consumes an I/O result from a REPL stdout write/flush: a closed peer pipe
/// (`BrokenPipe`, expected when the surface/test-harness on the other end
/// goes away mid-command) means "stop the loop quietly", while any other I/O
/// failure (disk full, permission denied, ...) is a simulator bug and must
/// keep panicking so it isn't silently swallowed. Returns `true` when the
/// caller should stop.
fn should_stop_repl(result: io::Result<()>) -> bool {
    match result {
        Ok(()) => false,
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => true,
        Err(e) => panic!("tasty-tui-sim: unexpected I/O error writing to stdout: {e}"),
    }
}

fn interactive_mode() {
    let mut out = io::stdout();

    // Signal that REPL is ready. Peer may already be gone — bail quietly.
    if should_stop_repl(write!(out, "READY\r\n").and_then(|()| out.flush())) {
        return;
    }

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

        if should_stop_repl(handle_command(&mut out, cmd, args)) {
            break;
        }

        // Ack after every command so the test can synchronize. The peer may
        // have closed the pipe between the command write above and here —
        // treat that the same as any other write failure.
        let ack = out
            .flush()
            .and_then(|()| write!(out, "OK\r\n"))
            .and_then(|()| out.flush());
        if should_stop_repl(ack) {
            break;
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// Flood stress mode
// ═══════════════════════════════════════════════════════════════════

/// Continuously redraw the full screen with per-cell truecolor SGR + glyph,
/// changing colors every frame so every cell is dirty. Used to reproduce
/// input-lag scenarios under heavy VTE throughput.
fn flood_mode(rate_ms: u64, cols: u16, rows: u16, frames: u64, inline: bool) {
    use std::fmt::Write as _;
    use std::time::{Duration, Instant};

    let (auto_cols, auto_rows) = crossterm::terminal::size().unwrap_or((200, 50));
    let cols = if cols == 0 { auto_cols.max(1) } else { cols };
    let rows = if rows == 0 { auto_rows.max(1) } else { rows };

    let glyphs = [b'#', b'@', b'%', b'&', b'*', b'+', b'=', b'-'];

    let stdout = io::stdout();
    let mut out = stdout.lock();

    // Enter alternate screen (unless --inline) and hide the cursor.
    if !inline && out.write_all(b"\x1b[?1049h\x1b[?25l\x1b[2J").is_err() {
        return;
    }

    let start = Instant::now();
    let mut frame: u64 = 0;
    let cell_capacity = cols as usize * rows as usize * 32 + 64;

    loop {
        frame += 1;

        let mut buf = String::with_capacity(cell_capacity);
        buf.push_str("\x1b[H");
        for y in 0..rows {
            for x in 0..cols {
                let r = (x as u64 + frame) as u8;
                let g = (y as u64 + frame.wrapping_mul(2)) as u8;
                let b = (x as u64 + y as u64 + frame.wrapping_mul(3)) as u8;
                let glyph = glyphs[(x as usize + y as usize + frame as usize) % glyphs.len()];
                write!(
                    buf,
                    "\x1b[38;2;{};{};{};48;2;{};{};{}m{}",
                    r,
                    g,
                    b,
                    255 - r,
                    255 - g,
                    255 - b,
                    glyph as char,
                )
                .expect("writing to a String is infallible");
            }
        }

        let bytes = buf.len();
        let elapsed = start.elapsed().as_millis();
        // Counter on the last row, reset SGR first so it stays readable.
        write!(
            buf,
            "\x1b[0m\x1b[{};1Hflood #{} {}ms {}B/frame",
            rows, frame, elapsed, bytes,
        )
        .expect("writing to a String is infallible");

        // Surface may close mid-write (BrokenPipe) — bail out without panicking.
        if out.write_all(buf.as_bytes()).is_err() || out.flush().is_err() {
            break;
        }

        if frames != 0 && frame >= frames {
            break;
        }
        if rate_ms != 0 {
            std::thread::sleep(Duration::from_millis(rate_ms));
        }
    }

    // Best-effort restore: surface may already be gone (BrokenPipe), so ignore errors.
    if !inline {
        let _ = out.write_all(b"\x1b[?1049l\x1b[?25h"); // 정리: 복원 실패해도 무시
        let _ = out.flush(); // 정리: 복원 실패해도 무시
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
    if !s.len().is_multiple_of(2) {
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

fn scenario_cursor_inline(
    out: &mut io::Stdout,
    row: u16,
    col: u16,
    marker: &str,
) -> io::Result<()> {
    write!(out, "\x1b[2J\x1b[H")?;
    write!(out, "\x1b[{};{}H{}", row + 1, col + 1, marker)?;
    Ok(())
}

fn scenario_colors_inline(out: &mut io::Stdout) -> io::Result<()> {
    write!(out, "\x1b[2J\x1b[H")?;
    // Row 0: ANSI 16 fg
    for i in 0u8..16 {
        write!(out, "\x1b[38;5;{i}m{:X}", i)?;
    }
    write!(out, "\x1b[0m")?;
    // Row 1: ANSI 16 bg
    write!(out, "\x1b[2;1H")?;
    for i in 0u8..16 {
        write!(out, "\x1b[48;5;{i}m ")?;
    }
    write!(out, "\x1b[0m")?;
    // Row 2: TrueColor
    write!(out, "\x1b[3;1H")?;
    write!(
        out,
        "\x1b[38;2;255;0;0mR\x1b[38;2;0;255;0mG\x1b[38;2;0;0;255mB\x1b[0m"
    )?;
    Ok(())
}

fn scenario_attrs_inline(out: &mut io::Stdout) -> io::Result<()> {
    write!(out, "\x1b[2J\x1b[H")?;
    write!(out, "\x1b[1mBOLD\x1b[0m")?;
    write!(out, "\x1b[2;1H\x1b[3mITALIC\x1b[0m")?;
    write!(out, "\x1b[3;1H\x1b[4mUNDERLINE\x1b[0m")?;
    write!(out, "\x1b[4;1H\x1b[9mSTRIKE\x1b[0m")?;
    write!(out, "\x1b[5;1H\x1b[7mINVERSE\x1b[0m")?;
    write!(out, "\x1b[6;1H\x1b[1;3;4mCOMBO\x1b[0m")?;
    write!(out, "\x1b[7;1H\x1b[2mDIM\x1b[0m")?;
    Ok(())
}

fn scenario_unicode_inline(out: &mut io::Stdout) -> io::Result<()> {
    write!(out, "\x1b[2J\x1b[H")?;
    write!(out, "\u{D55C}\u{AE00}")?;
    write!(out, "\x1b[2;1H\u{6F22}\u{5B57}")?;
    write!(out, "\x1b[3;1HAB\u{D55C}CD")?;
    write!(out, "\x1b[4;1H\u{3042}\u{3044}\u{3046}")?;
    Ok(())
}

fn scenario_scroll_region_inline(out: &mut io::Stdout) -> io::Result<()> {
    write!(out, "\x1b[2J\x1b[H")?;
    write!(out, "\x1b[3;6r")?;
    for i in 0..8u16 {
        write!(out, "\x1b[{};1HLINE{}", i + 1, i)?;
    }
    write!(out, "\x1b[6;1H\nSCROLLED")?;
    write!(out, "\x1b[r")?;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════
// One-shot scenario subcommands (for manual use)
// ═══════════════════════════════════════════════════════════════════

fn clear_and_setup(out: &mut io::Stdout) {
    write!(out, "\x1b[2J\x1b[H").unwrap();
    out.flush().unwrap();
}

/// Writes the completion marker and (unless `exit`) blocks until the peer
/// signals it's done inspecting the screen. Callers propagate/best-effort
/// per their own context — the one-shot subcommands below are already at
/// their last statement when they call this, so they just discard the
/// result (nothing left to do if the peer is already gone).
fn finish(out: &mut io::Stdout, marker: &str, exit: bool) -> io::Result<()> {
    write!(out, "\x1b[999;1H{marker}")?;
    out.flush()?;
    if !exit {
        // "keep open" 모드: stdin이 닫히면 Err — 정상적인 종료 신호이므로 무시.
        if let Err(e) = crossterm::event::read() {
            eprintln!("tasty-tui-sim: event wait ended: {e}");
        }
    }
    Ok(())
}

fn scenario_cursor(row: u16, col: u16, marker: &str, exit: bool) {
    let mut out = io::stdout();
    clear_and_setup(&mut out);
    write!(out, "\x1b[{};{}H{}", row + 1, col + 1, marker).unwrap();
    out.flush().unwrap();
    let _ = finish(&mut out, "CURSOR_TEST_DONE", exit); // 마지막 문 — peer 가 이미 사라졌으면 결과 무의미(best-effort)
}

fn scenario_colors(exit: bool) {
    let mut out = io::stdout();
    if scenario_colors_inline(&mut out).is_err() {
        return; // 파이프가 이미 끊어짐 — 이어지는 flush/finish는 무의미
    }
    if out.flush().is_err() {
        return;
    }
    let _ = finish(&mut out, "COLORS_TEST_DONE", exit); // 마지막 문 — peer 가 이미 사라졌으면 결과 무의미(best-effort)
}

fn scenario_attrs(exit: bool) {
    let mut out = io::stdout();
    if scenario_attrs_inline(&mut out).is_err() {
        return;
    }
    if out.flush().is_err() {
        return;
    }
    let _ = finish(&mut out, "ATTRS_TEST_DONE", exit); // 마지막 문 — peer 가 이미 사라졌으면 결과 무의미(best-effort)
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
    if scenario_unicode_inline(&mut out).is_err() {
        return;
    }
    if out.flush().is_err() {
        return;
    }
    let _ = finish(&mut out, "UNICODE_TEST_DONE", exit); // 마지막 문 — peer 가 이미 사라졌으면 결과 무의미(best-effort)
}

fn scenario_scroll_region(exit: bool) {
    let mut out = io::stdout();
    if scenario_scroll_region_inline(&mut out).is_err() {
        return;
    }
    if out.flush().is_err() {
        return;
    }
    let _ = finish(&mut out, "SCROLL_TEST_DONE", exit); // 마지막 문 — peer 가 이미 사라졌으면 결과 무의미(best-effort)
}
