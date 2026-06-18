//! VTE handler: esc 도메인.

use termwiz::cell::CellAttributes;
use termwiz::color::ColorAttribute;
use termwiz::escape::esc::{Esc, EscCode};
use termwiz::surface::{Change, CursorVisibility, Position};

use crate::{CursorShape, MouseTrackingMode, TerminalState};

impl TerminalState {
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
            Esc::Code(EscCode::Index) => self.perform_index(),
            Esc::Code(EscCode::ReverseIndex) => self.perform_reverse_index(),
            Esc::Code(EscCode::NextLine) => {
                // NEL (ESC E): index (line feed, scrolling at the region bottom)
                // followed by a carriage return — cursor lands at column 0 of the
                // next line.
                let mut changes = self.perform_index();
                changes.push(Change::CursorPosition {
                    x: Position::Absolute(0),
                    y: Position::Relative(0),
                });
                changes
            }
            Esc::Code(EscCode::DecScreenAlignmentDisplay) => {
                // DECALN (ESC # 8): fill the entire screen with 'E' for alignment
                // testing. Resets pen attributes and homes the cursor. Each row is
                // written explicitly (cursor positioned per row) so the fill never
                // relies on auto-wrap.
                let mut changes = vec![Change::AllAttributes(CellAttributes::default())];
                for row in 0..self.rows {
                    changes.push(Change::CursorPosition {
                        x: Position::Absolute(0),
                        y: Position::Absolute(row),
                    });
                    changes.push(Change::Text("E".repeat(self.cols)));
                }
                changes.push(Change::CursorPosition {
                    x: Position::Absolute(0),
                    y: Position::Absolute(0),
                });
                changes
            }
            Esc::Code(EscCode::FullReset) => {
                self.saved_cursor = None;
                self.alt_saved_cursor = None;
                self.use_alternate = false;
                self.alternate_surface = None;
                // The returned AllAttributes(default) resets `current_pen` via the
                // normal apply path; reset the inactive-surface mirror too so no
                // stale pen survives the full reset.
                self.saved_pen = CellAttributes::default();
                self.application_cursor_keys = false;
                self.cursor_visible = true;
                self.cursor_shape = CursorShape::Default;
                self.bracketed_paste = false;
                self.mouse_tracking = MouseTrackingMode::None;
                self.sgr_mouse = false;
                self.focus_tracking = false;
                self.insert_mode = false;
                self.scroll_region = None;
                self.last_print = None;
                vec![
                    Change::AllAttributes(CellAttributes::default()),
                    Change::ClearScreen(ColorAttribute::Default),
                    Change::CursorVisibility(CursorVisibility::Visible),
                ]
            }
            _ => vec![],
        }
    }
}
