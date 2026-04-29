use std::collections::VecDeque;

use termwiz::cell::CellAttributes;

use crate::disk_scrollback;

/// Scrollback buffer with optional disk-backed storage.
pub(crate) struct Scrollback {
    lines: VecDeque<Vec<(String, CellAttributes)>>,
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
    pub fn push_line(&mut self, line: Vec<(String, CellAttributes)>) {
        self.lines.push_back(line);
        self.flush_to_disk();
    }

    /// Pop the most recent line from the back (for rows grow restoration).
    pub fn pop_back(&mut self) -> Option<Vec<(String, CellAttributes)>> {
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

    /// Get a specific scrollback line by index (0 = oldest, memory only).
    /// For disk-backed lines, use `line_owned()`.
    pub fn line(&self, index: usize) -> Option<&Vec<(String, CellAttributes)>> {
        let disk_count = self.disk.as_ref().map(|ds| ds.line_count()).unwrap_or(0);
        if index < disk_count {
            None // Disk lines can't be returned as reference — use line_owned()
        } else {
            self.lines.get(index - disk_count)
        }
    }

    /// Get a scrollback line by index, returning owned data.
    /// Works for both memory and disk-backed lines.
    pub fn line_owned(&self, index: usize) -> Option<Vec<(String, CellAttributes)>> {
        let disk_count = self.disk.as_ref().map(|ds| ds.line_count()).unwrap_or(0);
        if index < disk_count {
            self.disk
                .as_ref()
                .and_then(|ds| ds.read_line(index).ok().flatten())
        } else {
            self.lines.get(index - disk_count).cloned()
        }
    }

    /// Flush excess scrollback lines to disk (if disk swap is enabled).
    fn flush_to_disk(&mut self) {
        while self.lines.len() > self.limit {
            if let Some(ds) = &mut self.disk {
                if let Some(line) = self.lines.pop_front() {
                    let _ = ds.push_lines(&[line]);
                }
            } else {
                self.lines.pop_front();
            }
        }
    }
}
