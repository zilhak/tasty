use std::io::{Read, Write};
use std::sync::{LazyLock, mpsc};
use std::thread;

use anyhow::Result;
use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};
use termwiz::cell::{AttributeChange, CellAttributes};
use termwiz::color::ColorAttribute;
use termwiz::escape::csi::{Cursor, Edit, EraseInDisplay, EraseInLine, Sgr, CSI};
use termwiz::escape::esc::{Esc, EscCode};
use termwiz::escape::parser::Parser;
use termwiz::escape::{Action, ControlCode, OperatingSystemCommand};
use termwiz::surface::{Change, CursorVisibility, Position, Surface};

/// Events emitted by the terminal during processing.
pub struct TerminalEvent {
    /// The surface ID that generated this event (0 if not yet assigned).
    pub surface_id: u32,
    pub kind: TerminalEventKind,
}

/// Types of events a terminal can emit.
pub enum TerminalEventKind {
    /// A notification from OSC 9 / OSC 99 / OSC 777.
    Notification { title: String, body: String },
    /// Bell character received.
    BellRing,
    /// Window title changed via OSC 0 / OSC 2.
    TitleChanged(String),
    /// Current working directory changed via OSC 7.
    CwdChanged(String),
    /// The child process has exited.
    ProcessExited,
}

/// Maximum size of the output buffer (1 MB).
const OUTPUT_BUFFER_MAX: usize = 1_048_576;

pub struct Terminal {
    surface: Surface,
    parser: Parser,
    pty_writer: Box<dyn Write + Send>,
    pty_master: Box<dyn portable_pty::MasterPty + Send>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    action_rx: mpsc::Receiver<Vec<u8>>,
    _reader_thread: thread::JoinHandle<()>,
    cols: usize,
    rows: usize,
    /// Saved cursor position for ESC 7 / ESC 8
    saved_cursor: Option<(usize, usize)>,
    /// Events accumulated during process(), consumed via take_events().
    events: Vec<TerminalEvent>,
    /// Raw PTY output history for read-mark API.
    output_buffer: Vec<u8>,
    /// Byte offset of the read mark in the output buffer.
    read_mark: Option<usize>,
    /// Whether we've already emitted a ProcessExited event.
    process_exit_emitted: bool,
}

impl Terminal {
    pub fn new(cols: usize, rows: usize) -> Result<Self> {
        Self::new_with_shell(cols, rows, None)
    }

