//! `Terminal` 핸들의 grid/VTE 상태 접근자 — 공유 `TerminalState` 락을 잡아
//! 위임하는 thin wrapper 들 (ADR-0002). 외부(`src/`)의 `terminal.X()` 호출처가
//! 그대로 동작하도록 기존 `Terminal` API 시그니처를 보존한다.

use termwiz::cell::CellAttributes;
use termwiz::surface::line::Line;

use crate::search::{SearchError, SearchMatch, SearchOptions};
use crate::{CellInfo, CursorShape, MouseTrackingMode, ScrollbackLine, Terminal};

impl Terminal {
    // ── Surface (owned snapshots — guard 밖으로 ref 를 빼낼 수 없으므로 복제) ──

    /// Active surface dimensions `(cols, rows)`.
    pub fn dimensions(&self) -> (usize, usize) {
        self.with_surface(|s| s.dimensions())
    }

    /// Active surface cursor position `(col, row)`.
    pub fn cursor_position(&self) -> (usize, usize) {
        self.with_surface(|s| s.cursor_position())
    }

    /// Snapshot of the active surface's visible lines (owned, since the borrow
    /// cannot escape the state lock).
    pub fn screen_lines(&self) -> Vec<Line> {
        self.with_surface(|s| {
            s.screen_lines()
                .into_iter()
                .map(|c| c.into_owned())
                .collect()
        })
    }

    // ── Modes ──

    pub fn application_cursor_keys(&self) -> bool {
        self.lock_state().application_cursor_keys()
    }

    pub fn cursor_visible(&self) -> bool {
        self.lock_state().cursor_visible()
    }

    pub fn cursor_shape(&self) -> CursorShape {
        self.lock_state().cursor_shape()
    }

    /// Whether reverse-screen mode (DECSCNM) is active.
    pub fn screen_reverse(&self) -> bool {
        self.lock_state().screen_reverse()
    }

    pub fn bracketed_paste(&self) -> bool {
        self.lock_state().bracketed_paste()
    }

    pub fn mouse_tracking(&self) -> MouseTrackingMode {
        self.lock_state().mouse_tracking()
    }

    /// 무장된 "첫 마우스 캡처 안내" 플래그를 소비한다(읽고 disarm). 트래킹 세션당 1회 true.
    pub fn take_mouse_capture_hint(&self) -> bool {
        self.lock_state().take_mouse_capture_hint()
    }

    pub fn sgr_mouse(&self) -> bool {
        self.lock_state().sgr_mouse()
    }

    pub fn focus_tracking(&self) -> bool {
        self.lock_state().focus_tracking()
    }

    pub fn is_alternate_screen(&self) -> bool {
        self.lock_state().is_alternate_screen()
    }

    pub fn synchronized_output(&self) -> bool {
        self.lock_state().synchronized_output()
    }

    pub fn find_fake_cursor_cell(&self) -> Option<(usize, usize)> {
        self.lock_state().find_fake_cursor_cell()
    }

    // ── Screen / cell inspection ──

    pub fn screen_text(&self, include_dim: bool) -> String {
        self.lock_state().screen_text(include_dim)
    }

    pub fn screen_text_lines(&self, n: usize, include_dim: bool) -> String {
        self.lock_state().screen_text_lines(n, include_dim)
    }

    pub fn screen_row(&self, row: usize, include_dim: bool) -> String {
        self.lock_state().screen_row(row, include_dim)
    }

    pub fn cell_info(&self, row: usize, col: usize) -> Option<CellInfo> {
        self.lock_state().cell_info(row, col)
    }

    pub fn row_cells(&self, row: usize) -> Vec<(usize, CellInfo)> {
        self.lock_state().row_cells(row)
    }

    pub fn cell_attrs(&self, row: usize, col: usize) -> Option<CellAttributes> {
        self.lock_state().cell_attrs(row, col)
    }

    // ── Snapshot / search ──

    pub fn snapshot_as_vt(&self) -> Vec<u8> {
        self.lock_state().snapshot_as_vt()
    }

    pub fn search(
        &self,
        query: &str,
        options: &SearchOptions,
    ) -> Result<Vec<SearchMatch>, SearchError> {
        self.lock_state().search(query, options)
    }

    // ── Scrollback ──

    pub fn scroll_offset(&self) -> usize {
        self.lock_state().scroll_offset()
    }

    pub fn set_scrollback_limit(&mut self, limit: usize) {
        self.lock_state().set_scrollback_limit(limit);
    }

    pub fn enable_disk_scrollback(&mut self, surface_id: u32) {
        self.lock_state().enable_disk_scrollback(surface_id);
    }

    pub fn scroll_up(&mut self, lines: usize) {
        self.lock_state().scroll_up(lines);
    }

    pub fn scroll_down(&mut self, lines: usize) {
        self.lock_state().scroll_down(lines);
    }

    pub fn scroll_to_bottom(&mut self) {
        self.lock_state().scroll_to_bottom();
    }

    pub fn set_scroll_offset(&mut self, offset: usize) {
        self.lock_state().set_scroll_offset(offset);
    }

    pub fn scrollback_len(&self) -> usize {
        self.lock_state().scrollback_len()
    }

    pub fn scrollback_line_owned(&self, index: usize) -> Option<Vec<(String, CellAttributes)>> {
        self.lock_state().scrollback_line_owned(index)
    }

    pub fn scrollback_line_wrapped(&self, index: usize) -> Option<bool> {
        self.lock_state().scrollback_line_wrapped(index)
    }

    pub fn scrollback_line_full(&self, index: usize) -> Option<ScrollbackLine> {
        self.lock_state().scrollback_line_full(index)
    }

    pub fn screen_snapshot_lines(&self) -> Vec<ScrollbackLine> {
        self.lock_state().screen_snapshot_lines()
    }

    pub fn inject_scrollback(&mut self, lines: Vec<ScrollbackLine>) {
        self.lock_state().inject_scrollback(lines);
    }

    pub fn prefill_visible_from_scrollback(&mut self, count: usize) -> usize {
        self.lock_state().prefill_visible_from_scrollback(count)
    }
}
