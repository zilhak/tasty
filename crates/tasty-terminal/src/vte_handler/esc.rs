//! VTE handler: esc 도메인.

use termwiz::cell::CellAttributes;
use termwiz::color::ColorAttribute;
use termwiz::escape::esc::{Esc, EscCode};
use termwiz::surface::{Change, CursorVisibility, Position};

use crate::{MouseTrackingMode, Terminal};

impl Terminal {
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
}
