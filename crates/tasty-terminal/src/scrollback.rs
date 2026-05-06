use std::collections::VecDeque;

use termwiz::cell::CellAttributes;

use crate::disk_scrollback;

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

    /// Flush excess scrollback lines to disk (if disk swap is enabled).
    fn flush_to_disk(&mut self) {
        while self.lines.len() > self.limit {
            if let Some(ds) = &mut self.disk {
                if let Some(line) = self.lines.pop_front() {
                    if let Err(e) = ds.push_lines(&[line]) {
                        tracing::warn!("disk scrollback push failed: {e}");
                    }
                }
            } else {
                self.lines.pop_front();
            }
        }
    }
}
