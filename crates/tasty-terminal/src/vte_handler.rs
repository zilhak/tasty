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
                self.last_print = Some(text.clone());
                self.print_with_insert_mode(unicode_column_width(&text, None), text)
            }
            Action::PrintString(s) => {
                self.emit_output_text(|| s.clone());
                if let Some(last) = s.chars().last() {
                    self.last_print = Some(last.to_string());
                }
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

    /// Column of the next tab stop strictly to the right of `col`. Falls back to
    /// the last column when no further stop exists (HT never wraps to the next
    /// line; it clamps at the right margin).
    pub(crate) fn next_tab_stop(&self, col: usize) -> usize {
        let last = self.cols.saturating_sub(1);
        for c in (col + 1)..self.cols {
            if self.tab_stops.get(c).copied().unwrap_or(false) {
                return c;
            }
        }
        last
    }

    /// Column of the previous tab stop strictly to the left of `col`. Falls back
    /// to column 0 (CBT clamps at the left margin).
    pub(crate) fn prev_tab_stop(&self, col: usize) -> usize {
        for c in (0..col).rev() {
            if self.tab_stops.get(c).copied().unwrap_or(false) {
                return c;
            }
        }
        0
    }

    /// Set a tab stop at `col` (HTS).
    pub(crate) fn set_tab_stop(&mut self, col: usize) {
        if let Some(slot) = self.tab_stops.get_mut(col) {
            *slot = true;
        }
    }

    /// Clear the tab stop at `col` (TBC 0).
    pub(crate) fn clear_tab_stop(&mut self, col: usize) {
        if let Some(slot) = self.tab_stops.get_mut(col) {
            *slot = false;
        }
    }

    /// Clear every tab stop (TBC 3).
    pub(crate) fn clear_all_tab_stops(&mut self) {
        self.tab_stops.iter_mut().for_each(|s| *s = false);
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
