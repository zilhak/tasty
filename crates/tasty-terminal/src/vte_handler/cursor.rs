//! VTE handler: cursor 도메인.

use termwiz::escape::csi::{Cursor, CursorStyle, CursorTabulationControl, TabulationClear};
use termwiz::surface::{Change, Position};

use crate::{CursorShape, TerminalState};

impl TerminalState {
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
                // CHT: move forward n tab stops, clamped at the right margin.
                let (cx, _cy) = self.surface().cursor_position();
                let mut col = cx;
                for _ in 0..n {
                    col = self.next_tab_stop(col);
                }
                vec![Change::CursorPosition {
                    x: Position::Absolute(col),
                    y: Position::Relative(0),
                }]
            }
            Cursor::BackwardTabulation(n) => {
                // CBT: move backward n tab stops, clamped at the left margin.
                let (cx, _cy) = self.surface().cursor_position();
                let mut col = cx;
                for _ in 0..n {
                    col = self.prev_tab_stop(col);
                }
                vec![Change::CursorPosition {
                    x: Position::Absolute(col),
                    y: Position::Relative(0),
                }]
            }
            Cursor::TabulationClear(mode) => {
                // TBC: clear character tab stops. Line tab stops are unsupported.
                match mode {
                    TabulationClear::ClearCharacterTabStopAtActivePosition => {
                        let (cx, _cy) = self.surface().cursor_position();
                        self.clear_tab_stop(cx);
                    }
                    TabulationClear::ClearAllCharacterTabStops
                    | TabulationClear::ClearAllTabStops => {
                        self.clear_all_tab_stops();
                    }
                    _ => {}
                }
                vec![]
            }
            Cursor::TabulationControl(ctrl) => {
                // CTC (CSI W): set/clear character tab stops at the active column.
                match ctrl {
                    CursorTabulationControl::SetCharacterTabStopAtActivePosition => {
                        let (cx, _cy) = self.surface().cursor_position();
                        self.set_tab_stop(cx);
                    }
                    CursorTabulationControl::ClearCharacterTabStopAtActivePosition => {
                        let (cx, _cy) = self.surface().cursor_position();
                        self.clear_tab_stop(cx);
                    }
                    CursorTabulationControl::ClearAllCharacterTabStops => {
                        self.clear_all_tab_stops();
                    }
                    _ => {}
                }
                vec![]
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
            Cursor::CursorStyle(style) => {
                // DECSCUSR (CSI Ps SP q): record the requested cursor shape so the
                // renderer can read it via cursor_shape(). xterm parameter mapping:
                // 0/Default → default, odd = blinking, even = steady.
                self.cursor_shape = match style {
                    CursorStyle::Default => CursorShape::Default,
                    CursorStyle::BlinkingBlock => CursorShape::BlinkingBlock,
                    CursorStyle::SteadyBlock => CursorShape::SteadyBlock,
                    CursorStyle::BlinkingUnderline => CursorShape::BlinkingUnderline,
                    CursorStyle::SteadyUnderline => CursorShape::SteadyUnderline,
                    CursorStyle::BlinkingBar => CursorShape::BlinkingBar,
                    CursorStyle::SteadyBar => CursorShape::SteadyBar,
                };
                vec![]
            }
            _ => vec![],
        }
    }
}
