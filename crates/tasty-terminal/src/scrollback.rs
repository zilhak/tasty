use std::collections::VecDeque;

use termwiz::cell::CellAttributes;

use crate::TerminalState;
use crate::disk_scrollback;
use termwiz::surface::Change;

/// One scrollback line with its cells and a soft-wrap flag.
///
/// `wrapped == true` means this line was scrolled off because the terminal
/// auto-wrapped at the right edge — i.e. the next line is a logical
/// continuation, not a hard newline. Used for wrap-aware copy.
#[derive(Debug, Clone)]
pub struct ScrollbackLine {
    pub cells: Vec<(String, CellAttributes)>,
    pub wrapped: bool,
}

impl ScrollbackLine {
    pub fn new(cells: Vec<(String, CellAttributes)>, wrapped: bool) -> Self {
        Self { cells, wrapped }
    }
}

/// Scrollback buffer with optional disk-backed storage.
pub(crate) struct Scrollback {
    lines: VecDeque<ScrollbackLine>,
    limit: usize,
    /// Current scroll offset (0 = at bottom/live, >0 = scrolled up).
    pub scroll_offset: usize,
    disk: Option<disk_scrollback::DiskScrollback>,
}

impl Scrollback {
    pub fn new() -> Self {
        Self {
            lines: VecDeque::new(),
            limit: 10000,
            scroll_offset: 0,
            disk: None,
        }
    }

    /// Set the scrollback buffer limit.
    pub fn set_limit(&mut self, limit: usize) {
        self.limit = limit;
        self.flush_to_disk();
    }

    /// Enable disk-backed scrollback swap.
    pub fn enable_disk(&mut self, surface_id: u32) {
        if self.disk.is_none() {
            match disk_scrollback::DiskScrollback::new(surface_id) {
                Ok(ds) => self.disk = Some(ds),
                Err(e) => tracing::warn!("failed to create disk scrollback: {e}"),
            }
        }
    }

    /// Push a line to the back of the scrollback buffer.
    pub fn push_line(&mut self, line: ScrollbackLine) {
        self.lines.push_back(line);
        self.flush_to_disk();
    }

    /// Pop the most recent line from the back (for rows grow restoration).
    pub fn pop_back(&mut self) -> Option<ScrollbackLine> {
        self.lines.pop_back()
    }

    /// Number of lines in memory only (for resize operations).
    pub fn memory_len(&self) -> usize {
        self.lines.len()
    }

    /// Total number of lines (memory + disk, for public API).
    pub fn total_len(&self) -> usize {
        let disk_count = self.disk.as_ref().map(|ds| ds.line_count()).unwrap_or(0);
        disk_count + self.lines.len()
    }

    /// Scroll up (towards older content).
    pub fn scroll_up(&mut self, count: usize) {
        let max = self.total_len();
        self.scroll_offset = (self.scroll_offset + count).min(max);
    }

    /// Scroll down (towards newer/live content).
    pub fn scroll_down(&mut self, count: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(count);
    }

    /// Reset scroll position to the bottom (live view).
    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
    }

    /// Get a specific scrollback line's cells by index (0 = oldest, memory only).
    /// For disk-backed lines, use `line_owned()`.
    pub fn line(&self, index: usize) -> Option<&Vec<(String, CellAttributes)>> {
        let disk_count = self.disk.as_ref().map(|ds| ds.line_count()).unwrap_or(0);
        if index < disk_count {
            None // Disk lines can't be returned as reference — use line_owned()
        } else {
            self.lines.get(index - disk_count).map(|l| &l.cells)
        }
    }

    /// Get a scrollback line's cells by index, returning owned data.
    /// Works for both memory and disk-backed lines.
    pub fn line_owned(&self, index: usize) -> Option<Vec<(String, CellAttributes)>> {
        let disk_count = self.disk.as_ref().map(|ds| ds.line_count()).unwrap_or(0);
        if index < disk_count {
            self.disk
                .as_ref()
                .and_then(|ds| ds.read_line(index).ok().flatten())
                .map(|l| l.cells)
        } else {
            self.lines.get(index - disk_count).map(|l| l.cells.clone())
        }
    }

    /// Returns `true` if the line at `index` was soft-wrapped (auto-wrap at
    /// the right edge of the terminal), meaning the next line is a logical
    /// continuation rather than a hard newline.
    pub fn line_wrapped(&self, index: usize) -> Option<bool> {
        let disk_count = self.disk.as_ref().map(|ds| ds.line_count()).unwrap_or(0);
        if index < disk_count {
            self.disk
                .as_ref()
                .and_then(|ds| ds.read_line(index).ok().flatten())
                .map(|l| l.wrapped)
        } else {
            self.lines.get(index - disk_count).map(|l| l.wrapped)
        }
    }

    /// Discard all scrollback (memory + disk) and snap the viewport back to
    /// live. Used by ED3 (`CSI 3J`) erase-scrollback.
    pub fn clear(&mut self) {
        self.lines.clear();
        self.scroll_offset = 0;
        if let Some(ds) = &mut self.disk
            && let Err(e) = ds.clear()
        {
            tracing::warn!("disk scrollback clear failed: {e}");
        }
    }

    /// Flush excess scrollback lines to disk (if disk swap is enabled).
    pub(crate) fn flush_to_disk(&mut self) {
        while self.lines.len() > self.limit {
            if let Some(ds) = &mut self.disk {
                if let Some(line) = self.lines.pop_front()
                    && let Err(e) = ds.push_lines(&[line])
                {
                    tracing::warn!("disk scrollback push failed: {e}");
                }
            } else {
                self.lines.pop_front();
            }
        }
    }
}

