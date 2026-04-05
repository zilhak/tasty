use termwiz::cell::{unicode_column_width, AttributeChange, CellAttributes};
use termwiz::color::ColorAttribute;
use termwiz::escape::csi::{Cursor, Device, Edit, EraseInDisplay, EraseInLine, Sgr, CSI};
use termwiz::escape::esc::{Esc, EscCode};
use termwiz::escape::{Action, ControlCode, OperatingSystemCommand};
use termwiz::surface::{Change, CursorVisibility, Position};

use super::{MouseTrackingMode, Terminal, TerminalEvent, TerminalEventKind};

impl Terminal {
    /// Convert a parsed VT action into Surface changes.
    pub(crate) fn action_to_changes(&mut self, action: Action) -> Vec<Change> {
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

    /// Perform a line feed (Index): move cursor down one line.
    /// If the cursor is at the bottom of the scroll region, scroll the region up.
    pub(crate) fn perform_index(&mut self) -> Vec<Change> {
        let (_cx, cy) = self.surface().cursor_position();
        let (top, size) = self.scroll_region_params();
        let bottom = top + size - 1;

        if cy == bottom {
            // Cursor is at the bottom of the scroll region — scroll region up
            vec![Change::ScrollRegionUp {
                first_row: top,
                region_size: size,
                scroll_count: 1,
            }]
        } else {
            // Normal line feed — just move cursor down.
            // Use CursorPosition instead of Text("\n") because:
            // 1. termwiz Surface's print_text("\n") calls scroll_screen_up() at the
            //    bottom row, which ignores scroll regions and scrolls the entire screen.
            // 2. During synchronized output (mode 2026), changes are staged and flushed
            //    later. The cursor position at flush time may differ from when this
            //    decision was made, causing Text("\n") to trigger unexpected scrolls.
            // CursorPosition with Relative(1) safely clamps at the bottom without scrolling.
            vec![Change::CursorPosition {
                x: Position::Relative(0),
                y: Position::Relative(1),
            }]
        }
    }

    /// Perform a reverse index: move cursor up one line.
    /// If the cursor is at the top of the scroll region, scroll the region down.
    pub(crate) fn perform_reverse_index(&mut self) -> Vec<Change> {
        let (_cx, cy) = self.surface().cursor_position();
        let (top, size) = self.scroll_region_params();

        if cy == top {
            // Cursor is at the top of the scroll region — scroll region down
            vec![Change::ScrollRegionDown {
                first_row: top,
                region_size: size,
                scroll_count: 1,
            }]
        } else {
            // Normal cursor up
            vec![Change::CursorPosition {
                x: Position::Relative(0),
                y: Position::Relative(-1),
            }]
        }
    }

    pub(crate) fn map_control(&mut self, code: ControlCode) -> Vec<Change> {
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

    pub(crate) fn map_csi(&mut self, csi: CSI) -> Vec<Change> {
        match csi {
            CSI::Sgr(sgr) => self.map_sgr(sgr),
            CSI::Cursor(cursor) => self.map_cursor(cursor),
            CSI::Edit(edit) => self.map_edit(edit),
            CSI::Mode(_mode) => {
                // Handled in process() via handle_mode() before reaching here.
                vec![]
            }
            CSI::Device(device) => {
                self.handle_device(*device);
                vec![]
            }
            CSI::Mouse(_) => vec![],
            CSI::Window(_) => vec![],
            CSI::Keyboard(_) => vec![],
            _ => vec![],
        }
    }

    pub(crate) fn map_sgr(&self, sgr: Sgr) -> Vec<Change> {
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

    pub(crate) fn map_cursor(&mut self, cursor: Cursor) -> Vec<Change> {
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
            Cursor::RequestActivePositionReport => {
                let (x, y) = self.surface().cursor_position();
                self.send_terminal_response(&format!("\x1b[{};{}R", y + 1, x + 1));
                vec![]
            }
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

    pub(crate) fn map_edit(&mut self, edit: Edit) -> Vec<Change> {
        match edit {
            Edit::EraseInDisplay(mode) => match mode {
                EraseInDisplay::EraseToEndOfDisplay => {
                    vec![Change::ClearToEndOfScreen(ColorAttribute::Default)]
                }
                EraseInDisplay::EraseToStartOfDisplay => {
                    let (cx, cy) = self.surface().cursor_position();
                    let (cols, _rows) = self.surface().dimensions();
                    let mut changes = Vec::new();
                    for row in 0..cy {
                        changes.push(Change::CursorPosition {
                            x: Position::Absolute(0),
                            y: Position::Absolute(row),
                        });
                        changes.push(Change::ClearToEndOfLine(ColorAttribute::Default));
                    }
                    changes.push(Change::CursorPosition {
                        x: Position::Absolute(0),
                        y: Position::Absolute(cy),
                    });
                    if cx < cols {
                        changes.push(Change::Text(" ".repeat(cx + 1)));
                    }
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
                let (cx, cy) = self.surface().cursor_position();
                let (cols, _rows) = self.surface().dimensions();
                let remaining = cols.saturating_sub(cx);
                let n = (n as usize).min(remaining);
                if n == 0 {
                    return vec![];
                }
                let line = self.read_line_from_surface(cy, cx, cols);
                // Skip n columns worth of characters (n is in cells, not chars)
                let mut skip_cols = 0;
                let mut skip_chars = 0;
                for ch in line.chars() {
                    if skip_cols >= n {
                        break;
                    }
                    skip_cols += unicode_column_width(&ch.to_string(), None);
                    skip_chars += 1;
                }
                let after: String = line.chars().skip(skip_chars).collect();
                let after_width: usize = after
                    .chars()
                    .map(|c| unicode_column_width(&c.to_string(), None))
                    .sum();
                let mut text = after;
                for _ in 0..remaining.saturating_sub(after_width) {
                    text.push(' ');
                }
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
                let (cx, cy) = self.surface().cursor_position();
                let (cols, _rows) = self.surface().dimensions();
                let remaining = cols.saturating_sub(cx);
                let n = (n as usize).min(remaining);
                if n == 0 {
                    return vec![];
                }
                let line = self.read_line_from_surface(cy, cx, cols);
                // Insert n blank columns, then append existing content that fits
                let mut text = " ".repeat(n);
                let mut used_cols = n;
                for ch in line.chars() {
                    let w = unicode_column_width(&ch.to_string(), None);
                    if used_cols + w > remaining {
                        break;
                    }
                    text.push(ch);
                    used_cols += w;
                }
                while used_cols < remaining {
                    text.push(' ');
                    used_cols += 1;
                }
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
            Edit::DeleteLine(n) => {
                let (_cx, cy) = self.surface().cursor_position();
                let (first_row, region_size) = self.scroll_region_params();
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
                let _ = n;
                vec![]
            }
        }
    }

    pub(crate) fn map_esc(&mut self, esc: Esc) -> Vec<Change> {
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
            Esc::Code(EscCode::Index) => {
                self.perform_index()
            }
            Esc::Code(EscCode::ReverseIndex) => {
                self.perform_reverse_index()
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

    pub(crate) fn map_osc(&mut self, osc: OperatingSystemCommand) {
        match osc {
            OperatingSystemCommand::SetIconNameAndWindowTitle(title) => {
                self.events.push(TerminalEvent {
                    surface_id: 0,
                    kind: TerminalEventKind::TitleChanged(title),
                });
            }
            OperatingSystemCommand::SetWindowTitle(title)
            | OperatingSystemCommand::SetWindowTitleSun(title) => {
                self.events.push(TerminalEvent {
                    surface_id: 0,
                    kind: TerminalEventKind::TitleChanged(title),
                });
            }
            OperatingSystemCommand::CurrentWorkingDirectory(url) => {
                let path = if let Some(stripped) = url.strip_prefix("file://") {
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
            OperatingSystemCommand::SystemNotification(body) => {
                self.events.push(TerminalEvent {
                    surface_id: 0,
                    kind: TerminalEventKind::Notification {
                        title: "Terminal".to_string(),
                        body,
                    },
                });
            }
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
            OperatingSystemCommand::SetSelection(_selection, data) => {
                self.events.push(TerminalEvent {
                    surface_id: 0,
                    kind: TerminalEventKind::ClipboardSet(data),
                });
            }
            OperatingSystemCommand::Unspecified(params) => {
                if let Some(first) = params.first() {
                    if first == b"99" {
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

    fn handle_device(&mut self, device: Device) {
        match device {
            Device::StatusReport => self.send_terminal_response("\x1b[0n"),
            Device::RequestPrimaryDeviceAttributes => {
                self.send_terminal_response("\x1b[?1;2c");
            }
            _ => {}
        }
    }

    /// Read characters from a specific line of the active surface, from `start_col` to `end_col`.
    /// Uses `visible_cells()` to correctly skip continuation cells of wide characters,
    /// avoiding spurious spaces that would corrupt DCH/ICH operations.
    pub(crate) fn read_line_from_surface(&self, row: usize, start_col: usize, end_col: usize) -> String {
        let surface = self.surface();
        let lines = surface.screen_lines();
        if row >= lines.len() {
            return " ".repeat(end_col.saturating_sub(start_col));
        }
        let line = &lines[row];
        let mut result = String::new();
        for cell in line.visible_cells() {
            let idx = cell.cell_index();
            if idx >= end_col {
                break;
            }
            if idx >= start_col {
                result.push_str(cell.str());
            }
        }
        result
    }

    /// Get scroll region parameters for ScrollRegionUp/Down changes.
    pub(crate) fn scroll_region_params(&self) -> (usize, usize) {
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
}
