use std::collections::VecDeque;
use std::io::{Read, Write};
use std::sync::{LazyLock, mpsc};
use std::thread;

use anyhow::Result;
use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};
use termwiz::cell::CellAttributes;
use termwiz::escape::csi::CSI;
use termwiz::escape::parser::Parser;
use termwiz::escape::Action;
use termwiz::surface::{Change, Surface};

pub mod cwd;
pub mod disk_scrollback;
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
    /// Whether synchronized output mode (DECSET 2026) is active.
    pub(crate) synchronized_output: bool,
    /// Surface changes accumulated while synchronized output mode is active.
    pub(crate) pending_changes: Vec<Change>,
    /// Scrollback buffer: stores lines that scrolled off the top of the screen.
    /// Each line is a vector of (character, CellAttributes) pairs.
    scrollback: VecDeque<Vec<(String, CellAttributes)>>,
    /// Maximum number of scrollback lines.
    scrollback_limit: usize,
    /// Current scroll offset (0 = at bottom/live, >0 = scrolled up).
    pub scroll_offset: usize,
    /// Disk-backed scrollback for older lines (enabled by scrollback_disk_swap setting).
    disk_scrollback: Option<disk_scrollback::DiskScrollback>,
}

impl Terminal {
    pub fn new(cols: usize, rows: usize, surface_id: u32, waker: Waker) -> Result<Self> {
        Self::new_with_shell(cols, rows, None, surface_id, waker)
    }

    /// Create a terminal with an optional custom shell. If `shell` is `None` or empty,
    /// the platform default shell is used.
    ///
    /// The `waker` callback is invoked from the PTY reader thread whenever new data
    /// arrives, allowing the main event loop to wake up and process the output.
    pub fn new_with_shell(cols: usize, rows: usize, shell: Option<&str>, surface_id: u32, waker: Waker) -> Result<Self> {
        Self::new_with_shell_args(cols, rows, shell, &[], surface_id, waker)
    }

    pub fn new_with_shell_args(cols: usize, rows: usize, shell: Option<&str>, args: &[&str], surface_id: u32, waker: Waker) -> Result<Self> {
        Self::new_with_shell_args_cwd(cols, rows, shell, args, surface_id, waker, None)
    }