impl TerminalState {
    /// Current scroll offset (0 = at bottom/live, >0 = scrolled up).
    pub fn scroll_offset(&self) -> usize {
        self.scrollback.scroll_offset
    }

    /// Set the scrollback buffer limit.
    pub fn set_scrollback_limit(&mut self, limit: usize) {
        self.scrollback.set_limit(limit);
    }

    /// Enable disk-backed scrollback swap for this terminal.
    pub fn enable_disk_scrollback(&mut self, surface_id: u32) {
        self.scrollback.enable_disk(surface_id);
    }

    /// Scroll up (towards older content).
    pub fn scroll_up(&mut self, lines: usize) {
        self.scrollback.scroll_up(lines);
    }

    /// Scroll down (towards newer/live content).
    pub fn scroll_down(&mut self, lines: usize) {
        self.scrollback.scroll_down(lines);
    }

    /// Reset scroll position to the bottom (live view).
    pub fn scroll_to_bottom(&mut self) {
        self.scrollback.scroll_to_bottom();
    }

    /// Set scroll offset directly (for search navigation).
    /// Clamped to [0, scrollback_len].
    pub fn set_scroll_offset(&mut self, offset: usize) {
        let max = self.scrollback.total_len();
        self.scrollback.scroll_offset = offset.min(max);
    }

    /// Number of lines in the scrollback buffer (memory + disk).
    pub fn scrollback_len(&self) -> usize {
        self.scrollback.total_len()
    }

    /// Erase all scrollback history (ED3 / `CSI 3J`). Clears the memory + disk
    /// buffers and snaps the user viewport to live. The visible screen and its
    /// `saved_line_tails` (per-row reflow data) are left untouched — ED3 only
    /// touches scrollback. `restorable_scrollback_count` is reset because the
    /// resize "restore debt" lines it counted have just been discarded; leaving
    /// it set would let a later rows-grow wrongly pull fresh scrollback content
    /// back onto the screen.
    pub(crate) fn clear_scrollback(&mut self) {
        self.scrollback.clear();
        self.restorable_scrollback_count = 0;
    }

    /// Get a specific scrollback line by index (0 = oldest, memory only).
    /// For disk-backed lines, use scrollback_line_owned().
    pub fn scrollback_line(&self, index: usize) -> Option<&Vec<(String, CellAttributes)>> {
        self.scrollback.line(index)
    }

    /// Get a scrollback line by index, returning owned data.
    /// Works for both memory and disk-backed lines.
    pub fn scrollback_line_owned(&self, index: usize) -> Option<Vec<(String, CellAttributes)>> {
        self.scrollback.line_owned(index)
    }

    /// Returns whether the scrollback line at `index` ends in a soft wrap
    /// (auto-wrap at the right edge). Used by selection extraction to rejoin
    /// wrapped lines on copy. Returns `None` if `index` is out of range.
    pub fn scrollback_line_wrapped(&self, index: usize) -> Option<bool> {
        self.scrollback.line_wrapped(index)
    }

    /// Get a full scrollback line by index (cells + wrapped flag).
    pub fn scrollback_line_full(&self, index: usize) -> Option<crate::ScrollbackLine> {
        let cells = self.scrollback.line_owned(index)?;
        let wrapped = self.scrollback.line_wrapped(index).unwrap_or(false);
        Some(crate::ScrollbackLine::new(cells, wrapped))
    }

