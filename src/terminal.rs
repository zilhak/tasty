use std::io::{Read, Write};
use std::sync::{Arc, LazyLock, mpsc};
use std::thread;

use anyhow::Result;
use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};
use termwiz::cell::{AttributeChange, CellAttributes};
use termwiz::color::ColorAttribute;
use termwiz::escape::csi::{
    Cursor, DecPrivateMode, DecPrivateModeCode, Edit, EraseInDisplay, EraseInLine,
    Mode as CsiMode, Sgr, CSI,
};
use termwiz::escape::esc::{Esc, EscCode};
use termwiz::escape::parser::Parser;
use termwiz::escape::{Action, ControlCode, OperatingSystemCommand};
use termwiz::surface::{Change, CursorVisibility, Position, Surface};

/// Callback to wake the event loop when PTY data arrives.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

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

/// Mouse tracking modes (DECSET 1000/1002/1003).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseTrackingMode {
    None,
    Click,      // 1000
    CellMotion, // 1002
    AllMotion,  // 1003
}

pub struct Terminal {
    /// Primary screen buffer.
    primary_surface: Surface,
    /// Alternate screen buffer (lazily created on DECSET 1049/47).
    alternate_surface: Option<Surface>,
    /// Whether the alternate screen is active.
    use_alternate: bool,
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
    /// Saved cursor position specifically for alternate screen enter/exit.
    alt_saved_cursor: Option<(usize, usize)>,
    /// Events accumulated during process(), consumed via take_events().
    events: Vec<TerminalEvent>,
    /// Raw PTY output history for read-mark API.
    output_buffer: Vec<u8>,
    /// Byte offset of the read mark in the output buffer.
    read_mark: Option<usize>,
    /// Whether we've already emitted a ProcessExited event.
    process_exit_emitted: bool,
    /// DECCKM: application cursor keys mode.
    application_cursor_keys: bool,
    /// DECTCEM: cursor visibility.
    cursor_visible: bool,
    /// Bracketed paste mode (mode 2004).
    bracketed_paste: bool,
    /// Mouse tracking mode.
    mouse_tracking: MouseTrackingMode,
    /// SGR mouse encoding (mode 1006).
    sgr_mouse: bool,
    /// Focus event tracking (mode 1004).
    focus_tracking: bool,
    /// Scroll region top/bottom (1-based inclusive, None = full screen).
    scroll_region: Option<(usize, usize)>,
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
                // Handled in process() via handle_mode() before reaching here.
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
                let pos = self.surface().cursor_position();
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
            Cursor::SetTopAndBottomMargins { top, bottom } => {
                let top_val = top.as_zero_based() as usize;
                let bottom_val = bottom.as_zero_based() as usize;
                if top_val == 0 && bottom_val >= self.rows.saturating_sub(1) {
                    // Full screen -- clear scroll region
                    self.scroll_region = None;
                } else {
                    self.scroll_region = Some((top_val, bottom_val));
                }
                // DECSTBM also resets cursor to home
                vec![Change::CursorPosition {
                    x: Position::Absolute(0),
                    y: Position::Absolute(0),
                }]
            }
            Cursor::CursorStyle(_style) => {
                // TODO: map to CursorShape change
                vec![]
            }
            _ => vec![],
        }
    }

    fn map_edit(&mut self, edit: Edit) -> Vec<Change> {
        match edit {
            Edit::EraseInDisplay(mode) => match mode {
                EraseInDisplay::EraseToEndOfDisplay => {
                    vec![Change::ClearToEndOfScreen(ColorAttribute::Default)]
                }
                EraseInDisplay::EraseToStartOfDisplay => {
                    // Erase from top-left to cursor (inclusive).
                    // We approximate by clearing each line from 0 to current row,
                    // then clearing from col 0 to current col on the current line.
                    let (cx, cy) = self.surface().cursor_position();
                    let (cols, _rows) = self.surface().dimensions();
                    let mut changes = Vec::new();
                    // Clear full lines above cursor
                    for row in 0..cy {
                        changes.push(Change::CursorPosition {
                            x: Position::Absolute(0),
                            y: Position::Absolute(row),
                        });
                        changes.push(Change::ClearToEndOfLine(ColorAttribute::Default));
                    }
                    // Clear current line from start to cursor (inclusive)
                    changes.push(Change::CursorPosition {
                        x: Position::Absolute(0),
                        y: Position::Absolute(cy),
                    });
                    // Write spaces up to and including cursor column
                    if cx < cols {
                        changes.push(Change::Text(" ".repeat(cx + 1)));
                    }
                    // Restore cursor position
                    changes.push(Change::CursorPosition {
                        x: Position::Absolute(cx),
                        y: Position::Absolute(cy),
                    });
                    changes
                }
                EraseInDisplay::EraseDisplay => {
                    vec![Change::ClearScreen(ColorAttribute::Default)]
                }
                EraseInDisplay::EraseScrollback => {
                    // No scrollback buffer in termwiz Surface, ignore.
                    vec![]
                }
            },
            Edit::EraseInLine(mode) => match mode {
                EraseInLine::EraseToEndOfLine => {
                    vec![Change::ClearToEndOfLine(ColorAttribute::Default)]
                }
                EraseInLine::EraseToStartOfLine => {
                    let (cx, cy) = self.surface().cursor_position();
                    let mut changes = Vec::new();
                    changes.push(Change::CursorPosition {
                        x: Position::Absolute(0),
                        y: Position::Absolute(cy),
                    });
                    if cx > 0 {
                        changes.push(Change::Text(" ".repeat(cx + 1)));
                    }
                    changes.push(Change::CursorPosition {
                        x: Position::Absolute(cx),
                        y: Position::Absolute(cy),
                    });
                    changes
                }
                EraseInLine::EraseLine => {
                    let (_cx, cy) = self.surface().cursor_position();
                    vec![
                        Change::CursorPosition {
                            x: Position::Absolute(0),
                            y: Position::Absolute(cy),
                        },
                        Change::ClearToEndOfLine(ColorAttribute::Default),
                    ]
                }
            },
            Edit::ScrollUp(n) => {
                let (first_row, region_size) = self.scroll_region_params();
                vec![Change::ScrollRegionUp {
                    first_row,
                    region_size,
                    scroll_count: n as usize,
                }]
            }
            Edit::ScrollDown(n) => {
                let (first_row, region_size) = self.scroll_region_params();
                vec![Change::ScrollRegionDown {
                    first_row,
                    region_size,
                    scroll_count: n as usize,
                }]
            }
            Edit::DeleteCharacter(n) => {
                // DCH: delete N characters at cursor, shifting the rest left.
                // Approximate by overwriting with spaces from cursor to end of line,
                // then restoring the shifted content.
                let (cx, cy) = self.surface().cursor_position();
                let (cols, _rows) = self.surface().dimensions();
                let remaining = cols.saturating_sub(cx);
                let n = (n as usize).min(remaining);
                if n == 0 {
                    return vec![];
                }
                // Read current line content from the surface
                let line = self.read_line_from_surface(cy, cx, cols);
                let mut new_content: Vec<char> = line.chars().collect();
                // Remove n characters at the beginning (relative to cx)
                let remove_count = n.min(new_content.len());
                new_content.drain(..remove_count);
                // Pad with spaces to fill the line
                while new_content.len() < remaining {
                    new_content.push(' ');
                }
                let text: String = new_content.into_iter().collect();
                vec![
                    Change::CursorPosition {
                        x: Position::Absolute(cx),
                        y: Position::Absolute(cy),
                    },
                    Change::Text(text),
                    Change::CursorPosition {
                        x: Position::Absolute(cx),
                        y: Position::Absolute(cy),
                    },
                ]
            }
            Edit::InsertCharacter(n) => {
                // ICH: insert N blank characters at cursor, shifting content right.
                let (cx, cy) = self.surface().cursor_position();
                let (cols, _rows) = self.surface().dimensions();
                let remaining = cols.saturating_sub(cx);
                let n = (n as usize).min(remaining);
                if n == 0 {
                    return vec![];
                }
                let line = self.read_line_from_surface(cy, cx, cols);
                let mut new_content: String = " ".repeat(n);
                // Take only what fits
                let take = remaining.saturating_sub(n);
                new_content.extend(line.chars().take(take));
                // Pad if needed
                while new_content.len() < remaining {
                    new_content.push(' ');
                }
                new_content.truncate(remaining);
                vec![
                    Change::CursorPosition {
                        x: Position::Absolute(cx),
                        y: Position::Absolute(cy),
                    },
                    Change::Text(new_content),
                    Change::CursorPosition {
                        x: Position::Absolute(cx),
                        y: Position::Absolute(cy),
                    },
                ]
            }
            Edit::DeleteLine(n) => {
                // DL: delete N lines at cursor row, scrolling content up within scroll region.
                let (_cx, cy) = self.surface().cursor_position();
                let (first_row, region_size) = self.scroll_region_params();
                // Only scroll if cursor is within the scroll region
                let effective_first = cy.max(first_row);
                let effective_size = (first_row + region_size).saturating_sub(effective_first);
                if effective_size == 0 {
                    return vec![];
                }
                vec![
                    Change::ScrollRegionUp {
                        first_row: effective_first,
                        region_size: effective_size,
                        scroll_count: n as usize,
                    },
                    Change::CursorPosition {
                        x: Position::Absolute(0),
                        y: Position::Absolute(cy),
                    },
                ]
            }
            Edit::InsertLine(n) => {
                // IL: insert N blank lines at cursor row, scrolling content down.
                let (_cx, cy) = self.surface().cursor_position();
                let (first_row, region_size) = self.scroll_region_params();
                let effective_first = cy.max(first_row);
                let effective_size = (first_row + region_size).saturating_sub(effective_first);
                if effective_size == 0 {
                    return vec![];
                }
                vec![
                    Change::ScrollRegionDown {
                        first_row: effective_first,
                        region_size: effective_size,
                        scroll_count: n as usize,
                    },
                    Change::CursorPosition {
                        x: Position::Absolute(0),
                        y: Position::Absolute(cy),
                    },
                ]
            }
            Edit::EraseCharacter(n) => {
                // ECH: erase N characters starting at cursor (replace with spaces).
                let (cx, cy) = self.surface().cursor_position();
                let (cols, _rows) = self.surface().dimensions();
                let n = (n as usize).min(cols.saturating_sub(cx));
                if n == 0 {
                    return vec![];
                }
                vec![
                    Change::CursorPosition {
                        x: Position::Absolute(cx),
                        y: Position::Absolute(cy),
                    },
                    Change::Text(" ".repeat(n)),
                    Change::CursorPosition {
                        x: Position::Absolute(cx),
                        y: Position::Absolute(cy),
                    },
                ]
            }
            Edit::Repeat(n) => {
                // REP: repeat the last printed character N times.
                // We don't track the last char easily; ignore for now.
                let _ = n;
                vec![]
            }
        }
    }

    fn map_esc(&mut self, esc: Esc) -> Vec<Change> {
        match esc {
            Esc::Code(EscCode::DecSaveCursorPosition) => {
                let pos = self.surface().cursor_position();
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
                self.alt_saved_cursor = None;
                self.use_alternate = false;
                self.alternate_surface = None;
                self.application_cursor_keys = false;
                self.cursor_visible = true;
                self.bracketed_paste = false;
                self.mouse_tracking = MouseTrackingMode::None;
                self.sgr_mouse = false;
                self.focus_tracking = false;
                self.scroll_region = None;
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

    fn surface_mut(&mut self) -> &mut Surface {
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

    // ---- DECSET/DECRST handling ----

    fn handle_mode(&mut self, mode: &CsiMode) {
        match mode {
            CsiMode::SetDecPrivateMode(DecPrivateMode::Code(code)) => {
                self.set_dec_mode(code, true);
            }
            CsiMode::ResetDecPrivateMode(DecPrivateMode::Code(code)) => {
                self.set_dec_mode(code, false);
            }
            CsiMode::SetDecPrivateMode(DecPrivateMode::Unspecified(_))
            | CsiMode::ResetDecPrivateMode(DecPrivateMode::Unspecified(_)) => {
                // Unknown mode, ignore
            }
            _ => {}
        }
    }

    fn set_dec_mode(&mut self, code: &DecPrivateModeCode, enable: bool) {
        match *code {
            DecPrivateModeCode::ApplicationCursorKeys => {
                self.application_cursor_keys = enable;
            }
            DecPrivateModeCode::StartBlinkingCursor => {
                // Cursor blink -- no-op for now (rendering doesn't support blink)
            }
            DecPrivateModeCode::ShowCursor => {
                self.cursor_visible = enable;
                let vis = if enable {
                    CursorVisibility::Visible
                } else {
                    CursorVisibility::Hidden
                };
                self.surface_mut().add_change(Change::CursorVisibility(vis));
            }
            DecPrivateModeCode::ClearAndEnableAlternateScreen => {
                // Mode 1049: save cursor, switch to alt screen, clear it
                if enable {
                    // Save cursor on primary
                    let pos = self.primary_surface.cursor_position();
                    self.alt_saved_cursor = Some((pos.0, pos.1));
                    // Create alternate surface if needed
                    if self.alternate_surface.is_none() {
                        self.alternate_surface = Some(Surface::new(self.cols, self.rows));
                    }
                    self.use_alternate = true;
                    // Clear alternate screen
                    if let Some(alt) = &mut self.alternate_surface {
                        alt.add_change(Change::ClearScreen(ColorAttribute::Default));
                        alt.add_change(Change::CursorPosition {
                            x: Position::Absolute(0),
                            y: Position::Absolute(0),
                        });
                    }
                } else {
                    // Leave alternate screen
                    self.use_alternate = false;
                    // Restore cursor on primary
                    if let Some((x, y)) = self.alt_saved_cursor.take() {
                        self.primary_surface.add_change(Change::CursorPosition {
                            x: Position::Absolute(x),
                            y: Position::Absolute(y),
                        });
                    }
                }
            }
            DecPrivateModeCode::EnableAlternateScreen
            | DecPrivateModeCode::OptEnableAlternateScreen => {
                // Mode 47 / 1047: switch without save/clear
                if enable {
                    if self.alternate_surface.is_none() {
                        self.alternate_surface = Some(Surface::new(self.cols, self.rows));
                    }
                    self.use_alternate = true;
                } else {
                    self.use_alternate = false;
                }
            }
            DecPrivateModeCode::SaveCursor => {
                // Mode 1048: save/restore cursor
                if enable {
                    let pos = self.surface().cursor_position();
                    self.saved_cursor = Some((pos.0, pos.1));
                } else if let Some((x, y)) = self.saved_cursor {
                    self.surface_mut().add_change(Change::CursorPosition {
                        x: Position::Absolute(x),
                        y: Position::Absolute(y),
                    });
                }
            }
            DecPrivateModeCode::BracketedPaste => {
                self.bracketed_paste = enable;
            }
            DecPrivateModeCode::MouseTracking => {
                self.mouse_tracking = if enable {
                    MouseTrackingMode::Click
                } else {
                    MouseTrackingMode::None
                };
            }
            DecPrivateModeCode::ButtonEventMouse => {
                // Mode 1002
                self.mouse_tracking = if enable {
                    MouseTrackingMode::CellMotion
                } else {
                    MouseTrackingMode::None
                };
            }
            DecPrivateModeCode::AnyEventMouse => {
                // Mode 1003
                self.mouse_tracking = if enable {
                    MouseTrackingMode::AllMotion
                } else {
                    MouseTrackingMode::None
                };
            }
            DecPrivateModeCode::SGRMouse => {
                self.sgr_mouse = enable;
            }
            DecPrivateModeCode::FocusTracking => {
                self.focus_tracking = enable;
            }
            DecPrivateModeCode::AutoWrap => {
                // AutoWrap is handled by termwiz Surface internally, ignore for now
            }
            _ => {
                // Unknown/unsupported mode, ignore
            }
        }
    }

    /// Get scroll region parameters for ScrollRegionUp/Down changes.
    fn scroll_region_params(&self) -> (usize, usize) {
        match self.scroll_region {
            Some((top, bottom)) => {
                let size = bottom.saturating_sub(top) + 1;
                (top, size)
            }
            None => {
                let (_cols, rows) = self.surface().dimensions();
                (0, rows)
            }
        }
    }

    /// Read characters from a specific line of the active surface, from `start_col` to `end_col`.
    fn read_line_from_surface(&self, row: usize, start_col: usize, end_col: usize) -> String {
        let surface = self.surface();
        let lines = surface.screen_lines();
        if row >= lines.len() {
            return " ".repeat(end_col.saturating_sub(start_col));
        }
        let line = &lines[row];
        let mut result = String::new();
        for col in start_col..end_col {
            if let Some(cell) = line.get_cell(col) {
                result.push_str(cell.str());
            } else {
                result.push(' ');
            }
        }
        result
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

        // Parse DECSET 1 (application cursor keys)
        let mut parser = Parser::new();
        let actions = parser.parse_as_vec(b"\x1b[?1h");
        for action in actions {
            if let Action::CSI(CSI::Mode(ref mode)) = action {
                terminal.handle_mode(mode);
            }
        }
        assert!(terminal.application_cursor_keys());

        // Parse DECRST 1
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

        // DECRST 25 (hide cursor)
        let mut parser = Parser::new();
        let actions = parser.parse_as_vec(b"\x1b[?25l");
        for action in actions {
            if let Action::CSI(CSI::Mode(ref mode)) = action {
                terminal.handle_mode(mode);
            }
        }
        assert!(!terminal.cursor_visible());

        // DECSET 25 (show cursor)
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
        // DECSET 1000 (click tracking)
        let actions = parser.parse_as_vec(b"\x1b[?1000h");
        for action in actions {
            if let Action::CSI(CSI::Mode(ref mode)) = action {
                terminal.handle_mode(mode);
            }
        }
        assert_eq!(terminal.mouse_tracking(), MouseTrackingMode::Click);

        // DECSET 1003 (all motion)
        let actions = parser.parse_as_vec(b"\x1b[?1003h");
        for action in actions {
            if let Action::CSI(CSI::Mode(ref mode)) = action {
                terminal.handle_mode(mode);
            }
        }
        assert_eq!(terminal.mouse_tracking(), MouseTrackingMode::AllMotion);

        // DECRST 1003
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

        // Enter alternate screen (DECSET 1049)
        let mut parser = Parser::new();
        let actions = parser.parse_as_vec(b"\x1b[?1049h");
        for action in actions {
            if let Action::CSI(CSI::Mode(ref mode)) = action {
                terminal.handle_mode(mode);
            }
        }
        assert!(terminal.is_alternate_screen());
        assert!(terminal.alternate_surface.is_some());

        // Leave alternate screen (DECRST 1049)
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

        // Enter alternate screen (mode 47)
        let mut parser = Parser::new();
        let actions = parser.parse_as_vec(b"\x1b[?47h");
        for action in actions {
            if let Action::CSI(CSI::Mode(ref mode)) = action {
                terminal.handle_mode(mode);
            }
        }
        assert!(terminal.is_alternate_screen());

        // Leave
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

        // Enter alternate screen
        let mut parser = Parser::new();
        let actions = parser.parse_as_vec(b"\x1b[?1049h");
        for action in actions {
            if let Action::CSI(CSI::Mode(ref mode)) = action {
                terminal.handle_mode(mode);
            }
        }

        // Resize should affect both surfaces
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

        // Normal mode
        assert!(!terminal.application_cursor_keys());
        // Application cursor keys enabled
        let mut parser = Parser::new();
        let actions = parser.parse_as_vec(b"\x1b[?1h");
        for action in actions {
            if let Action::CSI(CSI::Mode(ref mode)) = action {
                terminal.handle_mode(mode);
            }
        }
        assert!(terminal.application_cursor_keys());
        // In application mode, arrow keys should send \x1bO{A..D}
    }

    // ---- Full reset test ----

    #[test]
    fn full_reset_clears_modes() {
        let waker = noop_waker();
        let mut terminal = Terminal::new(80, 24, waker).expect("terminal creation");

        // Set several modes
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

        // Full reset via ESC c
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
