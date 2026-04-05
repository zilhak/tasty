/// Test helper: lightweight terminal emulator without PTY.
/// Processes VTE sequences and applies them to a Surface.
/// Used for unit testing terminal rendering without spawning real shells.

use termwiz::escape::parser::Parser;
use termwiz::escape::csi::{CSI, Device};
use termwiz::escape::Action;
use termwiz::surface::{Change, Position, Surface};

use crate::events::*;

/// A lightweight terminal for testing — no PTY, no threads.
pub struct TestTerminal {
    pub surface: Surface,
    pub parser: Parser,
    pub events: Vec<TerminalEvent>,
    pub cols: usize,
    pub rows: usize,
    // Mode state
    pub application_cursor_keys: bool,
    pub cursor_visible: bool,
    pub bracketed_paste: bool,
    pub use_alternate: bool,
    pub alternate_surface: Option<Surface>,
    pub saved_cursor: Option<(usize, usize)>,
    pub alt_saved_cursor: Option<(usize, usize)>,
    pub mouse_tracking: MouseTrackingMode,
    pub sgr_mouse: bool,
    pub focus_tracking: bool,
    pub scroll_region: Option<(usize, usize)>,
    pub synchronized_output: bool,
    pub sent_bytes: Vec<u8>,
}

impl TestTerminal {
    pub fn new(cols: usize, rows: usize) -> Self {
        Self {
            surface: Surface::new(cols, rows),
            parser: Parser::new(),
            events: Vec::new(),
            cols,
            rows,
            application_cursor_keys: false,
            cursor_visible: true,
            bracketed_paste: false,
            use_alternate: false,
            alternate_surface: None,
            saved_cursor: None,
            alt_saved_cursor: None,
            mouse_tracking: MouseTrackingMode::None,
            sgr_mouse: false,
            focus_tracking: false,
            scroll_region: None,
            synchronized_output: false,
            sent_bytes: Vec::new(),
        }
    }