    /// Snapshot the current visible screen as `ScrollbackLine` records.
    /// Trailing blank rows (모두 공백 / 빈 cell) 은 잘라낸다.
    ///
    /// 사용처: layout 저장 시 scrollback 라인 뒤에 이어 붙여 디스크에 저장한다.
    /// 복원 시 `inject_scrollback` 으로 함께 push 하면 다음 세션에서 위로
    /// 스크롤할 때 이전 화면이 그대로 보인다 (현재 PTY 의 새 prompt 는 그 아래
    /// 에서 시작).
    pub fn screen_snapshot_lines(&self) -> Vec<crate::ScrollbackLine> {
        let surface = self.surface();
        let cols = self.cols;
        let lines = surface.screen_lines();
        let mut result: Vec<crate::ScrollbackLine> = Vec::with_capacity(lines.len());
        for line in lines.iter() {
            let cells: Vec<(String, CellAttributes)> = line
                .visible_cells()
                .map(|cell| (cell.str().to_string(), cell.attrs().clone()))
                .collect();
            let wrapped = Self::line_was_soft_wrapped(line, cols);
            result.push(crate::ScrollbackLine::new(cells, wrapped));
        }
        // Trim trailing blank rows: cells 가 비거나 모두 whitespace-only.
        while let Some(last) = result.last() {
            let blank = last
                .cells
                .iter()
                .all(|(s, _)| s.is_empty() || s.chars().all(char::is_whitespace));
            if blank {
                result.pop();
            } else {
                break;
            }
        }
        result
    }

    /// Inject scrollback lines (oldest first) into this terminal's scrollback buffer.
    /// Used to restore scrollback after recreating a terminal (closed-item / layout restore).
    pub fn inject_scrollback(&mut self, lines: Vec<crate::ScrollbackLine>) {
        for line in lines {
            self.scrollback.push_line(line);
        }
    }

    /// Pop up to `count` lines from the back of scrollback and draw them at the
    /// top of the visible screen, then place the cursor on the row immediately
    /// below them. Returns the number of lines actually drawn (saturates when
    /// scrollback is shorter than `count` or `count` exceeds available rows).
    ///
    /// 복원 직후 호출용 — 사용자가 "위에 더 있다" 는 사실을 인지하도록 visible
    /// 영역 상단에 옛 라인을 미리 보여주고 새 prompt 가 그 아래에서 시작하게
    /// 만든다. PTY 가 첫 prompt 를 출력하기 전에 호출해야 한다.
    pub fn prefill_visible_from_scrollback(&mut self, count: usize) -> usize {
        use termwiz::surface::Position;

        let count = count
            .min(self.scrollback.memory_len())
            .min(self.rows.saturating_sub(1));
        if count == 0 {
            return 0;
        }

        let mut to_draw: Vec<crate::ScrollbackLine> = Vec::with_capacity(count);
        for _ in 0..count {
            if let Some(line) = self.scrollback.pop_back() {
                to_draw.push(line);
            }
        }
        to_draw.reverse(); // oldest first

        for (row, line) in to_draw.iter().enumerate() {
            self.primary_surface.add_change(Change::CursorPosition {
                x: Position::Absolute(0),
                y: Position::Absolute(row),
            });
            for (text, attrs) in &line.cells {
                self.primary_surface
                    .add_change(Change::AllAttributes(attrs.clone()));
                self.primary_surface.add_change(Change::Text(text.clone()));
            }
        }

        // Park the cursor on the row right after the prefilled block so the
        // shell's first prompt is emitted there.
        let cursor_y = to_draw.len();
        self.primary_surface.add_change(Change::CursorPosition {
            x: Position::Absolute(0),
            y: Position::Absolute(cursor_y),
        });

        to_draw.len()
    }

    /// Capture the top line(s) from the surface before a scroll change is applied.
    ///
    /// Each captured line is tagged with a `wrapped` flag so the next line is
    /// known to be a logical continuation (used by wrap-aware copy). termwiz
    /// `Surface::print_text` does NOT set its own wrap bit when the cursor
    /// runs off the right edge — it just advances `ypos` — so we recover the
    /// flag heuristically: a line is treated as soft-wrapped when its
    /// rightmost cell is occupied by a non-space grapheme. Lines that ended in
    /// a real `\n` almost always have trailing whitespace; lines that wrapped
    /// at the right edge filled the last column. False positives (a hard
    /// newline that happened to fill the row) merge two lines on copy, which
    /// is a strictly better outcome than the prior unconditional `\n` join.
    pub(crate) fn capture_top_lines(&self, count: usize) -> Vec<crate::scrollback::ScrollbackLine> {
        let surface = self.surface();
        let cols = self.cols;
        let lines = surface.screen_lines();
        let mut result = Vec::new();
        for line in lines.iter().take(count.min(lines.len())) {
            let cells: Vec<(String, CellAttributes)> = line
                .visible_cells()
                .map(|cell| (cell.str().to_string(), cell.attrs().clone()))
                .collect();
            let wrapped = Self::line_was_soft_wrapped(line, cols);
            result.push(crate::scrollback::ScrollbackLine::new(cells, wrapped));
        }
        result
    }