    /// Create a terminal with an optional custom shell. If `shell` is `None` or empty,
    /// the platform default shell is used.
    pub fn new_with_shell(cols: usize, rows: usize, shell: Option<&str>) -> Result<Self> {
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
                    }
                    Err(_) => break,
                }
            }
        });

        let surface = Surface::new(cols, rows);
        let parser = Parser::new();

        Ok(Self {
            surface,
            parser,
            pty_writer,
            pty_master: pair.master,
            child,
            action_rx: rx,
            _reader_thread: reader_thread,
            cols,
            rows,
            saved_cursor: None,
            events: Vec::new(),
            output_buffer: Vec::new(),
            read_mark: None,
            process_exit_emitted: false,
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
                let changes = self.action_to_changes(action);
                if !changes.is_empty() {
                    for change in changes {
                        self.surface.add_change(change);
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

    /// Convert a parsed VT action into Surface changes.
    fn action_to_changes(&mut self, action: Action) -> Vec<Change> {
        match action {
            Action::Print(c) => vec![Change::Text(c.to_string())],
            Action::PrintString(s) => vec![Change::Text(s)],
            Action::Control(code) => self.map_control(code),
            Action::CSI(csi) => self.map_csi(csi),
            Action::Esc(esc) => self.map_esc(esc),
            Action::OperatingSystemCommand(osc) => {
                self.map_osc(*osc);
                vec![]
            }
            _ => vec![],
        }
    }

    fn map_control(&mut self, code: ControlCode) -> Vec<Change> {
        match code {
            ControlCode::LineFeed | ControlCode::VerticalTab | ControlCode::FormFeed => {
                vec![Change::Text("\n".into())]
            }
            ControlCode::CarriageReturn => vec![Change::Text("\r".into())],
            ControlCode::Backspace => vec![Change::CursorPosition {
                x: Position::Relative(-1),
                y: Position::Relative(0),
            }],
            ControlCode::HorizontalTab => vec![Change::Text("\t".into())],
            ControlCode::Bell => {
                self.events.push(TerminalEvent {
                    surface_id: 0,
                    kind: TerminalEventKind::BellRing,
                });
                vec![]
            }
            _ => vec![],
        }
    }

    fn map_csi(&mut self, csi: CSI) -> Vec<Change> {
        match csi {
            CSI::Sgr(sgr) => self.map_sgr(sgr),
            CSI::Cursor(cursor) => self.map_cursor(cursor),
            CSI::Edit(edit) => self.map_edit(edit),
            CSI::Mode(_mode) => {
                // TODO: handle DECSET/DECRST for alternate screen, cursor visibility etc.
                vec![]
            }
            CSI::Device(_) => vec![],
            CSI::Mouse(_) => vec![],
            CSI::Window(_) => vec![],
            CSI::Keyboard(_) => vec![],
            _ => vec![],
        }
    }

    fn map_sgr(&self, sgr: Sgr) -> Vec<Change> {
        match sgr {
            Sgr::Reset => vec![Change::AllAttributes(CellAttributes::default())],
            Sgr::Intensity(intensity) => {
                vec![Change::Attribute(AttributeChange::Intensity(intensity))]
            }
            Sgr::Underline(underline) => {
                vec![Change::Attribute(AttributeChange::Underline(underline))]
            }
            Sgr::Italic(on) => vec![Change::Attribute(AttributeChange::Italic(on))],
            Sgr::Blink(blink) => vec![Change::Attribute(AttributeChange::Blink(blink))],
            Sgr::Inverse(on) => vec![Change::Attribute(AttributeChange::Reverse(on))],
            Sgr::Invisible(on) => vec![Change::Attribute(AttributeChange::Invisible(on))],
            Sgr::StrikeThrough(on) => {
                vec![Change::Attribute(AttributeChange::StrikeThrough(on))]
            }
            Sgr::Foreground(color_spec) => {
                vec![Change::Attribute(AttributeChange::Foreground(
                    color_spec.into(),
                ))]
            }
            Sgr::Background(color_spec) => {
                vec![Change::Attribute(AttributeChange::Background(
                    color_spec.into(),
                ))]
            }
            Sgr::Font(_) | Sgr::Overline(_) | Sgr::UnderlineColor(_) | Sgr::VerticalAlign(_) => {
                // Not commonly needed for basic terminal emulation
                vec![]
            }
        }
    }

    fn map_cursor(&mut self, cursor: Cursor) -> Vec<Change> {
        match cursor {
            Cursor::Up(n) => vec![Change::CursorPosition {
                x: Position::Relative(0),
                y: Position::Relative(-(n as isize)),
            }],
            Cursor::Down(n) => vec![Change::CursorPosition {
                x: Position::Relative(0),
                y: Position::Relative(n as isize),
            }],
            Cursor::Left(n) => vec![Change::CursorPosition {
                x: Position::Relative(-(n as isize)),
                y: Position::Relative(0),
            }],
            Cursor::Right(n) => vec![Change::CursorPosition {
                x: Position::Relative(n as isize),
                y: Position::Relative(0),
            }],
            Cursor::Position { line, col } => vec![Change::CursorPosition {
                x: Position::Absolute(col.as_zero_based() as usize),
                y: Position::Absolute(line.as_zero_based() as usize),
            }],
            Cursor::CharacterAbsolute(col) | Cursor::CharacterPositionAbsolute(col) => {
                vec![Change::CursorPosition {
                    x: Position::Absolute(col.as_zero_based() as usize),
                    y: Position::Relative(0),
                }]
            }
            Cursor::LinePositionAbsolute(line) => vec![Change::CursorPosition {
                x: Position::Relative(0),
                y: Position::Absolute(line.saturating_sub(1) as usize),
            }],
            Cursor::CharacterPositionBackward(n) => vec![Change::CursorPosition {
                x: Position::Relative(-(n as isize)),
                y: Position::Relative(0),
            }],
            Cursor::CharacterPositionForward(n) => vec![Change::CursorPosition {
                x: Position::Relative(n as isize),
                y: Position::Relative(0),
            }],
            Cursor::CharacterAndLinePosition { line, col } => vec![Change::CursorPosition {
                x: Position::Absolute(col.as_zero_based() as usize),
                y: Position::Absolute(line.as_zero_based() as usize),
            }],
            Cursor::LinePositionBackward(n) => vec![Change::CursorPosition {
                x: Position::Relative(0),
                y: Position::Relative(-(n as isize)),
            }],
            Cursor::LinePositionForward(n) => vec![Change::CursorPosition {
                x: Position::Relative(0),
                y: Position::Relative(n as isize),
            }],
            Cursor::NextLine(n) => vec![Change::CursorPosition {
                x: Position::Absolute(0),
                y: Position::Relative(n as isize),
            }],
            Cursor::PrecedingLine(n) => vec![Change::CursorPosition {
                x: Position::Absolute(0),
                y: Position::Relative(-(n as isize)),
            }],
            Cursor::ForwardTabulation(n) => {
                // Move forward n tab stops
                vec![Change::Text("\t".repeat(n as usize))]
            }
            Cursor::SaveCursor => {
                let pos = self.surface.cursor_position();
                self.saved_cursor = Some((pos.0, pos.1));
                vec![]
            }
            Cursor::RestoreCursor => {
                if let Some((x, y)) = self.saved_cursor {
                    vec![Change::CursorPosition {
                        x: Position::Absolute(x),
                        y: Position::Absolute(y),
                    }]
                } else {
                    vec![]
                }
            }
            Cursor::SetTopAndBottomMargins { .. } => {
                // TODO: scroll region support
                vec![]
            }
            Cursor::CursorStyle(_style) => {
                // TODO: map to CursorShape change
                vec![]
            }
            _ => vec![],
        }
    }

    fn map_edit(&self, edit: Edit) -> Vec<Change> {
        match edit {
            Edit::EraseInDisplay(mode) => match mode {
                EraseInDisplay::EraseToEndOfDisplay => {
                    vec![Change::ClearToEndOfScreen(ColorAttribute::Default)]
                }
                EraseInDisplay::EraseToStartOfDisplay => {
                    // Clear from beginning to cursor - approximate with clear screen
                    // TODO: proper implementation
                    vec![]
                }
                EraseInDisplay::EraseDisplay => {
                    vec![Change::ClearScreen(ColorAttribute::Default)]
                }
                EraseInDisplay::EraseScrollback => {
                    // TODO: clear scrollback buffer
                    vec![]
                }
            },
            Edit::EraseInLine(mode) => match mode {
                EraseInLine::EraseToEndOfLine => {
                    vec![Change::ClearToEndOfLine(ColorAttribute::Default)]
                }
                EraseInLine::EraseToStartOfLine => {
                    // TODO: proper implementation
                    vec![]
                }
                EraseInLine::EraseLine => {
                    // Move to col 0, then clear to end of line
                    vec![
                        Change::CursorPosition {
                            x: Position::Absolute(0),
                            y: Position::Relative(0),
                        },
                        Change::ClearToEndOfLine(ColorAttribute::Default),
                    ]
                }
            },
            Edit::ScrollUp(n) => {
                let (_cols, rows) = self.surface.dimensions();
                vec![Change::ScrollRegionUp {
                    first_row: 0,
                    region_size: rows,
                    scroll_count: n as usize,
                }]
            }
            Edit::ScrollDown(n) => {
                let (_cols, rows) = self.surface.dimensions();
                vec![Change::ScrollRegionDown {
                    first_row: 0,
                    region_size: rows,
                    scroll_count: n as usize,
                }]
            }
            Edit::DeleteCharacter(_)
            | Edit::DeleteLine(_)
            | Edit::InsertCharacter(_)
            | Edit::InsertLine(_)
            | Edit::EraseCharacter(_)
            | Edit::Repeat(_) => {
                // TODO: these require direct cell manipulation not easily done via Change
                vec![]
            }
        }
    }

    fn map_esc(&mut self, esc: Esc) -> Vec<Change> {
        match esc {
            Esc::Code(EscCode::DecSaveCursorPosition) => {
                let pos = self.surface.cursor_position();
                self.saved_cursor = Some((pos.0, pos.1));
                vec![]
            }
            Esc::Code(EscCode::DecRestoreCursorPosition) => {
                if let Some((x, y)) = self.saved_cursor {
                    vec![Change::CursorPosition {
                        x: Position::Absolute(x),
                        y: Position::Absolute(y),
                    }]
                } else {
                    vec![]
                }
            }
            Esc::Code(EscCode::ReverseIndex) => {
                // Move cursor up one line, scrolling if at top
                vec![Change::CursorPosition {
                    x: Position::Relative(0),
                    y: Position::Relative(-1),
                }]
            }
            Esc::Code(EscCode::FullReset) => {
                self.saved_cursor = None;
                vec![
                    Change::AllAttributes(CellAttributes::default()),
                    Change::ClearScreen(ColorAttribute::Default),
                    Change::CursorVisibility(CursorVisibility::Visible),
                ]
            }
            _ => vec![],
        }
    }

    fn map_osc(&mut self, osc: OperatingSystemCommand) {
        match osc {
            // OSC 0: Set icon name and window title
            OperatingSystemCommand::SetIconNameAndWindowTitle(title) => {
                self.events.push(TerminalEvent {
                    surface_id: 0,
                    kind: TerminalEventKind::TitleChanged(title),
                });
            }
            // OSC 2: Set window title
            OperatingSystemCommand::SetWindowTitle(title)
            | OperatingSystemCommand::SetWindowTitleSun(title) => {
                self.events.push(TerminalEvent {
                    surface_id: 0,
                    kind: TerminalEventKind::TitleChanged(title),
                });
            }
            // OSC 7: Current working directory
            OperatingSystemCommand::CurrentWorkingDirectory(url) => {
                // url format: file://hostname/path
                let path = if let Some(stripped) = url.strip_prefix("file://") {
                    // Skip hostname part (up to next /)
                    if let Some(slash_pos) = stripped.find('/') {
                        stripped[slash_pos..].to_string()
                    } else {
                        stripped.to_string()
                    }
                } else {
                    url.clone()
                };
                self.events.push(TerminalEvent {
                    surface_id: 0,
                    kind: TerminalEventKind::CwdChanged(path),
                });
            }
            // OSC 9: iTerm2/ConEmu notification
            OperatingSystemCommand::SystemNotification(body) => {
                self.events.push(TerminalEvent {
                    surface_id: 0,
                    kind: TerminalEventKind::Notification {
                        title: "Terminal".to_string(),
                        body,
                    },
                });
            }
            // OSC 777: rxvt-unicode extension (notify;title;body)
            OperatingSystemCommand::RxvtExtension(parts) => {
                if parts.first().map(|s| s.as_str()) == Some("notify") {
                    let title = parts.get(1).cloned().unwrap_or_default();
                    let body = parts.get(2).cloned().unwrap_or_default();
                    self.events.push(TerminalEvent {
                        surface_id: 0,
                        kind: TerminalEventKind::Notification { title, body },
                    });
                }
            }
            // OSC 99: Kitty notification (arrives as Unspecified since termwiz doesn't parse it)
            OperatingSystemCommand::Unspecified(params) => {
                // Check if first param starts with "99"
                if let Some(first) = params.first() {
                    if first == b"99" {
                        // Parse key=value pairs from remaining params
                        let mut title = String::new();
                        let mut body = String::new();
                        for param in params.iter().skip(1) {
                            let s = String::from_utf8_lossy(param);
                            if let Some(val) = s.strip_prefix("t=") {
                                title = val.to_string();
                            } else if let Some(val) = s.strip_prefix("d=0;") {
                                body = val.to_string();
                            } else if let Some(val) = s.strip_prefix("d=1;") {
                                body = val.to_string();
                            } else if !s.contains('=') {
                                // Plain body text
                                body = s.to_string();
                            }
                        }
                        if title.is_empty() {
                            title = "Terminal".to_string();
                        }
                        self.events.push(TerminalEvent {
                            surface_id: 0,
                            kind: TerminalEventKind::Notification { title, body },
                        });
                    }
                }
            }
            _ => {}
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
        self.surface.resize(cols, rows);

        // Propagate resize to the PTY so the child process knows the new size
        let _ = self.pty_master.resize(PtySize {
            rows: rows as u16,
            cols: cols as u16,
            pixel_width: 0,
            pixel_height: 0,
        });
    }

    pub fn surface(&self) -> &Surface {
        &self.surface
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