    pub fn new_with_shell_args_cwd(cols: usize, rows: usize, shell: Option<&str>, args: &[&str], surface_id: u32, waker: Waker, working_dir: Option<&std::path::Path>) -> Result<Self> {
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
        // Launch as interactive login shell so .zshrc/.bashrc and themes are loaded.
        // On Windows, cmd.exe and powershell don't understand Unix-style -li flags.
        #[cfg(not(windows))]
        cmd.arg("-li");
        for arg in args {
            if !arg.is_empty() {
                cmd.arg(arg);
            }
        }
        cmd.env("TERM", "xterm-256color");
        cmd.env("TASTY_SURFACE_ID", surface_id.to_string());

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
            synchronized_output: false,
            pending_changes: Vec::new(),
            scrollback: VecDeque::new(),
            scrollback_limit: 10000,
            scroll_offset: 0,
            disk_scrollback: None,
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
                self.apply_or_stage_change(change);
            }
        }
    }

    /// Send keyboard input to PTY
    pub fn send_key(&mut self, text: &str) {
        let _ = self.pty_writer.write_all(text.as_bytes());
        let _ = self.pty_writer.flush();
    }

    pub(crate) fn send_terminal_response(&mut self, response: &str) {
        let _ = self.pty_writer.write_all(response.as_bytes());
        let _ = self.pty_writer.flush();
    }

    pub(crate) fn apply_or_stage_change(&mut self, change: Change) {
        if self.synchronized_output {
            self.pending_changes.push(change);
            return;
        }
        self.apply_change(change);
    }

    pub(crate) fn flush_pending_changes(&mut self) {
        if self.pending_changes.is_empty() {
            return;
        }
        for change in std::mem::take(&mut self.pending_changes) {
            self.apply_change(change);
        }
    }

    fn apply_change(&mut self, change: Change) {
        if !self.use_alternate {
            self.capture_before_scroll(&change);
        }
        self.surface_mut().add_change(change);
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

    /// Get the PID of the child process.
    pub fn process_id(&self) -> Option<u32> {
        self.child.process_id()
    }

    /// Get the current working directory of the child process.
    pub fn get_cwd(&self) -> Option<std::path::PathBuf> {
        let pid = self.child.process_id()?;
        cwd::get_cwd_of_pid(pid)
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

    // ---- Scrollback buffer methods ----

    /// Set the scrollback buffer limit.
    pub fn set_scrollback_limit(&mut self, limit: usize) {
        self.scrollback_limit = limit;
        self.flush_scrollback_to_disk();
    }

    /// Enable disk-backed scrollback swap for this terminal.
    pub fn enable_disk_scrollback(&mut self, surface_id: u32) {
        if self.disk_scrollback.is_none() {
            match disk_scrollback::DiskScrollback::new(surface_id) {
                Ok(ds) => self.disk_scrollback = Some(ds),
                Err(e) => tracing::warn!("failed to create disk scrollback: {e}"),
            }
        }
    }

    /// Flush excess scrollback lines to disk (if disk swap is enabled).
    fn flush_scrollback_to_disk(&mut self) {
        while self.scrollback.len() > self.scrollback_limit {
            if let Some(ds) = &mut self.disk_scrollback {
                if let Some(line) = self.scrollback.pop_front() {
                    let _ = ds.push_lines(&[line]);
                }
            } else {
                self.scrollback.pop_front();
            }
        }
    }

    /// Scroll up (towards older content).
    pub fn scroll_up(&mut self, lines: usize) {
        let max = self.scrollback_len();
        self.scroll_offset = (self.scroll_offset + lines).min(max);
    }

    /// Scroll down (towards newer/live content).
    pub fn scroll_down(&mut self, lines: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(lines);
    }

    /// Reset scroll position to the bottom (live view).
    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
    }

    /// Number of lines in the scrollback buffer (memory + disk).
    pub fn scrollback_len(&self) -> usize {
        let disk_count = self.disk_scrollback.as_ref().map(|ds| ds.line_count()).unwrap_or(0);
        disk_count + self.scrollback.len()
    }

    /// Get a specific scrollback line by index (0 = oldest, memory only).
    /// For disk-backed lines, use scrollback_line_owned().
    pub fn scrollback_line(&self, index: usize) -> Option<&Vec<(String, CellAttributes)>> {
        let disk_count = self.disk_scrollback.as_ref().map(|ds| ds.line_count()).unwrap_or(0);
        if index < disk_count {
            None // Disk lines can't be returned as reference — use scrollback_line_owned()
        } else {
            self.scrollback.get(index - disk_count)
        }
    }

    /// Get a scrollback line by index, returning owned data.
    /// Works for both memory and disk-backed lines.
    pub fn scrollback_line_owned(&self, index: usize) -> Option<Vec<(String, CellAttributes)>> {
        let disk_count = self.disk_scrollback.as_ref().map(|ds| ds.line_count()).unwrap_or(0);
        if index < disk_count {
            self.disk_scrollback.as_ref()
                .and_then(|ds| ds.read_line(index).ok().flatten())
        } else {
            self.scrollback.get(index - disk_count).cloned()
        }
    }

    /// Capture the top line(s) from the surface before a scroll change is applied.
    fn capture_top_lines(&self, count: usize) -> Vec<Vec<(String, CellAttributes)>> {
        let surface = self.surface();
        let lines = surface.screen_lines();
        let mut result = Vec::new();
        for i in 0..count.min(lines.len()) {
            let line: Vec<(String, CellAttributes)> = lines[i]
                .visible_cells()
                .map(|cell| (cell.str().to_string(), cell.attrs().clone()))
                .collect();
            result.push(line);
        }
        result
    }

    /// Inspect a change and capture scrollback lines before it's applied.
    fn capture_before_scroll(&mut self, change: &Change) {
        match change {
            Change::ScrollRegionUp { first_row, scroll_count, .. } if *first_row == 0 => {
                let captured = self.capture_top_lines(*scroll_count);
                let count = captured.len();
                for line in captured {
                    self.scrollback.push_back(line);
                }
                // Compensate scroll_offset so the user's viewport stays in place
                if self.scroll_offset > 0 {
                    self.scroll_offset += count;
                }
                self.flush_scrollback_to_disk();
            }
            _ => {}
        }
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
        let mut terminal = Terminal::new(80, 24, 0, waker).expect("terminal creation");
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
        let mut terminal = Terminal::new(80, 24, 0, waker).expect("terminal creation");
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
        let mut terminal = Terminal::new(80, 24, 0, waker).expect("terminal creation");
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
        let mut terminal = Terminal::new(80, 24, 0, waker).expect("terminal creation");
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
        let mut terminal = Terminal::new(80, 24, 0, waker).expect("terminal creation");
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
        let mut terminal = Terminal::new(80, 24, 0, waker).expect("terminal creation");

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
        let mut terminal = Terminal::new(80, 24, 0, waker).expect("terminal creation");

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
        let mut terminal = Terminal::new(80, 24, 0, waker).expect("terminal creation");

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
        let mut terminal = Terminal::new(80, 24, 0, waker).expect("terminal creation");

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