    /// Feed raw bytes and process through VTE parser.
    pub fn feed(&mut self, data: &[u8]) {
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

    /// Feed a string.
    pub fn feed_str(&mut self, s: &str) {
        self.feed(s.as_bytes());
    }

    /// Get text content of a specific row (0-indexed), trailing spaces trimmed.
    pub fn row(&self, row: usize) -> String {
        let surface = self.active_surface();
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

    /// Get all rows as a Vec<String>.
    pub fn rows_text(&self) -> Vec<String> {
        let surface = self.active_surface();
        let lines = surface.screen_lines();
        lines.iter().map(|line| {
            let mut text = String::new();
            for cell in line.visible_cells() {
                text.push_str(cell.str());
            }
            text.trim_end().to_string()
        }).collect()
    }

    fn active_surface(&self) -> &Surface {
        if self.use_alternate {
            self.alternate_surface.as_ref().unwrap_or(&self.surface)
        } else {
            &self.surface
        }
    }

    fn active_surface_mut(&mut self) -> &mut Surface {
        if self.use_alternate {
            self.alternate_surface.as_mut().unwrap_or(&mut self.surface)
        } else {
            &mut self.surface
        }
    }

    fn apply_or_stage_change(&mut self, change: Change) {
        // Always apply immediately — see Terminal::apply_or_stage_change() rationale.
        self.active_surface_mut().add_change(change);
    }

    fn flush_pending_changes(&mut self) {
        // No-op: changes are applied immediately.
    }

    fn send_terminal_response(&mut self, response: &str) {
        self.sent_bytes.extend_from_slice(response.as_bytes());
    }
}

// Implement the same VTE handler methods on TestTerminal
// by delegating to the same logic. We reuse the same patterns
// but on the TestTerminal struct directly.

use termwiz::cell::{AttributeChange, CellAttributes};
use termwiz::escape::csi::{Cursor, Edit, EraseInDisplay, EraseInLine, Sgr};
use termwiz::escape::esc::{Esc, EscCode};
use termwiz::escape::ControlCode;

impl TestTerminal {
    pub fn action_to_changes(&mut self, action: Action) -> Vec<Change> {
        match action {
            Action::Print(c) => vec![Change::Text(c.to_string())],
            Action::PrintString(s) => vec![Change::Text(s)],
            Action::Control(code) => self.map_control(code),
            Action::CSI(csi) => self.map_csi(csi),
            Action::Esc(esc) => self.map_esc(esc),
            Action::OperatingSystemCommand(_osc) => vec![],
            _ => vec![],
        }
    }

    fn perform_index(&mut self) -> Vec<Change> {
        let (_cx, cy) = self.active_surface().cursor_position();
        let (top, size) = self.scroll_region_params();
        let bottom = top + size - 1;
        if cy == bottom {
            vec![Change::ScrollRegionUp {
                first_row: top,
                region_size: size,
                scroll_count: 1,
            }]
        } else {
            vec![Change::CursorPosition {
                x: Position::Relative(0),
                y: Position::Relative(1),
            }]
        }
    }

    fn perform_reverse_index(&mut self) -> Vec<Change> {
        let (_cx, cy) = self.active_surface().cursor_position();
        let (top, size) = self.scroll_region_params();
        if cy == top {
            vec![Change::ScrollRegionDown {
                first_row: top,
                region_size: size,
                scroll_count: 1,
            }]
        } else {
            vec![Change::CursorPosition {
                x: Position::Relative(0),
                y: Position::Relative(-1),
            }]
        }
    }

    fn scroll_region_params(&self) -> (usize, usize) {
        match self.scroll_region {
            Some((top, bottom)) => {
                let size = bottom.saturating_sub(top) + 1;
                (top, size)
            }
            None => (0, self.rows),
        }
    }

    fn map_control(&mut self, code: ControlCode) -> Vec<Change> {
        match code {
            ControlCode::LineFeed | ControlCode::VerticalTab | ControlCode::FormFeed => {
                self.perform_index()
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
            CSI::Device(device) => {
                self.handle_device(*device);
                vec![]
            }
            _ => vec![],
        }
    }

    fn map_sgr(&self, sgr: Sgr) -> Vec<Change> {
        match sgr {
            Sgr::Reset => vec![Change::AllAttributes(CellAttributes::default())],
            Sgr::Intensity(i) => vec![Change::Attribute(AttributeChange::Intensity(i))],
            Sgr::Underline(u) => vec![Change::Attribute(AttributeChange::Underline(u))],
            Sgr::Italic(i) => vec![Change::Attribute(AttributeChange::Italic(i))],
            Sgr::Foreground(c) => vec![Change::Attribute(AttributeChange::Foreground(c.into()))],
            Sgr::Background(c) => vec![Change::Attribute(AttributeChange::Background(c.into()))],
            Sgr::StrikeThrough(s) => vec![Change::Attribute(AttributeChange::StrikeThrough(s))],
            Sgr::Inverse(i) => vec![Change::Attribute(AttributeChange::Reverse(i))],
            Sgr::Invisible(i) => vec![Change::Attribute(AttributeChange::Invisible(i))],
            Sgr::Blink(b) => vec![Change::Attribute(AttributeChange::Blink(b))],
            _ => vec![],
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
            Cursor::CharacterAbsolute(col) => vec![Change::CursorPosition {
                x: Position::Absolute(col.as_zero_based() as usize),
                y: Position::Relative(0),
            }],
            Cursor::RequestActivePositionReport => {
                let (x, y) = self.active_surface().cursor_position();
                self.send_terminal_response(&format!("\x1b[{};{}R", y + 1, x + 1));
                vec![]
            }
            _ => vec![],
        }
    }

    fn map_edit(&self, edit: Edit) -> Vec<Change> {
        match edit {
            Edit::EraseInDisplay(mode) => match mode {
                EraseInDisplay::EraseToEndOfDisplay => {
                    vec![Change::ClearToEndOfScreen(Default::default())]
                }
                EraseInDisplay::EraseDisplay => {
                    vec![Change::ClearScreen(Default::default())]
                }
                _ => vec![],
            },
            Edit::EraseInLine(mode) => match mode {
                EraseInLine::EraseToEndOfLine => {
                    vec![Change::ClearToEndOfLine(Default::default())]
                }
                _ => vec![],
            },
            _ => vec![],
        }
    }

    fn map_esc(&mut self, esc: Esc) -> Vec<Change> {
        match esc {
            Esc::Code(EscCode::Index) => {
                self.perform_index()
            }
            Esc::Code(EscCode::ReverseIndex) => {
                self.perform_reverse_index()
            }
            Esc::Code(EscCode::FullReset) => {
                self.application_cursor_keys = false;
                self.cursor_visible = true;
                self.bracketed_paste = false;
                self.use_alternate = false;
                self.scroll_region = None;
                vec![Change::ClearScreen(Default::default())]
            }
            _ => vec![],
        }
    }

    fn handle_mode(&mut self, mode: &termwiz::escape::csi::Mode) {
        use termwiz::escape::csi::{Mode as CsiMode, DecPrivateMode};
        match mode {
            CsiMode::SetDecPrivateMode(DecPrivateMode::Code(code)) => self.set_dec(code.clone(), true),
            CsiMode::ResetDecPrivateMode(DecPrivateMode::Code(code)) => self.set_dec(code.clone(), false),
            _ => {}
        }
    }

    fn set_dec(&mut self, code: termwiz::escape::csi::DecPrivateModeCode, enable: bool) {
        use termwiz::escape::csi::DecPrivateModeCode;
        match code {
            DecPrivateModeCode::ApplicationCursorKeys => self.application_cursor_keys = enable,
            DecPrivateModeCode::ShowCursor => self.cursor_visible = enable,
            DecPrivateModeCode::ClearAndEnableAlternateScreen => {
                if enable {
                    if self.alternate_surface.is_none() {
                        self.alternate_surface = Some(Surface::new(self.cols, self.rows));
                    }
                    self.use_alternate = true;
                    if let Some(alt) = &mut self.alternate_surface {
                        alt.add_change(Change::ClearScreen(Default::default()));
                    }
                } else {
                    self.use_alternate = false;
                }
            }
            DecPrivateModeCode::BracketedPaste => self.bracketed_paste = enable,
            DecPrivateModeCode::SynchronizedOutput => {
                self.synchronized_output = enable;
                if !enable {
                    self.flush_pending_changes();
                }
            }
            _ => {}
        }
    }

    fn handle_device(&mut self, device: Device) {
        match device {
            Device::StatusReport => self.send_terminal_response("\x1b[0n"),
            Device::RequestPrimaryDeviceAttributes => {
                self.send_terminal_response("\x1b[?1;2c");
            }
            _ => {}
        }
    }
}
