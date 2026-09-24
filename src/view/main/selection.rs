use crate::core::intent::{DomainIntent, SendPayload};
use crate::model::PhysicalRect;
use crate::selection::{self, SelectionMode, SelectionPoint, TextSelection};
use crate::view::ui::View;

use super::MainView;

impl MainView {
    /// Extend the current selection (or create one from last click) to the given position.
    /// Used for Shift+Click range selection.
    pub(super) fn extend_selection(&mut self, x: f32, y: f32, terminal_rect: &PhysicalRect) {
        if let Some((point, surface_id)) = self.mouse_to_grid(x, y, terminal_rect) {
            if let Some(sel) = &mut self.text_selection {
                // Existing selection: keep anchor, move cursor
                if sel.surface_id == surface_id {
                    sel.cursor = point;
                    sel.mode = SelectionMode::Normal;
                    sel.dragging = true;
                }
            } else if let Some((col, abs_row)) = self.last_click_pos {
                // No selection but have a previous click position: use it as anchor
                self.text_selection = Some(TextSelection {
                    anchor: SelectionPoint {
                        col,
                        absolute_row: abs_row,
                    },
                    cursor: point,
                    mode: SelectionMode::Normal,
                    surface_id,
                    dragging: true,
                });
            }
            self.mark_dirty();
        }
    }

    /// Start a new text selection from the given pixel position.
    pub(super) fn start_selection(&mut self, x: f32, y: f32, terminal_rect: &PhysicalRect) {
        if let Some((point, surface_id)) = self.mouse_to_grid(x, y, terminal_rect) {
            // Detect multi-click
            let now = std::time::Instant::now();
            let same_pos = self
                .last_click_pos
                .is_some_and(|(c, r)| c == point.col && r == point.absolute_row);
            let within_time = self
                .last_click_time
                .is_some_and(|t| now.duration_since(t).as_millis() < 400);
            if same_pos && within_time {
                self.click_count = (self.click_count + 1).min(3);
            } else {
                self.click_count = 1;
            }
            self.last_click_time = Some(now);
            self.last_click_pos = Some((point.col, point.absolute_row));

            let (mode, dragging) = match self.click_count {
                2 => (SelectionMode::Word, false),
                3 => {
                    self.click_count = 0; // Reset after triple
                    (SelectionMode::Line, false)
                }
                _ => (SelectionMode::Normal, true),
            };

            // For word/line mode, expand anchor/cursor
            let (anchor, cursor) = match mode {
                SelectionMode::Word => {
                    let (start_col, end_col) = self.find_word_bounds(point.col, point.absolute_row);
                    (
                        SelectionPoint {
                            col: start_col,
                            absolute_row: point.absolute_row,
                        },
                        SelectionPoint {
                            col: end_col,
                            absolute_row: point.absolute_row,
                        },
                    )
                }
                SelectionMode::Line => {
                    let cols = self
                        .state
                        .focused_terminal(&self.core_state)
                        .map(|t| t.dimensions().0)
                        .unwrap_or(80);
                    (
                        SelectionPoint {
                            col: 0,
                            absolute_row: point.absolute_row,
                        },
                        SelectionPoint {
                            col: cols.saturating_sub(1),
                            absolute_row: point.absolute_row,
                        },
                    )
                }
                SelectionMode::Normal | SelectionMode::Block => {
                    // Clear any existing selection on single click. Block mode
                    // is never produced by mouse click — defensive default.
                    (point, point)
                }
            };

            self.text_selection = Some(TextSelection {
                anchor,
                cursor,
                mode,
                surface_id,
                dragging,
            });
            self.mark_dirty();
        } else {
            // Clicked outside terminal — clear selection

            self.text_selection = None;
        }
    }

    /// Find word boundaries around the given column in the given absolute row.
    fn find_word_bounds(&self, col: usize, absolute_row: usize) -> (usize, usize) {
        let engine = &self.core_state;
        let terminal = match self.state.focused_terminal(engine) {
            Some(t) => t,
            None => return (col, col),
        };
        // Snapshot scrollback length and the target row under a single state lock
        // so the parser thread cannot shift scrollback_len relative to the line
        // read between locks (ADR-0013).
        let row_text: Vec<(String, usize)> =
            match terminal.with_render_view(|v| -> Option<Vec<(String, usize)>> {
                let scrollback_len = v.scrollback_len();
                if absolute_row < scrollback_len {
                    v.scrollback_line(absolute_row).map(|line| {
                        let mut result = Vec::new();
                        let mut c = 0;
                        for (text, _) in line.cells() {
                            let ch = text.chars().next().unwrap_or(' ');
                            let w = tasty_cell_width::unicode_width(ch);
                            result.push((text.to_string(), c));
                            c += w;
                        }
                        result
                    })
                } else {
                    let screen_row = absolute_row - scrollback_len;
                    v.surface().screen_lines().get(screen_row).map(|line| {
                        line.visible_cells()
                            .map(|cell| (cell.str().to_string(), cell.cell_index()))
                            .collect()
                    })
                }
            }) {
                Some(rt) => rt,
                None => return (col, col),
            };

        // Find which cell the col is in
        let is_word_char = |s: &str| -> bool {
            s.chars()
                .next()
                .is_some_and(|c| c.is_alphanumeric() || c == '_')
        };

        // Find the cell at col
        let target_idx = row_text
            .iter()
            .position(|(_, c)| *c >= col)
            .unwrap_or(row_text.len().saturating_sub(1));
        if row_text.is_empty() {
            return (col, col);
        }
        let target_idx = target_idx.min(row_text.len() - 1);
        let word = is_word_char(&row_text[target_idx].0);

        // Expand left
        let mut start = target_idx;
        while start > 0 && is_word_char(&row_text[start - 1].0) == word {
            start -= 1;
        }
        // Expand right
        let mut end = target_idx;
        while end + 1 < row_text.len() && is_word_char(&row_text[end + 1].0) == word {
            end += 1;
        }

        let start_col = row_text[start].1;
        let end_text = &row_text[end].0;
        let end_ch = end_text.chars().next().unwrap_or(' ');
        let end_col = row_text[end].1 + tasty_cell_width::unicode_width(end_ch) - 1;
        (start_col, end_col)
    }

