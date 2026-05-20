mod accessors;
mod events;
mod io;
mod modes;
mod output_buffer;
mod resize;
mod screen;
mod scrollback;
mod vte_handler;

pub mod cwd;
pub mod disk_scrollback;
pub mod foreground_process;
pub mod search;
pub mod test_helpers;

use std::io::{Read, Write};
use std::sync::mpsc;
use std::thread;

use anyhow::Result;
use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};
use termwiz::cell::CellAttributes;
use termwiz::escape::Action;
use termwiz::escape::csi::CSI;
use termwiz::escape::parser::Parser;
use termwiz::surface::Surface;

pub use events::*;
pub use scrollback::ScrollbackLine;

/// Configuration for creating a new Terminal.
pub struct TerminalConfig<'a> {
    pub cols: usize,
    pub rows: usize,
    pub shell: Option<&'a str>,
    pub args: &'a [&'a str],
    pub surface_id: u32,
    pub working_dir: Option<&'a std::path::Path>,
    /// PTY master fd 에 동기적으로 미리 써넣을 바이트. writer thread 가 spawn
    /// 되기 전에 직접 write_all + flush 되므로, child shell 이 stdin 을 처음 read
    /// 하는 순간 무조건 이 바이트가 첫 입력으로 들어온다. TUI 세션 복원
    /// (`claude -r <uuid>\r`) 처럼 spawn 과 동시에 실행되어야 할 명령을 넘기는 용도.
    /// 호출자는 줄바꿈(`\r`) 등 submit 문자를 직접 포함해야 한다.
    pub initial_input: Option<&'a str>,
}

/// Information about a single cell for debug inspection.
#[derive(Debug, Clone)]
pub struct CellInfo {
    pub text: String,
    pub fg: String,
    pub bg: String,
    /// Legacy bool kept for backward compat. True iff `intensity == "bold"`.
    pub bold: bool,
    pub italic: bool,
    /// Legacy bool kept for backward compat. True iff `underline_style != "none"`.
    pub underline: bool,
    pub strikethrough: bool,
    pub inverse: bool,
    pub width: usize,
    /// "normal" | "bold" | "half" (faint/dim, SGR 2)
    pub intensity: &'static str,
    /// "none" | "single" | "double" | "curly" | "dotted" | "dashed"
    pub underline_style: &'static str,
    /// "default" | "palette:N" | "#rrggbb"
    pub underline_color: String,
    /// "none" | "slow" | "rapid"
    pub blink: &'static str,
    pub invisible: bool,
    pub overline: bool,
    /// "baseline" | "super" | "sub"
    pub vertical_align: &'static str,
}

