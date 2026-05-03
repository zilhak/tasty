use std::io::{Read, Write};
use std::sync::mpsc;
use std::thread;

use anyhow::Result;
use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};
use termwiz::cell::{CellAttributes, Underline};
use termwiz::color::ColorAttribute;
use termwiz::escape::Action;
use termwiz::escape::csi::CSI;
use termwiz::escape::parser::Parser;
use termwiz::surface::{Change, Surface};

pub mod cwd;
pub mod disk_scrollback;
mod events;
pub mod foreground_process;
mod modes;
mod output_buffer;
mod scrollback;
pub mod test_helpers;
mod vte_handler;

pub use events::*;

/// Configuration for creating a new Terminal.
pub struct TerminalConfig<'a> {
    pub cols: usize,
    pub rows: usize,
    pub shell: Option<&'a str>,
    pub args: &'a [&'a str],
    pub surface_id: u32,
    pub working_dir: Option<&'a std::path::Path>,
}

/// Information about a single cell for debug inspection.
#[derive(Debug, Clone)]
pub struct CellInfo {
    pub text: String,
    pub fg: String,
    pub bg: String,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub inverse: bool,
    pub width: usize,
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
    /// Timestamp of the last actual PTY resize flush. Used for throttling.
    last_pty_flush: std::time::Instant,
}

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
            last_pty_flush: std::time::Instant::now(),
        })
    }

    /// Process pending PTY output. Returns true if surface changed.
    pub fn process(&mut self) -> bool {
        // Flush deferred PTY resize before processing new data
        self.force_flush_pty_resize();

        let mut changed = false;

        while let Ok(data) = self.action_rx.try_recv() {
            self.output.append(&data);

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

    /// Get the last N lines of terminal output (screen + scrollback from the bottom).
    /// If N is larger than available lines, returns everything available.
    pub fn screen_text_lines(&self, n: usize) -> String {
        let surface = self.surface();
        let screen_lines = surface.screen_lines();
        let screen_count = screen_lines.len();
        let scrollback_total = self.scrollback_len();

        if n <= screen_count {
            // Only need lines from the current screen (bottom N rows)
            let start = screen_count - n;
            let mut result = String::new();
            for line in &screen_lines[start..] {
                let mut row_text = String::new();
                for cell in line.visible_cells() {
                    row_text.push_str(cell.str());
                }
                result.push_str(row_text.trim_end());
                result.push('\n');
            }
            while result.ends_with("\n\n") {
                result.pop();
            }
            result
        } else {
            // Need scrollback lines + full screen
            let scrollback_needed = (n - screen_count).min(scrollback_total);
            let scrollback_start = scrollback_total - scrollback_needed;

            let mut result = String::new();

            // Append scrollback lines (from scrollback_start to end)
            for i in scrollback_start..scrollback_total {
                let line_text = self
                    .scrollback_line_owned(i)
                    .map(|cells| {
                        cells
                            .iter()
                            .map(|(s, _)| s.as_str())
                            .collect::<String>()
                    })
                    .unwrap_or_default();
                result.push_str(line_text.trim_end());
                result.push('\n');
            }

            // Append all screen lines
            for line in screen_lines {
                let mut row_text = String::new();
                for cell in line.visible_cells() {
                    row_text.push_str(cell.str());
                }
                result.push_str(row_text.trim_end());
                result.push('\n');
            }

            while result.ends_with("\n\n") {
                result.pop();
            }
            result
        }
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

    /// Get detailed information about a specific cell (row, col) on the current screen.
    /// Returns None if row/col is out of bounds.
    pub fn cell_info(&self, row: usize, col: usize) -> Option<CellInfo> {
        let surface = self.surface();
        let lines = surface.screen_lines();
        if row >= lines.len() {
            return None;
        }
        for cell in lines[row].visible_cells() {
            if cell.cell_index() == col {
                let attrs = cell.attrs();
                let width = if cell.str().chars().next().map_or(false, |c| {
                    unicode_width::UnicodeWidthChar::width(c).unwrap_or(1) > 1
                }) {
                    2
                } else {
                    1
                };
                return Some(CellInfo {
                    text: cell.str().to_string(),
                    fg: Self::color_attr_to_string(&attrs.foreground()),
                    bg: Self::color_attr_to_string(&attrs.background()),
                    bold: attrs.intensity() == termwiz::cell::Intensity::Bold,
                    italic: attrs.italic(),
                    underline: attrs.underline() != Underline::None,
                    strikethrough: attrs.strikethrough(),
                    inverse: attrs.reverse(),
                    width,
                });
            }
        }
        None
    }

    /// Get cell info for all cells in a specific row.
    /// Returns empty vec if row is out of bounds.
    pub fn row_cells(&self, row: usize) -> Vec<(usize, CellInfo)> {
        let surface = self.surface();
        let lines = surface.screen_lines();
        if row >= lines.len() {
            return Vec::new();
        }
        lines[row]
            .visible_cells()
            .map(|cell| {
                let attrs = cell.attrs();
                let width = if cell.str().chars().next().map_or(false, |c| {
                    unicode_width::UnicodeWidthChar::width(c).unwrap_or(1) > 1
                }) {
                    2
                } else {
                    1
                };
                (
                    cell.cell_index(),
                    CellInfo {
                        text: cell.str().to_string(),
                        fg: Self::color_attr_to_string(&attrs.foreground()),
                        bg: Self::color_attr_to_string(&attrs.background()),
                        bold: attrs.intensity() == termwiz::cell::Intensity::Bold,
                        italic: attrs.italic(),
                        underline: attrs.underline() != Underline::None,
                        strikethrough: attrs.strikethrough(),
                        inverse: attrs.reverse(),
                        width,
                    },
                )
            })
            .collect()
    }

    fn color_attr_to_string(attr: &ColorAttribute) -> String {
        match attr {
            ColorAttribute::Default => "default".to_string(),
            ColorAttribute::PaletteIndex(idx) => format!("palette:{idx}"),
            ColorAttribute::TrueColorWithPaletteFallback(srgba, _)
            | ColorAttribute::TrueColorWithDefaultFallback(srgba) => {
                format!(
                    "#{:02x}{:02x}{:02x}",
                    (srgba.0 * 255.0) as u8,
                    (srgba.1 * 255.0) as u8,
                    (srgba.2 * 255.0) as u8
                )
            }
        }
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

    /// Send keyboard input to PTY (non-blocking, queued to writer thread).
    pub fn send_key(&self, text: &str) {
        let _ = self.pty_write_tx.send(text.as_bytes().to_vec());
    }

    pub(crate) fn send_terminal_response(&self, response: &str) {
        let _ = self.pty_write_tx.send(response.as_bytes().to_vec());
    }

    pub(crate) fn apply_or_stage_change(&mut self, change: Change) {
        // Always apply changes immediately to keep surface state (especially
        // cursor position) current. Many VTE operations (EraseLine, DeleteLine,
        // EraseCharacter, etc.) read cursor_position() at generation time to
        // produce absolute-positioned changes. If changes are staged during
        // synchronized output (mode 2026), cursor_position() returns stale
        // values, causing those operations to target wrong rows/columns.
        //
        // Tasty's architecture is process-then-render: all PTY data is processed
        // before the GPU reads the surface, so immediate application doesn't
        // cause visual tearing — the renderer always sees the final state.
        self.apply_change(change);
    }

    fn apply_change(&mut self, change: Change) {
        if self.use_alternate {
            self.surface_mut().add_change(change);
            return;
        }

        self.capture_before_scroll(&change);
        self.surface_mut().add_change(change);
    }

    /// Send raw bytes to PTY (non-blocking, queued to writer thread).
    pub fn send_bytes(&self, bytes: &[u8]) {
        let _ = self.pty_write_tx.send(bytes.to_vec());
    }

    pub fn resize(&mut self, cols: usize, rows: usize) {
        if self.cols == cols && self.rows == rows {
            return;
        }

        let old_cols = self.cols;
        let old_rows = self.rows;

        // Save/restore line tails on the primary surface when cols change
        if cols != old_cols && !self.use_alternate {
            self.save_or_restore_line_tails(old_cols, cols, rows);
        }

        // Handle rows shrink BEFORE resize (need to capture lines before they're lost)
        if rows < old_rows && !self.use_alternate {
            self.handle_rows_shrink(rows, old_rows);
        }

        // Save cursor position before resize for grow restoration
        let old_cursor = self.primary_surface.cursor_position();

        self.cols = cols;
        self.rows = rows;
        self.primary_surface.resize(cols, rows);
        if let Some(alt) = &mut self.alternate_surface {
            alt.resize(cols, rows);
        }

        // Handle rows grow AFTER resize (surface now has room for ScrollRegionDown)
        let mut rows_restored = 0usize;
        if rows > old_rows && !self.use_alternate {
            rows_restored = self.handle_rows_grow(rows, old_rows);
        }

        // Restore saved tails onto the surface after resize expanded cols
        if cols > old_cols && !self.use_alternate {
            self.restore_tails_to_surface(old_cols, cols);
        }

        // Always restore cursor position after all resize operations.
        // restore_tails_to_surface and handle_rows_grow may leave cursor
        // at unexpected positions. Final restore ensures shell's SIGWINCH
        // response redraws at the correct location.
        if !self.use_alternate {
            use termwiz::surface::Position;
            let cursor_y = (old_cursor.1 + rows_restored).min(rows.saturating_sub(1));
            let cursor_x = old_cursor.0.min(cols.saturating_sub(1));
            self.primary_surface.add_change(Change::CursorPosition {
                x: Position::Absolute(cursor_x),
                y: Position::Absolute(cursor_y),
            });
        }

        // Reset scroll region on resize
        self.scroll_region = None;

        // Defer PTY resize notification to avoid SIGWINCH storms during drag.
        // Call flush_pty_resize() after resize events settle.
        self.pending_pty_resize = Some((cols, rows));
    }

    /// Throttle interval for PTY resize notifications.
    const PTY_RESIZE_THROTTLE: std::time::Duration = std::time::Duration::from_millis(100);

    /// Try to flush pending PTY resize. Returns true if flushed, false if throttled.
    /// When throttled, the pending resize is kept and the caller should retry later.
    pub fn flush_pty_resize(&mut self) -> bool {
        if self.pending_pty_resize.is_none() {
            return false;
        }

        if self.last_pty_flush.elapsed() < Self::PTY_RESIZE_THROTTLE {
            return false; // throttled — caller should retry later
        }

        if let Some((cols, rows)) = self.pending_pty_resize.take() {
            if let Err(e) = self.pty_master.resize(PtySize {
                rows: rows as u16,
                cols: cols as u16,
                pixel_width: 0,
                pixel_height: 0,
            }) {
                tracing::warn!("PTY resize failed: {e}");
            }
            self.last_pty_flush = std::time::Instant::now();
        }
        true
    }

    /// Force flush pending PTY resize regardless of throttle.
    /// Used for discrete events (pane close, split) where immediate notification is needed.
    pub fn force_flush_pty_resize(&mut self) {
        if let Some((cols, rows)) = self.pending_pty_resize.take() {
            if let Err(e) = self.pty_master.resize(PtySize {
                rows: rows as u16,
                cols: cols as u16,
                pixel_width: 0,
                pixel_height: 0,
            }) {
                tracing::warn!("PTY resize failed: {e}");
            }
            self.last_pty_flush = std::time::Instant::now();
        }
    }

    /// Check if there is a pending PTY resize.
    pub fn has_pending_pty_resize(&self) -> bool {
        self.pending_pty_resize.is_some()
    }

    /// When rows shrink, capture top lines to scrollback so the cursor stays
    /// near the bottom, mimicking xterm/Alacritty behavior.
    fn handle_rows_shrink(&mut self, new_rows: usize, old_rows: usize) {
        let (_, cursor_y) = self.primary_surface.cursor_position();
        let rows_to_remove = old_rows - new_rows;

        // Count blank lines below the cursor
        let lines = self.primary_surface.screen_lines();
        let mut blank_below = 0;
        for i in ((cursor_y + 1)..old_rows).rev() {
            if i < lines.len() && Self::is_line_blank(&lines[i]) {
                blank_below += 1;
            } else {
                break;
            }
        }

        // How many top lines need to be pushed to scrollback
        let lines_to_scroll = rows_to_remove.saturating_sub(blank_below);
        if lines_to_scroll > 0 {
            // Capture top lines to scrollback
            let captured = self.capture_top_lines(lines_to_scroll);
            let count = captured.len();
            for line in captured {
                self.scrollback.push_line(line);
            }
            // Shift saved_line_tails
            for _ in 0..count.min(self.saved_line_tails.len()) {
                self.saved_line_tails.remove(0);
            }

            // Scroll the surface up to remove the captured lines
            self.primary_surface.add_change(Change::ScrollRegionUp {
                first_row: 0,
                region_size: old_rows,
                scroll_count: count,
            });
        }
    }

    /// When rows grow, restore lines from scrollback to the top of the screen.
    /// Called AFTER primary_surface.resize() so the surface already has room.
    /// Returns the number of lines restored (for cursor offset calculation).
    fn handle_rows_grow(&mut self, new_rows: usize, old_rows: usize) -> usize {
        use termwiz::surface::Position;

        let rows_added = new_rows - old_rows;
        let restore_count = rows_added.min(self.scrollback.memory_len());

        if restore_count == 0 {
            return 0;
        }

        // Pop from scrollback (most recent first = back of deque)
        let mut to_restore: Vec<Vec<(String, CellAttributes)>> = Vec::new();
        for _ in 0..restore_count {
            if let Some(line) = self.scrollback.pop_back() {
                to_restore.push(line);
            }
        }
        let actual_restored = to_restore.len();
        to_restore.reverse(); // oldest first

        // Surface is already resized to new_rows.
        // Scroll current content down to make room at top.
        self.primary_surface.add_change(Change::ScrollRegionDown {
            first_row: 0,
            region_size: new_rows,
            scroll_count: actual_restored,
        });

        // Write restored lines at the top
        for (row, line_cells) in to_restore.iter().enumerate() {
            self.primary_surface.add_change(Change::CursorPosition {
                x: Position::Absolute(0),
                y: Position::Absolute(row),
            });
            for (text, attrs) in line_cells {
                self.primary_surface
                    .add_change(Change::AllAttributes(attrs.clone()));
                self.primary_surface.add_change(Change::Text(text.clone()));
            }
        }

        // Shift saved_line_tails to match shifted content positions
        if !self.saved_line_tails.is_empty() {
            let mut shifted = vec![Vec::new(); actual_restored];
            shifted.append(&mut self.saved_line_tails);
            self.saved_line_tails = shifted;
        }

        actual_restored
    }

    /// Check if a line is visually blank (all spaces or empty).
    fn is_line_blank(line: &termwiz::surface::line::Line) -> bool {
        for cell in line.visible_cells() {
            let s = cell.str();
            if !s.is_empty() && s.trim() != "" {
                return false;
            }
        }
        true
    }

    /// Before termwiz truncates lines, capture cells that would be lost (cols shrinking)
    /// or merge saved tails back when cols grow.
    fn save_or_restore_line_tails(&mut self, old_cols: usize, new_cols: usize, new_rows: usize) {
        let lines = self.primary_surface.screen_lines();
        let line_count = lines.len();

        // Ensure saved_line_tails has enough entries
        if self.saved_line_tails.len() < line_count {
            self.saved_line_tails.resize(line_count, Vec::new());
        }

        if new_cols < old_cols {
            // Cols shrinking: capture cells at indices [new_cols..] before termwiz truncates them
            for (i, line) in lines.iter().enumerate() {
                let mut tail_cells: Vec<(String, CellAttributes)> = line
                    .visible_cells()
                    .filter(|cell| cell.cell_index() >= new_cols)
                    .map(|cell| (cell.str().to_string(), cell.attrs().clone()))
                    .collect();
                // Prepend to any previously saved tail for this line
                if !self.saved_line_tails[i].is_empty() {
                    tail_cells.extend(self.saved_line_tails[i].drain(..));
                }
                self.saved_line_tails[i] = tail_cells;
            }
        } else if new_cols > old_cols {
            // Cols growing: trim saved tails — cells will be restored after resize
            // (nothing to do here; restore_tails_to_surface handles it)
        }

        // Trim saved_line_tails to match new row count
        self.saved_line_tails.truncate(new_rows);
    }

    /// After termwiz Surface::resize expanded cols, write back saved tail cells.
    fn restore_tails_to_surface(&mut self, old_cols: usize, new_cols: usize) {
        use termwiz::surface::Position;

        let restore_count = new_cols - old_cols;

        for (row, tail) in self.saved_line_tails.iter_mut().enumerate() {
            if tail.is_empty() {
                continue;
            }
            let cells_to_restore = restore_count.min(tail.len());
            let restored: Vec<(String, CellAttributes)> = tail.drain(..cells_to_restore).collect();

            // Position cursor at (old_cols, row) and write each cell
            self.primary_surface.add_change(Change::CursorPosition {
                x: Position::Absolute(old_cols),
                y: Position::Absolute(row),
            });
            for (text, attrs) in restored {
                self.primary_surface
                    .add_change(Change::AllAttributes(attrs));
                self.primary_surface.add_change(Change::Text(text));
            }
        }

        // Restore cursor to where it was (bottom of screen, col 0 as safe default)
        // The actual cursor position will be corrected by the next PTY output
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

    /// Get the foreground process info (name, PID) for this terminal.
    pub fn foreground_process_info(&self) -> Option<foreground_process::ForegroundProcessInfo> {
        let shell_pid = self.child.process_id()?;
        foreground_process::get_foreground_process(shell_pid)
    }

    /// Whether the terminal is currently running a foreground program other
    /// than the shell itself (e.g. `vim`, `cargo build`). Returns false when
    /// the shell is at its prompt or when foreground info cannot be resolved.
    pub fn is_busy(&self) -> bool {
        let Some(shell_pid) = self.child.process_id() else {
            return false;
        };
        let Some(info) = foreground_process::get_foreground_process(shell_pid) else {
            return false;
        };
        if info.pid == shell_pid {
            return false;
        }
        !foreground_process::is_known_shell_name(&info.name)
    }

    /// Get the current working directory of the child process.
    /// Returns the CWD cached from OSC 7 sequences. If no OSC 7 has been
    /// received yet, falls back to an OS-level query (proc_pidinfo on macOS,
    /// /proc on Linux). The fallback result is NOT cached to avoid stale data
    /// — OSC 7 remains the authoritative source when available.
    pub fn get_cwd(&self) -> Option<std::path::PathBuf> {
        if let Some(ref cwd) = self.cached_cwd {
            return Some(cwd.clone());
        }
        // On-demand fallback: query OS for CWD (microseconds on macOS/Linux, None on Windows)
        if let Some(pid) = self.process_id() {
            return cwd::get_cwd_of_pid(pid);
        }
        None
    }

    /// Set the cached CWD. Used by the OS-level CWD polling mechanism.
    pub fn set_cached_cwd(&mut self, cwd: std::path::PathBuf) {
        self.cached_cwd = Some(cwd);
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
        self.output.set_mark();
    }

    /// Return raw bytes accumulated since the last `set_output_scan_mark()` call.
    /// Used by the ClaudeError scanner; independent of `read_since_mark`'s mark.
    pub fn output_since_scan_mark(&self, strip_ansi: bool) -> String {
        self.output.output_since_scan_mark(strip_ansi)
    }

    /// Advance the scan mark to the current end of the output buffer.
    pub fn set_output_scan_mark(&mut self) {
        self.output.set_scan_mark();
    }

    /// Read output since the last mark. If no mark was set, reads from the beginning.
    pub fn read_since_mark(&self, strip_ansi: bool) -> String {
        self.output.read_since_mark(strip_ansi)
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

    /// Scan the active surface for an isolated reverse-video cell.
    ///
    /// Some TUIs (notably Ink-based ones like Claude Code) hide the real terminal
    /// cursor with `\e[?25l` and draw their own "fake cursor" by emitting a single
    /// cell with the reverse-video attribute (`\e[7m`). This scan detects that cell
    /// so we can use its position as the IME preedit anchor.
    ///
    /// Returns the cell position only when a **single** reverse-video cell exists.
    /// Multi-cell reverse regions (selection highlight, inverse-painted UI) are
    /// ambiguous and return None.
    pub fn find_fake_cursor_cell(&self) -> Option<(usize, usize)> {
        let surface = self.surface();
        let mut found: Option<(usize, usize)> = None;
        for (row_idx, line) in surface.screen_lines().iter().enumerate() {
            for cell_ref in line.visible_cells() {
                if cell_ref.attrs().reverse() {
                    if found.is_some() {
                        return None; // two or more — ambiguous
                    }
                    found = Some((cell_ref.cell_index(), row_idx));
                }
            }
        }
        found
    }

    // ---- Scrollback buffer methods (delegated to Scrollback) ----

    /// Current scroll offset (0 = at bottom/live, >0 = scrolled up).
    pub fn scroll_offset(&self) -> usize {
        self.scrollback.scroll_offset
    }

    /// Set the scrollback buffer limit.
    pub fn set_scrollback_limit(&mut self, limit: usize) {
        self.scrollback.set_limit(limit);
    }

    /// Enable disk-backed scrollback swap for this terminal.
    pub fn enable_disk_scrollback(&mut self, surface_id: u32) {
        self.scrollback.enable_disk(surface_id);
    }

    /// Scroll up (towards older content).
    pub fn scroll_up(&mut self, lines: usize) {
        self.scrollback.scroll_up(lines);
    }

    /// Scroll down (towards newer/live content).
    pub fn scroll_down(&mut self, lines: usize) {
        self.scrollback.scroll_down(lines);
    }

    /// Reset scroll position to the bottom (live view).
    pub fn scroll_to_bottom(&mut self) {
        self.scrollback.scroll_to_bottom();
    }

    /// Number of lines in the scrollback buffer (memory + disk).
    pub fn scrollback_len(&self) -> usize {
        self.scrollback.total_len()
    }

    /// Get a specific scrollback line by index (0 = oldest, memory only).
    /// For disk-backed lines, use scrollback_line_owned().
    pub fn scrollback_line(&self, index: usize) -> Option<&Vec<(String, CellAttributes)>> {
        self.scrollback.line(index)
    }

    /// Get a scrollback line by index, returning owned data.
    /// Works for both memory and disk-backed lines.
    pub fn scrollback_line_owned(&self, index: usize) -> Option<Vec<(String, CellAttributes)>> {
        self.scrollback.line_owned(index)
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
            Change::ScrollRegionUp {
                first_row,
                scroll_count,
                ..
            } if *first_row == 0 => {
                let captured = self.capture_top_lines(*scroll_count);
                let count = captured.len();
                for line in captured {
                    self.scrollback.push_line(line);
                }
                // Shift saved_line_tails: remove top entries that scrolled off
                for _ in 0..count.min(self.saved_line_tails.len()) {
                    self.saved_line_tails.remove(0);
                }
                // Compensate scroll_offset so the user's viewport stays in place
                if self.scrollback.scroll_offset > 0 {
                    self.scrollback.scroll_offset += count;
                }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use termwiz::escape::csi::CSI;
    use termwiz::escape::parser::Parser;

    fn noop_waker() -> Waker {
        Arc::new(|| {})
    }

    fn test_terminal(cols: usize, rows: usize) -> Terminal {
        let waker = noop_waker();
        Terminal::new(
            TerminalConfig {
                cols,
                rows,
                shell: None,
                args: &[],
                surface_id: 0,
                working_dir: None,
            },
            waker,
        )
        .expect("terminal creation")
    }

    // ---- DECSET/DECRST mode toggling tests ----

    #[test]
    fn decset_application_cursor_keys() {
        let mut terminal = test_terminal(80, 24);
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
        let mut terminal = test_terminal(80, 24);
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
        let mut terminal = test_terminal(80, 24);
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
        let mut terminal = test_terminal(80, 24);
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
        let mut terminal = test_terminal(80, 24);
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
        let mut terminal = test_terminal(80, 24);

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
        let mut terminal = test_terminal(80, 24);

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
        let mut terminal = test_terminal(80, 24);

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
        let mut terminal = test_terminal(80, 24);

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

    // ---- Scrollback capture: implicit scroll via line wrap ----

    fn first_scrollback_text(terminal: &Terminal, index: usize) -> String {
        terminal
            .scrollback_line(index)
            .map(|l| {
                l.iter()
                    .map(|(s, _)| s.clone())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .unwrap_or_default()
    }

    #[test]
    fn scrollback_captured_on_lf_at_bottom_row() {
        let mut terminal = test_terminal(10, 4);
        terminal.process_bytes(b"row0\r\nrow1\r\nrow2\r\nrow3");
        assert_eq!(terminal.scrollback_len(), 0);
        terminal.process_bytes(b"\r\nrow4");
        assert_eq!(
            terminal.scrollback_len(),
            1,
            "newline at bottom row must push row0 to scrollback"
        );
        assert_eq!(first_scrollback_text(&terminal, 0), "row0");
    }

}
