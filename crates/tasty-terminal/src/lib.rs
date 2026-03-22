use std::io::{Read, Write};
use std::sync::{LazyLock, mpsc};
use std::thread;

use anyhow::Result;
use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};
use termwiz::escape::csi::CSI;
use termwiz::escape::parser::Parser;
use termwiz::escape::Action;
use termwiz::surface::Surface;

mod events;
mod modes;
pub mod test_helpers;
mod vte_handler;

pub use events::*;

/// Maximum size of the output buffer (1 MB).
const OUTPUT_BUFFER_MAX: usize = 1_048_576;

pub struct Terminal {
    /// Primary screen buffer.
    pub(crate) primary_surface: Surface,
    /// Alternate screen buffer (lazily created on DECSET 1049/47).
    pub(crate) alternate_surface: Option<Surface>,
    /// Whether the alternate screen is active.
    pub(crate) use_alternate: bool,
    parser: Parser,
    pty_writer: Box<dyn Write + Send>,
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
    /// Raw PTY output history for read-mark API.
    output_buffer: Vec<u8>,
    /// Byte offset of the read mark in the output buffer.
    read_mark: Option<usize>,
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
}

impl Terminal {
    pub fn new(cols: usize, rows: usize, waker: Waker) -> Result<Self> {
        Self::new_with_shell(cols, rows, None, waker)
    }

    /// Create a terminal with an optional custom shell. If `shell` is `None` or empty,
    /// the platform default shell is used.
    ///
    /// The `waker` callback is invoked from the PTY reader thread whenever new data
    /// arrives, allowing the main event loop to wake up and process the output.
    pub fn new_with_shell(cols: usize, rows: usize, shell: Option<&str>, waker: Waker) -> Result<Self> {
        let pty_system = NativePtySystem::default();

        let pair = pty_system.openpty(PtySize {
            rows: rows as u16,
            cols: cols as u16,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let shell = match shell {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => Self::default_shell(),
        };
        let mut cmd = CommandBuilder::new(&shell);
        cmd.env("TERM", "xterm-256color");

        let child = pair.slave.spawn_command(cmd)?;
        drop(pair.slave);

        let pty_writer = pair.master.take_writer()?;
        let mut pty_reader = pair.master.try_clone_reader()?;

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
            pty_writer,
            pty_master: pair.master,
            child,
            action_rx: rx,
            _reader_thread: reader_thread,
            cols,
            rows,
            saved_cursor: None,
            alt_saved_cursor: None,
            events: Vec::new(),
            output_buffer: Vec::new(),
            read_mark: None,
            process_exit_emitted: false,
            application_cursor_keys: false,
            cursor_visible: true,
            bracketed_paste: false,
            mouse_tracking: MouseTrackingMode::None,
            sgr_mouse: false,
            focus_tracking: false,
            scroll_region: None,
        })
    }