pub struct Terminal {
    /// Primary screen buffer.
    pub(crate) primary_surface: Surface,
    /// Alternate screen buffer (lazily created on DECSET 1049/47).
    pub(crate) alternate_surface: Option<Surface>,
    /// Whether the alternate screen is active.
    pub(crate) use_alternate: bool,
    parser: Parser,
    /// Channel for non-blocking PTY writes. A background writer thread drains this.
    pty_write_tx: mpsc::Sender<Vec<u8>>,
    _writer_thread: thread::JoinHandle<()>,
    pty_master: Box<dyn portable_pty::MasterPty + Send>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    action_rx: mpsc::Receiver<Vec<u8>>,
    _reader_thread: thread::JoinHandle<()>,
    pub(crate) cols: usize,
    pub(crate) rows: usize,
    /// Saved cursor position for ESC 7 / ESC 8
    pub(crate) saved_cursor: Option<(usize, usize)>,
    /// Saved cursor position specifically for alternate screen enter/exit.
    pub(crate) alt_saved_cursor: Option<(usize, usize)>,
    /// Events accumulated during process(), consumed via take_events().
    pub(crate) events: Vec<TerminalEvent>,
    /// Raw PTY output buffer for read-mark API and ClaudeError scanner.
    output: output_buffer::OutputBuffer,
    /// Whether we've already emitted a ProcessExited event.
    process_exit_emitted: bool,
    /// DECCKM: application cursor keys mode.
    pub(crate) application_cursor_keys: bool,
    /// DECTCEM: cursor visibility.
    pub(crate) cursor_visible: bool,
    /// Bracketed paste mode (mode 2004).
    pub(crate) bracketed_paste: bool,
    /// Mouse tracking mode.
    pub(crate) mouse_tracking: MouseTrackingMode,
    /// SGR mouse encoding (mode 1006).
    pub(crate) sgr_mouse: bool,
    /// Focus event tracking (mode 1004).
    pub(crate) focus_tracking: bool,
    /// Scroll region top/bottom (1-based inclusive, None = full screen).
    pub(crate) scroll_region: Option<(usize, usize)>,
    /// Whether synchronized output mode (DECSET 2026) is active.
    /// Note: changes are always applied immediately regardless of this flag.
    /// See apply_or_stage_change() for rationale.
    pub(crate) synchronized_output: bool,
    /// Scrollback buffer (memory + optional disk).
    scrollback: scrollback::Scrollback,
    /// CWD cached from OSC 7 (CurrentWorkingDirectory) sequences emitted by the shell.
    /// Used by get_cwd() to avoid spawning external processes.
    pub(crate) cached_cwd: Option<std::path::PathBuf>,
    /// Saved right-side cells for each line, preserved when cols shrink.
    /// Each entry corresponds to a screen line and holds cells beyond the current cols.
    /// Restored when cols grow again. Cleared on scrollback capture (scroll up).
    saved_line_tails: Vec<Vec<(String, CellAttributes)>>,
    /// Pending PTY resize: surface is updated immediately, but PTY notification
    /// is throttled to avoid SIGWINCH storms during continuous window drag.
    pending_pty_resize: Option<(usize, usize)>,
    /// Number of scrollback lines that were pushed by `handle_rows_shrink` and
    /// are awaiting a symmetric `handle_rows_grow` to restore them. Without
    /// this counter, every grow would unconditionally pop from scrollback —
    /// pulling injected/historical lines (e.g. previous-session restore) into
    /// the visible area even though no corresponding shrink pushed anything,
    /// causing visible content to accumulate on repeated resize cycles.
    restorable_scrollback_count: usize,
    /// Timestamp of the last actual PTY resize flush. Used for throttling.
    last_pty_flush: std::time::Instant,
    /// Timestamp of the most recent non-empty PTY output processed by this terminal.
    /// Used by `is_busy()` to drop the busy state when a foreground program goes
    /// quiet (e.g. claude waiting for the next prompt).
    last_output_at: std::time::Instant,
    /// Timestamp of the most recent user input sent to the PTY via `send_key()`
    /// or `send_bytes()`. Used by `is_busy()` to distinguish user-typing echo
    /// from genuine program output.
    last_input_at: std::time::Instant,
}

/// How long after the last PTY output a terminal still counts as busy.
/// Tuned so that bursty token streams (claude, llms, tail -f) stay marked
/// while genuinely idle TUIs (vim sitting still, claude waiting for input)
/// drop out within a couple of polling intervals.
pub(crate) const BUSY_OUTPUT_WINDOW: std::time::Duration = std::time::Duration::from_secs(2);

/// Maximum delay between user input and its PTY echo. Output arriving within
/// this window after the last `send_key()`/`send_bytes()` is treated as echo
/// and does NOT count toward the busy indicator. Program-generated output
/// (token streams, build logs) arrives well after this threshold.
pub(crate) const INPUT_ECHO_WINDOW: std::time::Duration = std::time::Duration::from_millis(200);

