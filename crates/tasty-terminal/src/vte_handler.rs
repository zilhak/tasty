use termwiz::cell::unicode_column_width;
use termwiz::escape::{Action, ControlCode};
use termwiz::surface::Change;

use super::{TerminalEvent, TerminalEventKind, TerminalState};

impl TerminalState {
    /// Convert a parsed VT action into Surface changes.
    pub(crate) fn action_to_changes(&mut self, action: Action) -> Vec<Change> {
        match action {
            Action::Print(c) => {
                self.emit_output_text(|| c.to_string());
                let text = c.to_string();
                self.print_with_insert_mode(unicode_column_width(&text, None), text)
            }
            Action::PrintString(s) => {
                self.emit_output_text(|| s.clone());
                let width = unicode_column_width(&s, None);
                self.print_with_insert_mode(width, s)
            }
            Action::Control(code) => {
                if matches!(code, ControlCode::LineFeed) {
                    self.emit_output_text(|| "\n".to_string());
                }
                self.map_control(code)
            }
            Action::CSI(csi) => self.map_csi(csi),
            Action::Esc(esc) => self.map_esc(esc),
            Action::OperatingSystemCommand(osc) => {
                self.map_osc(*osc);
                vec![]
            }
            // XtGetTcap (DCS + q ... ST): termcap/terminfo capability query.
            // Answered with a "currently not supported" reply (see handler).
            Action::XtGetTcap(names) => {
                self.handle_xtgettcap(&names);
                vec![]
            }
            _ => vec![],
        }
    }

    /// Build the changes for printing `text` (column width `width`). When IRM
    /// (insert mode) is active, existing cells are shifted right by `width`
    /// columns first (ICH semantics) so the glyphs are inserted rather than
    /// overwriting. The shift Change is emitted before the Text, keeping the
    /// Print on its normal (non-scrolling) path.
    fn print_with_insert_mode(&self, width: usize, text: String) -> Vec<Change> {
        if !self.insert_mode {
            return vec![Change::Text(text)];
        }
        let mut changes = self.insert_blank_changes(width);
        changes.push(Change::Text(text));
        changes
    }

    /// Push an `OutputAppended` event for the observer router. The text is
    /// built lazily so a disabled gate (no observers — the common case) costs
    /// no allocation. `surface_id = 0` here; the host fills it on
    /// `collect_events`.
    fn emit_output_text(&mut self, make_text: impl FnOnce() -> String) {
        if !self.emit_output_events {
            return;
        }
        let text = make_text();
        if text.is_empty() {
            return;
        }
        self.events.push(TerminalEvent {
            surface_id: 0,
            kind: TerminalEventKind::OutputAppended { text },
        });
    }

    /// Perform a line feed (Index): move cursor down one line.
    /// If the cursor is at the bottom of the scroll region, scroll the region up.
    pub(crate) fn read_line_from_surface(
        &self,
        row: usize,
        start_col: usize,
        end_col: usize,
    ) -> String {
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

mod control;
mod cursor;
mod edit;
mod esc;
mod osc;