    /// Push lines that scrolled off the top into scrollback and keep the
    /// viewport-relative bookkeeping consistent (line tails + scroll offset).
    pub(crate) fn push_scrolled_off(&mut self, captured: Vec<crate::scrollback::ScrollbackLine>) {
        let count = captured.len();
        for line in captured {
            self.scrollback.push_line(line);
        }
        // Shift saved_line_tails: remove top entries that scrolled off
        for _ in 0..count.min(self.saved_line_tails.len()) {
            self.saved_line_tails.remove(0);
        }
        // Compensate scroll_offset so the user's viewport stays in place
        if self.scrollback.scroll_offset > 0 {
            self.scrollback.scroll_offset += count;
        }
    }

    /// Inspect a change and capture scrollback lines before it's applied.
    /// Only the explicit `ScrollRegionUp` path is handled here; the implicit
    /// scroll caused by auto-wrapping text is handled in
    /// [`apply_text_capturing_scrolls`] because the evicted rows are produced
    /// mid-apply and would not exist in a pre-apply snapshot.
    pub(crate) fn capture_before_scroll(&mut self, change: &Change) {
        if let Change::ScrollRegionUp {
            first_row: 0,
            region_size,
            scroll_count,
        } = change
        {
            // termwiz `scroll_region_up` evicts at most `region_size` rows. When
            // `scroll_count` exceeds the region (a top-anchored partial region with
            // an over-sized SU), capturing `scroll_count` rows would also copy rows
            // *below* the region — which stay on screen — producing duplicates in
            // scrollback. Clamp to the region so only genuinely evicted top rows
            // are captured (see ADR/verification: E2). The clamp lives here at the
            // callsite, not in `capture_top_lines`, because the auto-wrap path
            // (`apply_text_capturing_scrolls`) calls `capture_top_lines(1)` and is
            // region-agnostic.
            let count = (*scroll_count).min(*region_size);
            let captured = self.capture_top_lines(count);
            self.push_scrolled_off(captured);
        }
    }

    /// Apply a `Change::Text`, capturing every row that termwiz scrolls off the
    /// top while rendering it.
    ///
    /// termwiz `Surface::print_text` scrolls the grid *internally* (no
    /// `ScrollRegionUp` Change) whenever auto-wrap or a newline runs past the
    /// bottom row, discarding the evicted top line. Tasty owns the scrollback,
    /// so we must observe each eviction. The evicted content is generated within
    /// the same `add_change`, so a pre-apply snapshot is empty — instead we
    /// split the text at the exact byte offsets where a scroll will occur, apply
    /// each segment, and snapshot the (now-populated) top row right before the
    /// scroll consumes it.
    pub(crate) fn apply_text_capturing_scrolls(&mut self, text: String) {
        let offsets = self.text_scroll_offsets(&text);
        if offsets.is_empty() {
            self.surface_mut().add_change(Change::Text(text));
            return;
        }
        let mut prev = 0usize;
        for off in offsets {
            if off > prev {
                self.surface_mut()
                    .add_change(Change::Text(text[prev..off].to_string()));
            }
            // The top row is about to scroll off when the next segment's first
            // grapheme is applied — snapshot it now.
            let captured = self.capture_top_lines(1);
            self.push_scrolled_off(captured);
            prev = off;
        }
        if prev < text.len() {
            self.surface_mut()
                .add_change(Change::Text(text[prev..].to_string()));
        }
    }

    /// Simulate termwiz `print_text` cursor advancement to find the byte offset
    /// of every grapheme whose application scrolls the grid. Mirrors termwiz's
    /// deferred-wrap logic exactly (same grapheme segmentation and column width)
    /// so the offsets line up with the real scrolls.
    fn text_scroll_offsets(&self, text: &str) -> Vec<usize> {
        use finl_unicode::grapheme_clusters::Graphemes;

        let mut offsets = Vec::new();
        let width = self.cols;
        let height = self.rows;
        if width == 0 || height == 0 {
            return offsets;
        }
        let (mut xpos, mut ypos) = self.surface().cursor_position();
        let mut byte = 0usize;
        for g in Graphemes::new(text) {
            let at_bottom = ypos + 1 >= height;
            let scrolls = match g {
                "\r" => false,
                "\r\n" | "\n" => at_bottom,
                _ => xpos >= width && at_bottom,
            };
            if scrolls {
                offsets.push(byte);
            }
            // Advance state exactly as termwiz print_text does.
            match g {
                "\r" => xpos = 0,
                "\r\n" => {
                    xpos = 0;
                    if !at_bottom {
                        ypos += 1;
                    }
                }
                "\n" => {
                    if !at_bottom {
                        ypos += 1;
                    }
                }
                _ => {
                    if xpos >= width {
                        if !at_bottom {
                            ypos += 1;
                        }
                        xpos = 0;
                    }
                    xpos += termwiz::cell::grapheme_column_width(g, None).max(1);
                }
            }
            byte += g.len();
        }
        offsets
    }
}