impl Terminal {
    /// Create a new terminal.
    ///
    /// If `config.shell` is `None` or empty, the platform default shell is used.
    /// The `waker` callback is invoked from the PTY reader thread whenever new data
    /// arrives, allowing the main event loop to wake up and process the output.
    pub fn new(config: TerminalConfig<'_>, waker: Waker) -> Result<Self> {
        let cols = config.cols;
        let rows = config.rows;
        let surface_id = config.surface_id;
        let working_dir = config.working_dir;
        let pty_system = NativePtySystem::default();

        let pair = pty_system.openpty(PtySize {
            rows: rows as u16,
            cols: cols as u16,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let shell = match config.shell {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => Self::default_shell(),
        };
        let mut cmd = CommandBuilder::new(&shell);
        // Launch as interactive login shell so .zshrc/.bashrc and themes are loaded.
        // On Windows, cmd.exe and powershell don't understand Unix-style -li flags.
        #[cfg(not(windows))]
        cmd.arg("-li");
        for arg in config.args {
            if !arg.is_empty() {
                cmd.arg(arg);
            }
        }
        cmd.env("TERM", "xterm-256color");
        cmd.env("TASTY_SURFACE_ID", surface_id.to_string());

        // Remove CMUX_* environment variables so cmux CLI doesn't work inside tasty terminals.
        // tasty inherits these from the parent process when launched from cmux.
        for (key, _) in std::env::vars() {
            if key.starts_with("CMUX_") {
                cmd.env_remove(&key);
            }
        }

        // Add tasty's own binary directory to PATH so `tasty` CLI works inside the terminal
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                let exe_dir_str = exe_dir.to_string_lossy();
                let sep = if cfg!(windows) { ";" } else { ":" };
                let new_path = if let Ok(existing) = std::env::var("PATH") {
                    format!("{}{}{}", exe_dir_str, sep, existing)
                } else {
                    exe_dir_str.to_string()
                };
                cmd.env("PATH", new_path);
            }
        }

        if let Some(dir) = working_dir {
            cmd.cwd(dir);
        }

        let child = pair.slave.spawn_command(cmd)?;
        drop(pair.slave);

        let mut pty_writer = pair.master.take_writer()?;
        let mut pty_reader = pair.master.try_clone_reader()?;

        // PTY master 의 첫 바이트로 initial_input 을 동기 write. child 가 stdin 을
        // 처음 read 하는 순간 이 바이트가 무조건 들어간다. writer thread 의 채널
        // 경유로는 미세한 race 또는 첫 write 가 지연되는 케이스가 있어, 직접 master
        // fd 에 써서 timing 을 결정적으로 만든다.
        if let Some(input) = config.initial_input {
            if !input.is_empty() {
                if let Err(e) = pty_writer.write_all(input.as_bytes()) {
                    tracing::warn!("initial_input write_all failed: {e}");
                } else if let Err(e) = pty_writer.flush() {
                    tracing::warn!("initial_input flush failed: {e}");
                }
            }
        }

        // Writer thread: drains queued writes to PTY without blocking the main thread.
        let (write_tx, write_rx) = mpsc::channel::<Vec<u8>>();
        let writer_thread = thread::spawn(move || {
            while let Ok(data) = write_rx.recv() {
                if pty_writer.write_all(&data).is_err() {
                    break;
                }
                if pty_writer.flush().is_err() {
                    break;
                }
            }
        });

        let (tx, rx) = mpsc::sync_channel(32); // 32 * 8KB = 256KB max buffered

        let reader_thread = thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match pty_reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                        waker(); // Wake the event loop
                    }
                    Err(_) => break,
                }
            }
        });

        let primary_surface = Surface::new(cols, rows);
        let parser = Parser::new();

        Ok(Self {
            primary_surface,
            alternate_surface: None,
            use_alternate: false,
            parser,
            pty_write_tx: write_tx,
            _writer_thread: writer_thread,
            pty_master: pair.master,
            child,
            action_rx: rx,
            _reader_thread: reader_thread,
            cols,
            rows,
            saved_cursor: None,
            alt_saved_cursor: None,
            events: Vec::new(),
            output: output_buffer::OutputBuffer::new(),
            process_exit_emitted: false,
            application_cursor_keys: false,
            cursor_visible: true,
            bracketed_paste: false,
            mouse_tracking: MouseTrackingMode::None,
            sgr_mouse: false,
            focus_tracking: false,
            scroll_region: None,
            synchronized_output: false,
            scrollback: scrollback::Scrollback::new(),
            cached_cwd: None,
            saved_line_tails: Vec::new(),
            pending_pty_resize: None,
            restorable_scrollback_count: 0,
            last_pty_flush: std::time::Instant::now(),
            last_output_at: std::time::Instant::now(),
            // Start in the past so the first PTY output is never mistaken for echo.
            last_input_at: std::time::Instant::now() - INPUT_ECHO_WINDOW,
        })
    }

    /// Process pending PTY output. Returns true if surface changed.
    pub fn process(&mut self) -> bool {
        // Flush deferred PTY resize before processing new data
        self.force_flush_pty_resize();

        let mut changed = false;

        while let Ok(data) = self.action_rx.try_recv() {
            self.output.append(&data);
            if !data.is_empty() {
                self.last_output_at = std::time::Instant::now();
            }

            let actions = self.parser.parse_as_vec(&data);
            for action in actions {
                // Intercept Mode actions (DECSET/DECRST) -- they affect Terminal
                // state rather than Surface content.
                if let Action::CSI(CSI::Mode(ref mode)) = action {
                    self.handle_mode(mode);
                    changed = true;
                    continue;
                }
                let changes = self.action_to_changes(action);
                if !changes.is_empty() {
                    for change in changes {
                        self.apply_or_stage_change(change);
                    }
                    changed = true;
                }
            }
        }

        // Check if the child process has exited (emit event once)
        if !self.process_exit_emitted && !self.check_process_alive() {
            self.process_exit_emitted = true;
            self.events.push(TerminalEvent {
                surface_id: 0,
                kind: TerminalEventKind::ProcessExited,
            });
        }

        changed
    }

    fn default_shell() -> String {
        #[cfg(windows)]
        {
            std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string())
        }
        #[cfg(not(windows))]
        {
            std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())
        }
    }
}

#[cfg(test)]
mod tests;