    /// Move the terminal cursor to the clicked position using the click_cursor module.
    pub(super) fn move_cursor_to_click(&mut self, x: f32, y: f32, terminal_rect: &PhysicalRect) {
        let engine = &mut self.core_state;
        if !engine.settings.general.click_to_move_cursor {
            return;
        }

        // Only move cursor when a shell is in the foreground.
        // Non-shell programs have their own cursor semantics that don't match
        // terminal grid positions, so arrow-key injection would be incorrect.
        let is_shell = self
            .state
            .focused_terminal(&self.core_state)
            .and_then(|t| t.foreground_process_info())
            .map(|info| crate::click_cursor::is_shell_process(&info.name))
            .unwrap_or(false);
        if !is_shell {
            return;
        }

        let surface_id = match self.state.focused_surface_id(&self.core_state) {
            Some(sid) => sid,
            None => return,
        };

        // Commit any in-progress IME composition before moving cursor.
        // preedit text 도 Intent 큐로 — Intent FIFO 라 arrow 보다 먼저 처리.
        if let Some(preedit) = self.ime_preedit.take()
            && !preedit.text.is_empty()
        {
            self.state.dispatch_intent(
                DomainIntent::SendToSurface {
                    surface_id: preedit.surface_id,
                    payload: SendPayload::Text(preedit.text),
                }
                .from_user_shortcut("click_cursor"),
            );
        }

        let terminal = match self.state.focused_terminal(&self.core_state) {
            Some(t) => t,
            None => return,
        };

        let region = match crate::click_cursor::EditableRegion::from_terminal(terminal) {
            Some(r) => r,
            None => return,
        };

        let (cols, rows) = terminal.dimensions();
        // Use the actual content rect (after tab bar) instead of the raw pane rect
        let surface_rect = match self.state.focused_surface_rect(
            &self.core_state,
            *terminal_rect,
            self.base.gpu.scale_factor(),
        ) {
            Some(r) => r,
            None => return,
        };
        let (click_col, click_row) = crate::click_cursor::pixel_to_grid(
            x,
            y,
            &surface_rect,
            self.base.gpu.cell_width(),
            self.base.gpu.cell_height(),
            cols,
            rows,
        );

        // Clamp to editable region
        let (click_row, click_col) = match region.clamp(click_row, click_col) {
            Some(pos) => pos,
            None => return,
        };

        if click_row == region.cursor_row && click_col == region.cursor_col {
            return;
        }

        let going_right = (click_row, click_col) > (region.cursor_row, region.cursor_col);
        let arrow_count = crate::click_cursor::count_arrows(
            terminal,
            region.cursor_row,
            region.cursor_col,
            click_row,
            click_col,
            cols,
        );

        if arrow_count == 0 {
            return;
        }

        let app_cursor = terminal.application_cursor_keys();
        let arrow: &'static [u8] = if going_right {
            if app_cursor { b"\x1bOC" } else { b"\x1b[C" }
        } else if app_cursor {
            b"\x1bOD"
        } else {
            b"\x1b[D"
        };

