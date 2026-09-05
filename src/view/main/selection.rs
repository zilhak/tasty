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
        // read between locks (ADR-0002).
        let row_text: Vec<(String, usize)> =
            match terminal.with_render_view(|v| -> Option<Vec<(String, usize)>> {
                let scrollback_len = v.scrollback_len();
                if absolute_row < scrollback_len {
                    v.scrollback_line(absolute_row).map(|line| {
                        let mut result = Vec::new();
                        let mut c = 0;
                        for (text, _) in line.cells() {
                            let ch = text.chars().next().unwrap_or(' ');
                            let w = crate::renderer::unicode_width(ch);
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
        let end_col = row_text[end].1 + crate::renderer::unicode_width(end_ch) - 1;
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
        // 참조해야 좌표 변환이 화면과 일치한다(ADR-0049).
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

    /// Copy the current selection to clipboard. Selection is preserved.
    ///
    /// No focus check here — the terminal right-click menu's "copy" item
    /// (`redraw.rs`, see the comment above its `has_selection` gate) is a long-standing
    /// convention that operates on the global `text_selection` regardless of which
    /// surface is focused (right-click doesn't move focus, so "select in A, right-click
    /// B, copy A's selection" must keep working). The keyboard Ctrl+C path is the one
    /// exception that needs focus to match — that check lives in its caller,
    /// `handle_copy_shortcut` (`copy_paste.rs`), via `selection_matches_focus`, not here.
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

    /// Copy the current selection to clipboard, collapsing every line break into
    /// a single space. Soft wraps are already joined by `extract_selected_text`;
    /// here we additionally replace each hard `\n` with one space so a multi-line
    /// selection becomes a single space-separated line. Selection is preserved.
    ///
    /// Same convention as `copy_selection_to_clipboard`: no focus check. This is the
    /// terminal right-click menu's "copy, no newline" item, which — like "copy" — is
    /// deliberately surface-independent (`redraw.rs`, comment above the `has_selection`
    /// gate). This function has no keyboard-shortcut caller, so there's no focus-mismatch
    /// case to guard against here.
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

/// `text_selection` 이 가리키는 surface 가 지금 포커스된 surface 와 같은지. `focused` 는
/// 이미 조회된 `AppState::focused_surface_id` 결과를 받는다(이 fn 자체는 조회를 하지
/// 않는 순수 판정이라 `MainView` 없이 유닛 테스트 가능). 키보드 Ctrl+C 경로
/// (`handle_copy_shortcut`, `adapters/ui/input/shortcuts/copy_paste.rs`)가
/// `copy_selection_to_clipboard` 를 부르기 전에 이 fn 으로 포커스 일치를 먼저 확인한다 —
/// 터미널 우클릭 메뉴 관례(surface 무관 전역 selection)를 건드리지 않기 위해 체크를
/// 공유 함수가 아니라 키보드 호출부에 둔다.
pub(crate) fn selection_matches_focus(selection_surface_id: u32, focused: Option<u32>) -> bool {
    focused == Some(selection_surface_id)
}

/// `handle_copy_shortcut`(`adapters/ui/input/shortcuts/copy_paste.rs`)가 `text_selection`이
/// 있을 때만 `selection_matches_focus`로 넘기는 `Option` 래핑 판정을 그대로 뽑아낸 순수
/// 함수. `MainView`를 만들지 않고도 "selection 없음" / "selection은 있지만 포커스가
/// 다른 surface" 두 케이스를 함께 검증하기 위해 존재한다 — `handle_copy_shortcut` 자체는
/// clipboard/text_selection 등 `MainView` 필드가 필요해 이 fn 처럼 직접 유닛 테스트할
/// 헤드리스 하네스가 없다.
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
        // 터미널 A(5) 드래그 선택 → Explorer B(2)로 포커스 이동 → Ctrl+C. handle_copy_shortcut
        // 은 이 경우 false 를 얻어 copy_selection_to_clipboard 를 호출하지 않고 다음 분기
        // (Explorer 자신의 copy 처리)로 흘려보내야 한다.
        assert!(!should_copy_via_focused_selection(Some(5), Some(2)));
    }

    #[test]
    fn copy_shortcut_skips_when_no_selection_exists() {
        // text_selection 이 None 이면 selection_matches_focus 를 호출할 대상 자체가 없다 —
        // Option 래핑이 이 케이스를 안전하게 false 로 단락시키는지 확인한다.
        assert!(!should_copy_via_focused_selection(None, Some(2)));
    }
}