    /// Process pending PTY output. Returns true if surface changed.
    pub fn process(&mut self) -> bool {
        let mut changed = false;

        while let Ok(data) = self.action_rx.try_recv() {
            // Accumulate raw bytes for read-mark API
            self.output_buffer.extend_from_slice(&data);
            // Trim to max size
            if self.output_buffer.len() > OUTPUT_BUFFER_MAX {
                let excess = self.output_buffer.len() - OUTPUT_BUFFER_MAX;
                self.output_buffer.drain(..excess);
                // Adjust mark if it was in the trimmed region
                if let Some(mark) = &mut self.read_mark {
                    if *mark <= excess {
                        self.read_mark = None; // mark was in trimmed region, invalidate
                    } else {
                        *mark -= excess;
                    }
                }
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
                        self.surface_mut().add_change(change);
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

    /// Get the visible text content of the screen as a string.
    /// Each row is on its own line, trailing spaces are trimmed.
    pub fn screen_text(&self) -> String {
        let surface = self.surface();
        let lines = surface.screen_lines();
        let mut result = String::new();
        for line in lines {
            let mut row_text = String::new();
            for cell in line.visible_cells() {
                row_text.push_str(cell.str());
            }
            result.push_str(row_text.trim_end());
            result.push('\n');
        }
        // Trim trailing empty lines
        while result.ends_with("\n\n") {
            result.pop();
        }
        result
    }

    /// Get the text of a specific row (0-indexed), trimmed.
    pub fn screen_row(&self, row: usize) -> String {
        let surface = self.surface();
        let lines = surface.screen_lines();
        if row >= lines.len() {
            return String::new();
        }
        let mut text = String::new();
        for cell in lines[row].visible_cells() {
            text.push_str(cell.str());
        }
        text.trim_end().to_string()
    }

    /// Process raw bytes through the VTE parser and apply to the surface.
    /// This is useful for testing without a real PTY.
    pub fn process_bytes(&mut self, data: &[u8]) {
        let actions = self.parser.parse_as_vec(data);
        for action in actions {
            if let Action::CSI(CSI::Mode(ref mode)) = action {
                self.handle_mode(mode);
                continue;
            }
            let changes = self.action_to_changes(action);
            for change in changes {
                self.surface_mut().add_change(change);
            }
        }
    }

    /// Send keyboard input to PTY
    pub fn send_key(&mut self, text: &str) {
        let _ = self.pty_writer.write_all(text.as_bytes());
        let _ = self.pty_writer.flush();
    }

    /// Send raw bytes to PTY
    pub fn send_bytes(&mut self, bytes: &[u8]) {
        let _ = self.pty_writer.write_all(bytes);
        let _ = self.pty_writer.flush();
    }

    pub fn resize(&mut self, cols: usize, rows: usize) {
        self.cols = cols;
        self.rows = rows;
        self.primary_surface.resize(cols, rows);
        if let Some(alt) = &mut self.alternate_surface {
            alt.resize(cols, rows);
        }
        // Reset scroll region on resize
        self.scroll_region = None;

        // Propagate resize to the PTY so the child process knows the new size
        let _ = self.pty_master.resize(PtySize {
            rows: rows as u16,
            cols: cols as u16,
            pixel_width: 0,
            pixel_height: 0,
        });
    }

    pub fn surface(&self) -> &Surface {
        if self.use_alternate {
            self.alternate_surface
                .as_ref()
                .unwrap_or(&self.primary_surface)
        } else {
            &self.primary_surface
        }
    }

    pub(crate) fn surface_mut(&mut self) -> &mut Surface {
        if self.use_alternate {
            self.alternate_surface
                .as_mut()
                .unwrap_or(&mut self.primary_surface)
        } else {
            &mut self.primary_surface
        }
    }

    pub fn cols(&self) -> usize {
        self.cols
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    /// Check if the child process is still running.
    pub fn is_alive(&mut self) -> bool {
        self.child.try_wait().ok().flatten().is_none()
    }

    /// Check if the child process has exited. Returns false if exited.
    pub fn check_process_alive(&mut self) -> bool {
        match self.child.try_wait() {
            Ok(Some(_status)) => false, // exited
            _ => true,
        }
    }

    /// Take all accumulated events, leaving the internal buffer empty.
    pub fn take_events(&mut self) -> Vec<TerminalEvent> {
        std::mem::take(&mut self.events)
    }

    /// Set a read mark at the current end of the output buffer.
    pub fn set_mark(&mut self) {
        self.read_mark = Some(self.output_buffer.len());
    }

    /// Read output since the last mark. If no mark was set, reads from the beginning.
    pub fn read_since_mark(&self, strip_ansi: bool) -> String {
        let start = self.read_mark.unwrap_or(0);
        let bytes = &self.output_buffer[start..];
        let text = String::from_utf8_lossy(bytes).to_string();
        if strip_ansi {
            strip_ansi_escapes(&text)
        } else {
            text
        }
    }

    // ---- Public getters for terminal state ----

    /// Whether application cursor keys mode is active (DECCKM).
    pub fn application_cursor_keys(&self) -> bool {
        self.application_cursor_keys
    }

    /// Whether the cursor is visible (DECTCEM).
    pub fn cursor_visible(&self) -> bool {
        self.cursor_visible
    }

    /// Whether bracketed paste mode is active.
    pub fn bracketed_paste(&self) -> bool {
        self.bracketed_paste
    }

    /// Current mouse tracking mode.
    pub fn mouse_tracking(&self) -> MouseTrackingMode {
        self.mouse_tracking
    }

    /// Whether SGR mouse encoding is active.
    pub fn sgr_mouse(&self) -> bool {
        self.sgr_mouse
    }

    /// Whether focus tracking is active.
    pub fn focus_tracking(&self) -> bool {
        self.focus_tracking
    }

    /// Whether the alternate screen is active.
    pub fn is_alternate_screen(&self) -> bool {
        self.use_alternate
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

static ANSI_ESCAPE_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"\x1b\[[0-9;]*[a-zA-Z]|\x1b\][^\x07]*\x07|\x1b\][^\x1b]*\x1b\\")
        .expect("static regex is valid")
});

/// Strip ANSI escape sequences from a string using regex.
fn strip_ansi_escapes(s: &str) -> String {
    ANSI_ESCAPE_RE.replace_all(s, "").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use termwiz::escape::csi::CSI;
    use termwiz::escape::parser::Parser;

    fn noop_waker() -> Waker {
        Arc::new(|| {})
    }

    // ---- DECSET/DECRST mode toggling tests ----

    #[test]
    fn decset_application_cursor_keys() {
        let waker = noop_waker();
        let mut terminal = Terminal::new(80, 24, waker).expect("terminal creation");
        assert!(!terminal.application_cursor_keys());

        let mut parser = Parser::new();
        let actions = parser.parse_as_vec(b"\x1b[?1h");
        for action in actions {
            if let Action::CSI(CSI::Mode(ref mode)) = action {
                terminal.handle_mode(mode);
            }
        }
        assert!(terminal.application_cursor_keys());

        let actions = parser.parse_as_vec(b"\x1b[?1l");
        for action in actions {
            if let Action::CSI(CSI::Mode(ref mode)) = action {
                terminal.handle_mode(mode);
            }
        }
        assert!(!terminal.application_cursor_keys());
    }

    #[test]
    fn decset_cursor_visibility() {
        let waker = noop_waker();
        let mut terminal = Terminal::new(80, 24, waker).expect("terminal creation");
        assert!(terminal.cursor_visible());

        let mut parser = Parser::new();
        let actions = parser.parse_as_vec(b"\x1b[?25l");
        for action in actions {
            if let Action::CSI(CSI::Mode(ref mode)) = action {
                terminal.handle_mode(mode);
            }
        }
        assert!(!terminal.cursor_visible());

        let actions = parser.parse_as_vec(b"\x1b[?25h");
        for action in actions {
            if let Action::CSI(CSI::Mode(ref mode)) = action {
                terminal.handle_mode(mode);
            }
        }
        assert!(terminal.cursor_visible());
    }

    #[test]
    fn decset_bracketed_paste() {
        let waker = noop_waker();
        let mut terminal = Terminal::new(80, 24, waker).expect("terminal creation");
        assert!(!terminal.bracketed_paste());

        let mut parser = Parser::new();
        let actions = parser.parse_as_vec(b"\x1b[?2004h");
        for action in actions {
            if let Action::CSI(CSI::Mode(ref mode)) = action {
                terminal.handle_mode(mode);
            }
        }
        assert!(terminal.bracketed_paste());

        let actions = parser.parse_as_vec(b"\x1b[?2004l");
        for action in actions {
            if let Action::CSI(CSI::Mode(ref mode)) = action {
                terminal.handle_mode(mode);
            }
        }
        assert!(!terminal.bracketed_paste());
    }

    #[test]
    fn decset_mouse_tracking() {
        let waker = noop_waker();
        let mut terminal = Terminal::new(80, 24, waker).expect("terminal creation");
        assert_eq!(terminal.mouse_tracking(), MouseTrackingMode::None);

        let mut parser = Parser::new();
        let actions = parser.parse_as_vec(b"\x1b[?1000h");
        for action in actions {
            if let Action::CSI(CSI::Mode(ref mode)) = action {
                terminal.handle_mode(mode);
            }
        }
        assert_eq!(terminal.mouse_tracking(), MouseTrackingMode::Click);

        let actions = parser.parse_as_vec(b"\x1b[?1003h");
        for action in actions {
            if let Action::CSI(CSI::Mode(ref mode)) = action {
                terminal.handle_mode(mode);
            }
        }
        assert_eq!(terminal.mouse_tracking(), MouseTrackingMode::AllMotion);

        let actions = parser.parse_as_vec(b"\x1b[?1003l");
        for action in actions {
            if let Action::CSI(CSI::Mode(ref mode)) = action {
                terminal.handle_mode(mode);
            }
        }
        assert_eq!(terminal.mouse_tracking(), MouseTrackingMode::None);
    }

    // ---- Alternate screen tests ----

    #[test]
    fn alternate_screen_switching() {
        let waker = noop_waker();
        let mut terminal = Terminal::new(80, 24, waker).expect("terminal creation");
        assert!(!terminal.is_alternate_screen());

        let mut parser = Parser::new();
        let actions = parser.parse_as_vec(b"\x1b[?1049h");
        for action in actions {
            if let Action::CSI(CSI::Mode(ref mode)) = action {
                terminal.handle_mode(mode);
            }
        }
        assert!(terminal.is_alternate_screen());
        assert!(terminal.alternate_surface.is_some());

        let actions = parser.parse_as_vec(b"\x1b[?1049l");
        for action in actions {
            if let Action::CSI(CSI::Mode(ref mode)) = action {
                terminal.handle_mode(mode);
            }
        }
        assert!(!terminal.is_alternate_screen());
    }

    #[test]
    fn alternate_screen_mode_47() {
        let waker = noop_waker();
        let mut terminal = Terminal::new(80, 24, waker).expect("terminal creation");

        let mut parser = Parser::new();
        let actions = parser.parse_as_vec(b"\x1b[?47h");
        for action in actions {
            if let Action::CSI(CSI::Mode(ref mode)) = action {
                terminal.handle_mode(mode);
            }
        }
        assert!(terminal.is_alternate_screen());

        let actions = parser.parse_as_vec(b"\x1b[?47l");
        for action in actions {
            if let Action::CSI(CSI::Mode(ref mode)) = action {
                terminal.handle_mode(mode);
            }
        }
        assert!(!terminal.is_alternate_screen());
    }

    #[test]
    fn alternate_screen_resize() {
        let waker = noop_waker();
        let mut terminal = Terminal::new(80, 24, waker).expect("terminal creation");

        let mut parser = Parser::new();
        let actions = parser.parse_as_vec(b"\x1b[?1049h");
        for action in actions {
            if let Action::CSI(CSI::Mode(ref mode)) = action {
                terminal.handle_mode(mode);
            }
        }

        terminal.resize(120, 40);
        assert_eq!(terminal.cols(), 120);
        assert_eq!(terminal.rows(), 40);
        let (cols, rows) = terminal.surface().dimensions();
        assert_eq!(cols, 120);
        assert_eq!(rows, 40);
    }

    // ---- Arrow key mode switching ----

    #[test]
    fn arrow_key_sequences_normal_vs_application() {
        let waker = noop_waker();
        let mut terminal = Terminal::new(80, 24, waker).expect("terminal creation");

        assert!(!terminal.application_cursor_keys());
        let mut parser = Parser::new();
        let actions = parser.parse_as_vec(b"\x1b[?1h");
        for action in actions {
            if let Action::CSI(CSI::Mode(ref mode)) = action {
                terminal.handle_mode(mode);
            }
        }
        assert!(terminal.application_cursor_keys());
    }

    // ---- Full reset test ----

    #[test]
    fn full_reset_clears_modes() {
        let waker = noop_waker();
        let mut terminal = Terminal::new(80, 24, waker).expect("terminal creation");

        let mut parser = Parser::new();
        let data = b"\x1b[?1h\x1b[?25l\x1b[?2004h\x1b[?1049h";
        let actions = parser.parse_as_vec(data);
        for action in actions {
            if let Action::CSI(CSI::Mode(ref mode)) = action {
                terminal.handle_mode(mode);
            }
        }
        assert!(terminal.application_cursor_keys());
        assert!(!terminal.cursor_visible());
        assert!(terminal.bracketed_paste());
        assert!(terminal.is_alternate_screen());

        let actions = parser.parse_as_vec(b"\x1bc");
        for action in actions {
            let _changes = terminal.action_to_changes(action);
        }
        assert!(!terminal.application_cursor_keys());
        assert!(terminal.cursor_visible());
        assert!(!terminal.bracketed_paste());
        assert!(!terminal.is_alternate_screen());
    }
}