        // arrow_count 만큼 sequence 를 한 Vec<u8> 에 concat 후 1 Intent 발행
        // (큐 폭증 회피).
        let mut bytes = Vec::with_capacity(arrow.len() * arrow_count);
        for _ in 0..arrow_count {
            bytes.extend_from_slice(arrow);
        }
        self.state.dispatch_intent(
            DomainIntent::SendToSurface {
                surface_id,
                payload: SendPayload::Bytes(bytes),
            }
            .from_user_shortcut("click_cursor"),
        );
    }

    /// Convert mouse physical coordinates to a grid SelectionPoint for the focused terminal.
    pub(super) fn mouse_to_grid(
        &self,
        x: f32,
        y: f32,
        terminal_rect: &PhysicalRect,
    ) -> Option<(SelectionPoint, u32)> {
        let engine = &self.core_state;
        let surface_id = self.state.focused_surface_id(engine)?;
        // hard 점유(readonly)면 mirror, 아니면 live — 실제 렌더되는 것과 동일 대상을
        // 참조해야 좌표 변환이 화면과 일치한다(ADR-0021).
        let terminal = engine.visible_terminal(surface_id)?;
        // Use the actual content rect (after tab bar) instead of the raw pane rect
        let surface_rect = self.state.focused_surface_rect(
            engine,
            *terminal_rect,
            self.base.gpu.scale_factor(),
        )?;
        let (cols, rows) = terminal.dimensions();
        let point = selection::pixel_to_grid(
            x,
            y,
            &surface_rect,
            self.base.gpu.cell_width(),
            self.base.gpu.cell_height(),
            cols,
            rows,
            terminal.scroll_offset(),
            terminal.scrollback_len(),
        );
        Some((point, surface_id))
    }

    /// 현재 선택을 복사하고 선택 범위는 유지한다. 우클릭 복사는 포커스와 무관하다.
    /// Ctrl+C의 포커스 일치 검사는 호출자인 handle_copy_shortcut에서 한다.
    pub fn copy_selection_to_clipboard(&mut self) -> bool {
        let engine = &mut self.core_state;
        let sel = match &self.text_selection {
            Some(s) if !s.is_empty() => s.clone(),
            _ => return false,
        };
        let text = if let Some(terminal) = engine.visible_terminal(sel.surface_id) {
            selection::extract_selected_text(terminal, &sel)
        } else {
            return false;
        };
        if text.is_empty() {
            return false;
        }
        if let Some(cb) = &mut self.clipboard {
            cb.set_text(&text);
        }
        self.state.toasts.push_info(
            crate::i18n::t("toast.copied"),
            crate::adapters::ui::ToastScope::Surface(sel.surface_id),
        );

        true
    }

    /// 현재 선택의 줄바꿈을 공백 하나로 바꿔 복사한다. soft wrap은 추출할 때 이미 연결된다.
    /// 우클릭 복사와 같이 포커스를 검사하지 않고 선택 범위도 유지한다.
    pub fn copy_selection_no_newline(&mut self) -> bool {
        let engine = &mut self.core_state;
        let sel = match &self.text_selection {
            Some(s) if !s.is_empty() => s.clone(),
            _ => return false,
        };
        let text = if let Some(terminal) = engine.visible_terminal(sel.surface_id) {
            selection::extract_selected_text(terminal, &sel)
        } else {
            return false;
        };
        if text.is_empty() {
            return false;
        }
        let text = text.replace('\n', " ");
        if let Some(cb) = &mut self.clipboard {
            cb.set_text(&text);
        }
        self.state.toasts.push_info(
            crate::i18n::t("toast.copied"),
            crate::adapters::ui::ToastScope::Surface(sel.surface_id),
        );

        true
    }
}

/// 선택한 surface와 포커스가 같은지 확인한다. Ctrl+C에만 적용하며 우클릭 복사는 제외한다.
pub(crate) fn selection_matches_focus(selection_surface_id: u32, focused: Option<u32>) -> bool {
    focused == Some(selection_surface_id)
}

/// 선택이 있고 포커스된 surface에 속하는지 확인한다.
pub(crate) fn should_copy_via_focused_selection(
    selection_surface_id: Option<u32>,
    focused: Option<u32>,
) -> bool {
    selection_surface_id
        .map(|sid| selection_matches_focus(sid, focused))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::{selection_matches_focus, should_copy_via_focused_selection};

    #[test]
    fn matches_when_focused_surface_owns_the_selection() {
        assert!(selection_matches_focus(5, Some(5)));
    }

    #[test]
    fn does_not_match_after_focus_moves_to_another_surface() {
        // 터미널 A(5)에서 드래그 선택 후 Explorer B(2)로 포커스 이동 — A 의 선택은 여전히
        // `text_selection` 에 남아있지만 더 이상 포커스와 일치하지 않는다.
        assert!(!selection_matches_focus(5, Some(2)));
    }

    #[test]
    fn does_not_match_when_nothing_is_focused() {
        assert!(!selection_matches_focus(5, None));
    }

    #[test]
    fn copy_shortcut_uses_selection_when_focus_matches() {
        assert!(should_copy_via_focused_selection(Some(5), Some(5)));
    }

    #[test]
    fn copy_shortcut_skips_stale_selection_after_focus_moves() {
        // 다른 surface에 포커스가 있으면 그 surface의 복사 처리를 사용한다.
        assert!(!should_copy_via_focused_selection(Some(5), Some(2)));
    }

    #[test]
    fn copy_shortcut_skips_when_no_selection_exists() {
        // 선택이 없으면 false다.
        assert!(!should_copy_via_focused_selection(None, Some(2)));
    }
}
